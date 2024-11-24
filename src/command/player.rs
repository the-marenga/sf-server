use std::fmt::Write;

use fastrand::Rng;
use num_traits::FromPrimitive;
use sf_api::gamestate::character::{Gender, Race};
use sqlx::Sqlite;

use super::{
    debug::{handle_cheat_command, CheatCmd},
    effective_mount, in_seconds, now, poll, xp_for_next_level,
    CommandArguments, Portrait, ResponseBuilder, ServerError, ServerResponse,
};
use crate::{misc::from_sf_string, request::Session};

pub(crate) async fn player_mount_buy(
    session: Session,
    db: &sqlx::Pool<Sqlite>,
    args: CommandArguments<'_>,
) -> Result<ServerResponse, ServerError> {
    let mount = args.get_int(0, "mount")?;
    let mut tx = db.begin().await?;

    let character = sqlx::query!(
        "SELECT silver, mushrooms, mount, mount_end FROM CHARACTER WHERE pid \
         = $1",
        session.player_id
    )
    .fetch_one(&mut *tx)
    .await?;

    let mut silver = character.silver;
    let mut mushrooms = character.mushrooms;

    let price = match mount {
        0 => 0,
        1 => 100,
        2 => 500,
        3 => 0,
        4 => 0, // TODO: Silver reward
        _ => return Err(ServerError::BadRequest),
    };

    let mush_price = match mount {
        3 => 1,
        4 => 25,
        _ => 0,
    };
    if mushrooms < mush_price {
        return Err(ServerError::NotEnoughMoney);
    }
    mushrooms -= mush_price;

    if silver < price {
        return Err(ServerError::NotEnoughMoney);
    }
    silver -= price;

    let now = now();
    let mount_duration = 60 * 60 * 24 * 14;
    let mount_end = if mount != character.mount || character.mount_end < now {
        now + mount_duration
    } else {
        character.mount_end + mount_duration
    };

    sqlx::query!(
        "UPDATE Character SET mount = $1, mount_end = $2, mushrooms = $4, \
         silver = $5 WHERE pid = $3",
        mount,
        mount_end,
        session.player_id,
        mushrooms,
        silver,
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    poll(session, "", db, Default::default()).await
}

pub(crate) async fn player_tutorial(
    session: Session,
    db: &sqlx::Pool<Sqlite>,
    args: CommandArguments<'_>,
) -> Result<ServerResponse, ServerError> {
    let status = args.get_int(0, "tutorial status")?;
    if !(0..=0xFFFFFFF).contains(&status) {
        Err(ServerError::BadRequest)?;
    }
    sqlx::query!(
        "UPDATE CHARACTER SET tutorial_status = $1 WHERE pid = $2", status,
        session.player_id,
    )
    .execute(db)
    .await?;
    Ok(ServerResponse::Success)
}

pub(crate) async fn player_whisper(
    session: Session,
    db: &sqlx::Pool<Sqlite>,
    args: CommandArguments<'_>,
) -> Result<ServerResponse, ServerError> {
    let name = args.get_str(0, "name")?.to_lowercase();
    if name != "server" {
        todo!()
    }
    use clap::Parser;
    let command = CheatCmd::try_parse_from(args.get_str(1, "args")?.split(' '))
        .map_err(|_| ServerError::BadRequest)?;
    handle_cheat_command(session, db, command).await
}

pub(crate) async fn player_finish_quest(
    session: Session,
    db: &sqlx::Pool<Sqlite>,
) -> Result<ServerResponse, ServerError> {
    let mut tx = db.begin().await?;

    let row = sqlx::query!(
        "SELECT
        activity.typ,
        activity.busy_until,
        activity.sub_type,

        q1.item as q1item,
        q1.length as q1length,
        q1.Location as q1location,
        q1.Monster as q1monster,
        q1.Mushrooms as q1mush,
        q1.Silver as q1silver,
        q1.XP as q1xp,

        q2.item as q2item,
        q2.length as q2length,
        q2.Location as q2location,
        q2.Monster as q2monster,
        q2.Mushrooms as q2mush,
        q2.SILVER as q2silver,
        q2.XP as q2xp,

        q3.item as q3item,
        q3.length as q3length,
        q3.Location as q3location,
        q3.Monster as q3monster,
        q3.Mushrooms as q3mush,
        q3.SILVER as q3silver,
        q3.XP as q3xp,

        level,
        name,

        portrait.mouth,
        portrait.hair,
        portrait.brows,
        portrait.eyes,
        portrait.beards,
        portrait.nose,
        portrait.ears,
        portrait.extra,
        portrait.horns,

        race,
        gender,
        class,
        experience,
        portrait.influencer

        FROM character
            NATURAL JOIN PORTRAIT
            NATURAL JOIN tavern
            NATURAL JOIN activity
            JOIN quest as q1 on tavern.quest1 = q1.id
            JOIN quest as q2 on tavern.quest2 = q2.id
            JOIN quest as q3 on tavern.quest2 = q3.id
            WHERE pid = $1",
        session.player_id,
    )
    .fetch_one(&mut *tx)
    .await?;

    if row.typ != 2 {
        // We are not actually questing
        return Err(ServerError::StillBusy);
    }
    let busyuntil = row.busy_until;

    if busyuntil > now() {
        // Quest is still going
        return Err(ServerError::StillBusy);
    }

    let subtyp = row.sub_type;

    let (_item, location, monster, mush, silver, quest_xp) = match subtyp {
        1 => (
            row.q1item, row.q1location, row.q1monster, row.q1mush,
            row.q1silver, row.q1xp,
        ),
        2 => (
            row.q2item, row.q2location, row.q2monster, row.q2mush,
            row.q2silver, row.q2xp,
        ),
        3 => (
            row.q3item, row.q3location, row.q3monster, row.q3mush,
            row.q3silver, row.q3xp,
        ),
        _ => todo!(),
    };

    let honor_won = 10;

    let mut resp = ResponseBuilder::default();

    resp.add_key("fightresult.battlereward");
    resp.add_val(true as u8);
    // won
    resp.add_val(0);
    resp.add_val(silver);
    resp.add_val(quest_xp);

    resp.add_val(mush);
    resp.add_val(honor_won);
    for _ in 0..15 {
        resp.add_val(0);
    }

    resp.add_key("fightheader.fighters");
    let monster_id = -monster;
    let mut character_lvl = row.level;
    let starting_character_xp = row.experience;

    let mut total_xp = quest_xp + starting_character_xp;
    let mut required_xp = xp_for_next_level(character_lvl);
    // Level up the character
    while total_xp > required_xp {
        character_lvl += 1;
        total_xp -= required_xp;
        required_xp = xp_for_next_level(character_lvl);
    }

    let character_attributes = [1, 1, 1, 1, 1];
    let monster_attributes = [1, 1, 1, 1, 1];
    let monster_hp = 10_000;
    let character_hp = 10_000;

    resp.add_val(1);
    resp.add_val(0);
    resp.add_val(0);

    // Location
    resp.add_val(location);

    resp.add_val(1);
    resp.add_val(session.player_id);
    resp.add_str(&row.name);
    resp.add_val(character_lvl);
    for _ in 0..2 {
        resp.add_val(character_hp);
    }
    for val in character_attributes {
        resp.add_val(val);
    }

    resp.add_val(row.mouth);
    resp.add_val(row.hair);
    resp.add_val(row.brows);
    resp.add_val(row.eyes);
    resp.add_val(row.beards);
    resp.add_val(row.nose);
    resp.add_val(row.ears);
    resp.add_val(row.extra);
    resp.add_val(row.horns);

    resp.add_val(row.influencer);
    // special influencer portraits

    resp.add_val(row.race);
    // race
    resp.add_val(row.gender);
    // gender
    resp.add_val(row.class);
    // class

    // Main weapon
    for _ in 0..12 {
        resp.add_val(0);
    }

    // Sub weapon
    for _ in 0..12 {
        resp.add_val(0);
    }

    // Monster
    for _ in 0..2 {
        resp.add_val(monster_id);
    }
    resp.add_val(character_lvl);
    // monster lvl
    resp.add_val(monster_hp);
    resp.add_val(monster_hp);
    for attr in monster_attributes {
        resp.add_val(attr);
    }
    resp.add_val(monster_id);
    for _ in 0..11 {
        resp.add_val(0);
    }
    resp.add_val(3);
    // Class?

    // Probably also items
    // This means just changing the portrait into the character
    resp.add_val(-1);
    for _ in 0..23 {
        resp.add_val(0);
    }

    resp.add_key("fight.r");
    resp.add_str(&format!("{},0,-1000", session.player_id));

    // TODO: actually simulate fight

    resp.add_key("winnerid");
    resp.add_val(session.player_id);

    resp.add_key("fightversion");
    resp.add_val(1);

    sqlx::query!(
        "UPDATE activity
                 SET typ = 0, sub_type = 0, started = 0, busy_until = 0
                 WHERE pid = $1",
        session.player_id,
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query!(
        "UPDATE character
                 SET silver = silver + $2, mushrooms = mushrooms + $3, honor = \
         honor + $4, level = $5, Experience = $6
                 WHERE pid = $1",
        session.player_id,
        silver,
        mush,
        honor_won,
        character_lvl,
        total_xp
    )
    .execute(&mut *tx)
    .await?;

    // TODO: Reroll quests, add item & save fight somewhere for rewatch (save)

    tx.commit().await?;

    poll(session, "", db, resp).await
}

pub(crate) async fn player_start_quest(
    session: Session,
    db: &sqlx::Pool<Sqlite>,
    args: CommandArguments<'_>,
) -> Result<ServerResponse, ServerError> {
    let quest = args.get_int(0, "quest")?;
    let skip_inv = args.get_int(1, "skip_inv")?;

    if !(1..=3).contains(&quest) || !(0..=1).contains(&skip_inv) {
        Err(ServerError::BadRequest)?;
    }

    let mut tx = db.begin().await?;

    let row = sqlx::query!(
        "SELECT
                typ,
                mount,
                mount_end,
                q1.length as ql1,
                q2.Length as ql2,
                q3.length as ql3,
                tfa
                FROM character
                NATURAL JOIN activity
                NATURAL JOIN TAVERN
                    JOIN Quest as q1 on q1.id = tavern.Quest1
                    JOIN Quest as q2 on q2.id = tavern.Quest2
                    JOIN Quest as q3 on q3.id = tavern.Quest3
                WHERE pid = $1",
        session.player_id,
    )
    .fetch_one(&mut *tx)
    .await?;

    if row.typ != 0 {
        return Err(ServerError::StillBusy);
    }

    let mut mount = row.mount;
    let mut mount_end = row.mount_end;
    let mount_effect = effective_mount(&mut mount_end, &mut mount);

    let quest_length = match quest {
        1 => row.ql1,
        2 => row.ql2,
        _ => row.ql3,
    } as f32
        * mount_effect.ceil().max(0.0);

    let quest_length = quest_length as i64;
    let tfa = row.tfa;

    if tfa < quest_length {
        // TODO: Actual error
        return Err(ServerError::StillBusy);
    }
    let busy_until = in_seconds(quest_length);
    sqlx::query!(
        "UPDATE activity
                    SET typ = 2,
                    sub_type = $2,
                    busy_until = $3,
                    started = CURRENT_TIMESTAMP
                WHERE pid = $1",
        session.player_id,
        quest,
        busy_until
    )
    .execute(&mut *tx)
    .await?;

    // TODO: We should keep track of how much we deduct here, so that
    // we can accurately refund this on cancel
    sqlx::query!(
        "UPDATE tavern
                 SET tfa = max(0, tfa - $2)
                 WHERE pid = $1",
        session.player_id,
        quest_length
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    poll(session, "", db, Default::default()).await
}

pub(crate) async fn player_gamble_gold(
    session: Session,
    db: &sqlx::Pool<Sqlite>,
    args: CommandArguments<'_>,
) -> Result<ServerResponse, ServerError> {
    let mut rng = Rng::new();
    let mut silver = args.get_int(0, "gold value")?;

    let mut tx = db.begin().await?;
    let character_silver = sqlx::query_scalar!(
        "SELECT silver FROM character where pid = $1", session.player_id,
    )
    .fetch_one(&mut *tx)
    .await?;

    if silver < 0 || character_silver < silver {
        return Err(ServerError::BadRequest);
    }

    if rng.bool() {
        silver *= 2;
    } else {
        silver = -silver;
    }
    let new_silver = character_silver + silver;

    sqlx::query!(
        "UPDATE character SET silver = $1 WHERE pid = $2", new_silver,
        session.player_id,
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    ResponseBuilder::default()
        .add_key("gamblegoldvalue")
        .add_val(silver)
        .build()
}

pub(crate) async fn player_get_hof(
    session: Session,
    db: &sqlx::Pool<Sqlite>,
    args: CommandArguments<'_>,
) -> Result<ServerResponse, ServerError> {
    let rank = args.get_int(0, "rank").unwrap_or_default();
    let pre = args.get_int(2, "pre").unwrap_or_default();
    let post = args.get_int(3, "post").unwrap_or_default();
    let name = args.get_str(1, "name or rank");

    let rank = match rank {
        1.. => rank,
        _ => {
            let name = name?;
            sqlx::query_scalar!(
                "WITH selected_character AS
                            (SELECT honor, pid
                            FROM character
                            WHERE name = $1 AND world_id = $2)
                        SELECT
                            (SELECT count(*)
                            FROM character
                            WHERE world_id = $2
                                AND honor >
                                (SELECT honor
                                FROM selected_character)
                                OR (honor =
                                    (SELECT honor
                                    FROM selected_character)
                                    AND pid <=
                                    (SELECT pid
                                    FROM selected_character))) AS rank",
                name,
                session.world_id
            )
            .fetch_one(db)
            .await?
        }
    };

    let offset = (rank - pre).max(1) - 1;
    let limit = (pre + post).min(30);

    let res = sqlx::query!(
        "SELECT name, level, honor, class
                     FROM character
                     WHERE world_id = $3
                     ORDER BY honor desc, pid asc
                     LIMIT $2 OFFSET $1",
        offset,
        limit,
        session.world_id,
    )
    .fetch_all(db)
    .await?;

    let mut characters = String::new();
    for (entry_idx, character) in res.into_iter().enumerate() {
        characters
            .write_fmt(format_args!(
                "{},{},{},{},{},{},{};",
                offset + entry_idx as i64 + 1,
                character.name,
                "",
                character.level,
                character.honor,
                character.class,
                "bg"
            ))
            .unwrap();
    }

    ResponseBuilder::default()
        .add_key("Ranklistplayer.r")
        .add_str(&characters)
        .build()
}

pub(crate) async fn player_set_descr(
    session: Session,
    db: &sqlx::Pool<Sqlite>,
    args: CommandArguments<'_>,
) -> Result<ServerResponse, ServerError> {
    let description = args.get_str(0, "description")?;
    let description = from_sf_string(description);
    sqlx::query!(
        "UPDATE character SET description = $1 WHERE pid = $2", description,
        session.player_id
    )
    .execute(db)
    .await?;
    poll(session, "", db, Default::default()).await
}

pub(crate) async fn player_set_face(
    session: Session,
    db: &sqlx::Pool<Sqlite>,
    command_args: CommandArguments<'_>,
) -> Result<ServerResponse, ServerError> {
    let race = command_args.get_int(0, "race")?;
    Race::from_i64(race).ok_or_else(|| ServerError::BadRequest)?;
    let gender = command_args.get_int(1, "gender")?;
    Gender::from_i64(gender.saturating_sub(1))
        .ok_or_else(|| ServerError::BadRequest)?;
    let portrait_str = command_args.get_str(2, "portrait")?;
    let portrait =
        Portrait::parse(portrait_str).ok_or_else(|| ServerError::BadRequest)?;

    let mut tx = db.begin().await?;
    let mushrooms = sqlx::query_scalar!(
        "UPDATE CHARACTER SET gender = $1, race = $2, mushrooms = mushrooms - \
         1 WHERE pid = $3 RETURNING mushrooms",
        gender,
        race,
        session.player_id,
    )
    .fetch_one(&mut *tx)
    .await?;

    if mushrooms < 0 {
        tx.rollback().await?;
        return Err(ServerError::NotEnoughMoney);
    }

    sqlx::query!(
        "UPDATE PORTRAIT SET Mouth = $1, Hair = $2, Brows = $3, Eyes = $4, \
         Beards = $5, Nose = $6, Ears = $7, Horns = $8, extra = $9 WHERE pid \
         = $10",
        portrait.mouth,
        portrait.hair,
        portrait.eyebrows,
        portrait.eyes,
        portrait.beard,
        portrait.nose,
        portrait.ears,
        portrait.horns,
        portrait.extra,
        session.player_id
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(ServerResponse::Success)
}
