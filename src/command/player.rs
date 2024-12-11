use std::{borrow::Borrow, fmt::Write};

use enum_map::{enum_map, EnumMap};
use fastrand::Rng;
use log::error;
use num_traits::FromPrimitive;
use sf_api::{
    command::AttributeType,
    gamestate::{
        character::{Class, Gender, Race},
        items::Potion,
    },
    misc::from_sf_string,
    simulate::{
        AttackType, Battle, BattleEvent, BattleFighter, BattleLogger,
        BattleSide, UpgradeableFighter,
    },
};
use sqlx::Sqlite;

use super::{
    debug::{handle_cheat_command, CheatCmd},
    effective_mount, in_seconds, now, poll, xp_for_next_level,
    CommandArguments, Portrait, ResponseBuilder, ServerError, ServerResponse,
};
use crate::{command::player, request::Session};

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
        .map_err(|e| {
            error!("Error while parsing command: {:?}", e);
            ServerError::BadRequest
        })?;
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
            .map_err(|e| {
                error!("Error while writing format: {:?}", e);
                ServerError::Internal
            })?;
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

pub(crate) async fn player_look_at(
    _session: Session,
    db: &sqlx::Pool<Sqlite>,
    args: CommandArguments<'_>,
) -> Result<ServerResponse, ServerError> {
    let pid = match args.get_int(0, "") {
        Ok(x) => x,
        Err(_) => {
            let name = args.get_str(0, "look at pid or name")?;
            sqlx::query_scalar!(
                "SELECT pid FROM character WHERE name = $1", name
            )
            .fetch_one(db)
            .await?
        }
    };

    let mut resp = ResponseBuilder::default();
    let info = sqlx::query!(
        "
        SELECT name, level, honor, experience, race, portrait.*, gender, class,
            a.*, description
        FROM character c
        NATURAL JOIN portrait
        JOIN attributes a on a.id = c.attributes
        WHERE pid = $1",
        pid
    )
    .fetch_one(db)
    .await?;

    resp.add_key("otherplayergroupname.r");
    resp.add_val("");
    resp.add_key("otherplayer.playerlookat");
    resp.add_val(pid);
    resp.add_val(0);
    resp.add_val(info.level);
    resp.add_val(info.experience); // xp
    resp.add_val(xp_for_next_level(info.level)); // xp next lvl
    resp.add_val(info.honor);
    resp.add_val(10); // TODO: Rank
    resp.add_val(0); // ?
    resp.add_val(info.mouth);
    resp.add_val(info.hair);
    resp.add_val(info.brows);
    resp.add_val(info.eyes);
    resp.add_val(info.beards);
    resp.add_val(info.nose);
    resp.add_val(info.ears);
    resp.add_val(info.extra);
    resp.add_val(info.horns);
    resp.add_val(info.influencer);
    resp.add_val(info.race);
    resp.add_val(info.gender);
    resp.add_val(info.class);
    resp.add_val(info.strength);
    resp.add_val(info.dexterity);
    resp.add_val(info.intelligence);
    resp.add_val(info.stamina);
    resp.add_val(info.luck);
    // TODO: Bonus attrs
    resp.add_val(0);
    resp.add_val(0);
    resp.add_val(0);
    resp.add_val(0);
    resp.add_val(0);

    for _ in 0..8 {
        resp.add_val(0);
    }

    // Equipment
    for _ in 0..10 {
        for _ in 0..12 {
            resp.add_val(0);
        }
    }
    resp.add_val(0); // 159 mount
    resp.add_val(58);
    resp.add_val(37408);
    resp.add_val(2723);
    resp.add_val(11901);
    resp.add_val(0);
    resp.add_val(0);
    resp.add_val(1393194397);
    resp.add_val(1);
    resp.add_val(4165);
    resp.add_val(958);
    resp.add_val(2642);
    resp.add_val(3906638);
    for _ in 0..36 {
        resp.add_val(0);
    }
    // Mainly fortress stuff
    for _ in 0..53 {
        resp.add_val(0);
    }
    resp.add_key("otherdescription.s");
    resp.add_str(&info.description);
    resp.add_key("otherplayername.r");
    resp.add_val(info.name);
    resp.add_key("otherplayerunitlevel(4)");
    resp.add_val(190);
    resp.add_val(140);
    resp.add_val(145);
    resp.add_val(145);
    resp.add_key("otherplayerfriendstatus");
    resp.add_val(0);
    resp.add_key("otherplayerfortressrank");
    resp.add_val(0);
    resp.add_key("otherplayerpetbonus.petbonus");
    resp.add_val(207011);
    resp.add_val(7);
    resp.add_val(6);
    resp.add_val(6);
    resp.add_val(6);
    resp.add_val(6);
    resp.add_key("soldieradvice");
    resp.add_val(18);
    resp.build()
}

pub(crate) async fn player_arena_fight(
    session: Session,
    db: &sqlx::Pool<Sqlite>,
    args: CommandArguments<'_>,
) -> Result<ServerResponse, ServerError> {
    let enemy_name = args.get_str(0, "arena enemy name")?;

    let enemy_id = sqlx::query_scalar!(
        "SELECT pid FROM character WHERE name = $1", enemy_name
    )
    .fetch_one(db)
    .await?;

    let mut resp = ResponseBuilder::default();
    resp.add_key("fightversion");
    resp.add_val(2);

    resp.add_key("fightheader.fighters");
    resp.add_val(0);
    resp.add_val(0);
    resp.add_val(0);
    resp.add_val(0);
    resp.add_val(1);

    let fighters = [session.player_id, enemy_id];

    let mut battle_fighters = Vec::with_capacity(2);

    for pid in fighters {
        let fighter = sqlx::query!(
            "SELECT name, portrait.*, a.*, ab.strength AS strengthb,
                    ab.dexterity AS dexterityb, ab.intelligence AS \
             intelligenceb,
                    ab.stamina AS staminab, ab.luck AS luckb, level, class, \
             race,
                    gender
            FROM character c
            NATURAL JOIN portrait
            JOIN attributes a ON a.id = c.attributes
            JOIN attributes ab ON ab.id = c.attributes_bought
            WHERE pid = $1",
            pid
        )
        .fetch_one(db)
        .await?;

        let attr: EnumMap<AttributeType, u32> = enum_map! {
            AttributeType::Strength => fighter.strength as u32,
            AttributeType::Dexterity => fighter.dexterity as u32,
            AttributeType::Intelligence => fighter.intelligence as u32,
            AttributeType::Constitution => fighter.stamina as u32,
            AttributeType::Luck => fighter.luck as u32,
        };

        let attr_bought: EnumMap<AttributeType, u32> = enum_map! {
            AttributeType::Strength => fighter.strengthb as u32,
            AttributeType::Dexterity => fighter.dexterityb as u32,
            AttributeType::Intelligence => fighter.intelligenceb as u32,
            AttributeType::Constitution => fighter.staminab as u32,
            AttributeType::Luck => fighter.luckb as u32,
        };

        let potions: [Option<Potion>; 3] = Default::default();

        let upgradeable_fighter = UpgradeableFighter {
            is_companion: false,
            level: fighter.level as u16,
            class: Class::from_i64(fighter.class).unwrap(),
            attribute_basis: attr,
            _attributes_bought: attr_bought,
            pet_attribute_bonus_perc: EnumMap::default(), // TODO
            equipment: Default::default(),                // TODO
            active_potions: potions,                      // TODO
            portal_hp_bonus: 0,                           // TODO
            portal_dmg_bonus: 0,                          // TODO
        };

        let bf = BattleFighter::from_upgradeable(&upgradeable_fighter);
        battle_fighters.push(bf.clone());

        // Player info
        resp.add_val(fighter.pid);
        resp.add_str(&fighter.name);
        resp.add_val(fighter.level);
        resp.add_val(bf.max_hp);
        resp.add_val(bf.max_hp);
        resp.add_val(fighter.strength); // str
        resp.add_val(fighter.dexterity); // dex
        resp.add_val(fighter.intelligence); // int
        resp.add_val(fighter.stamina); // const
        resp.add_val(fighter.luck); // luck
        resp.add_val(fighter.mouth); // mouth
        resp.add_val(fighter.hair); // hair
        resp.add_val(fighter.eyes); // brows
        resp.add_val(fighter.eyes); // eyes
        resp.add_val(fighter.beards); // beards
        resp.add_val(fighter.nose); // nose
        resp.add_val(fighter.ears); // ears
        resp.add_val(fighter.extra); // extra
        resp.add_val(fighter.horns); // horns
        resp.add_val(fighter.influencer); // influencer
        resp.add_val(fighter.race);
        resp.add_val(fighter.gender);
        resp.add_val(fighter.class);
        // Dont know, don't care (yet)
        resp.add_val(185204737);
        resp.add_val(327703);
        resp.add_val(494);
        resp.add_val(962);
        resp.add_val(4);
        resp.add_val(1);
        resp.add_val(2);
        resp.add_val(709);
        resp.add_val(0);
        resp.add_val(0);
        resp.add_val(110873491);
        resp.add_val(23396352);
        // Some item i think
        for _ in 0..12 {
            resp.add_val(0);
        }
    }

    resp.add_key("fight.r");

    let mut bf_left = [battle_fighters.get(0).unwrap().clone()];
    let mut bf_right = [battle_fighters.get(1).unwrap().clone()];
    let mut battle: Battle = Battle::new(&mut bf_left, &mut bf_right);
    let mut logger = MyCustomLogger::new(resp, [fighters[0], fighters[1]]);

    battle.simulate(&mut logger);
    resp = logger.response;

    let left_hp = match battle.left.current() {
        Some(f) => f.current_hp,
        None => 0,
    };

    resp.add_key("winnerid");
    resp.add_val(if left_hp > 0 {
        fighters[0]
    } else {
        fighters[1]
    });
    resp.add_key("fightresult.battlereward");
    resp.add_val((left_hp > 0) as i32); // have we won?
    resp.add_val(1);
    resp.add_val(0); // silver
    resp.add_val(0); // xp won
    resp.add_val(if left_hp > 0 { 1337 } else { 0 }); // mushrooms
    resp.add_val(0); // honor won
    resp.add_val(0);
    resp.add_val(2); // rank pre
    resp.add_val(2); // rank post
                     // Item
    for _ in 0..12 {
        resp.add_val(0);
    }
    resp.build()
}

struct MyCustomLogger {
    response: ResponseBuilder,
    fighter_ids: [i64; 2],
    player_turn: i64,
    msg_attack_type: i64,
    msg_enemy_reaction: i64,
}

impl MyCustomLogger {
    fn new(response: ResponseBuilder, fighter_ids: [i64; 2]) -> Self {
        Self {
            response,
            fighter_ids,
            player_turn: -1,
            msg_attack_type: 0,
            msg_enemy_reaction: 0,
        }
    }
}

impl BattleLogger for MyCustomLogger {
    fn log(&mut self, event: BattleEvent) {
        match event {
            BattleEvent::TurnUpdate(b) => {
                if self.player_turn == -1 {
                    self.player_turn = 0;
                    return;
                } else if self.player_turn == 0 {
                    let first = b.started.unwrap();
                    self.player_turn =
                        if first == BattleSide::Left { 2 } else { 1 };
                }
                println!("#### Turn update ####");
                let right_hp = match b.right.current() {
                    Some(f) => f.current_hp,
                    None => 0,
                };
                let left_hp = match (*b).left.current() {
                    Some(f) => f.current_hp,
                    None => 0,
                };
                println!("Left: {:?}, Right: {:?}", left_hp, right_hp);
                self.response.add_val(self.fighter_ids[((self.player_turn + 2) % 2) as usize]);
                self.response.add_val(0);
                self.response.add_val(self.msg_attack_type); // Attack type (normal=0, crit=1, catapult, etc.)
                self.response.add_val(self.msg_enemy_reaction); // Enemy reaction (repelled/dodged)
                self.response.add_val(0);
                // ugly
                if self.player_turn % 2 == 0 {
                    self.response.add_val(left_hp); // Attacker hp
                    self.response.add_val(right_hp); // Defender hp
                } else {
                    self.response.add_val(right_hp); // Attacker hp
                    self.response.add_val(left_hp); // Defender hp
                }
                self.response.add_val(0);
                self.response.add_val(0);
                // and reset for next turn
                self.player_turn += 1;
                self.msg_attack_type = 0;
                self.msg_enemy_reaction = 0;
            }
            BattleEvent::BattleEnd(b, side) => {}
            BattleEvent::Attack(from, to, attack_type) => {
                println!(
                    "Attack (from {:?} to {:?} (Attack-Type: {:?})",
                    from.class, to.class, attack_type as i32
                );
                // nothing to do, 0 is default & we dont wanna override crits
            }
            BattleEvent::Dodged(from, to) => {
                println!("Dodged (from {:?} to {:?})", from.class, to.class);
                self.msg_enemy_reaction = 1;
            }
            BattleEvent::Blocked(from, to) => {
                println!("Blocked (from {:?} to {:?})", from.class, to.class);
                self.msg_enemy_reaction = 2;
            }
            BattleEvent::Crit(from, to) => {
                println!("Crit (from {:?} to {:?})", from.class, to.class);
                self.msg_attack_type = 1;
            }
            BattleEvent::DamageReceived(from, to, dmg) => {
                println!(
                    "Damage received (from {:?} to {:?} (dmg: {:?})",
                    from.class, to.class, dmg
                );
            }
            BattleEvent::DemonHunterRevived(from, to) => {
                println!(
                    "Demon Hunter Revived (from {:?} to {:?})",
                    from.class, to.class
                );
            }
            BattleEvent::CometRepelled(from, to) => {
                println!(
                    "Comet Repelled (from {:?} to {:?})",
                    from.class, to.class
                );
                self.msg_enemy_reaction = 3;
            }
            BattleEvent::CometAttack(from, to) => {
                println!(
                    "Comet Attack (from {:?} to {:?})",
                    from.class, to.class
                );
                self.msg_attack_type = 10;
            }
            BattleEvent::MinionSpawned(from, to, minion) => {
                println!(
                    "Minion Spawned (from {:?} to {:?} (minion: {:?})",
                    from.class, to.class, minion
                );
            }
            BattleEvent::MinionSkeletonRevived(from, to) => {
                println!(
                    "Minion Skeleton Revived (from {:?} to {:?})",
                    from.class, to.class
                );
            }
            BattleEvent::BardPlay(from, to, quality) => {
                println!(
                    "Bard Play (from {:?} to {:?} (quality: {:?})",
                    from.class, to.class, quality
                );
            }
            BattleEvent::FighterDefeat(b, side) => {
                println!("Fighter Defeat (side: {:?})", side);
            }
            _ => {
                // log::error!("Unknown event: {:?}\n\nOccured during battle {:?}\n\nBattle log: {:?}", e, battle, logger.0);
            }
        }
    }
}
