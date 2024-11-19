use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::Query, http::Method, response::Response, routing::get, Router,
};
use base64::Engine;
use libsql::{params, Row, Rows};
use misc::{from_sf_string, to_sf_string};
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use serde::{Deserialize, Serialize};
use sf_api::{
    command::AttributeType,
    gamestate::{
        character::{Class, Gender, Race},
        items::{Enchantment, EquipmentSlot},
    },
};
use strum::EnumCount;
use tower_http::cors::CorsLayer;

use crate::response::*;

#[tokio::main]
async fn main() {
    // initialize tracing
    tracing_subscriber::fmt::init();
    let cors = CorsLayer::new()
        .allow_headers(tower_http::cors::Any)
        .allow_methods([Method::GET, Method::POST])
        .allow_origin(tower_http::cors::Any);

    let app = Router::new()
        .route("/req.php", get(request_wrapper))
        .layer(cors);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:6767").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

pub mod misc;
pub mod response;

const CRYPTO_IV: &str = "jXT#/vz]3]5X7Jl\\";
const DEFAULT_CRYPTO_ID: &str = "0-00000000000000";
const DEFAULT_SESSION_ID: &str = "00000000000000000000000000000000";
const DEFAULT_CRYPTO_KEY: &str = "[_/$VV&*Qg&)r?~g";
const SERVER_VERSION: u32 = 2007;

pub async fn get_db() -> Result<libsql::Connection, ServerError> {
    use async_once_cell::OnceCell;
    static DB: OnceCell<libsql::Connection> = OnceCell::new();

    DB.get_or_try_init(async { connect_init_db().await })
        .await
        .cloned()
}

pub async fn connect_init_db() -> Result<libsql::Connection, ServerError> {
    let db = libsql::Builder::new_local("sf.db")
        .build()
        .await?
        .connect()?;

    // TODO: Query the db to see, if this exists already
    if true {
        db.execute_batch(include_str!("../db.sql")).await?;
    }

    Ok(db)
}

#[derive(Debug)]
pub struct CommandArguments<'a>(Vec<&'a str>);

impl<'a> CommandArguments<'a> {
    pub fn get_int(
        &self,
        pos: usize,
        name: &'static str,
    ) -> Result<i64, ServerError> {
        self.0
            .get(pos)
            .and_then(|a| a.parse().ok())
            .ok_or_else(|| ServerError::MissingArgument(name))
    }

    pub fn get_str(
        &self,
        pos: usize,
        name: &'static str,
    ) -> Result<&str, ServerError> {
        self.0
            .get(pos)
            .copied()
            .ok_or_else(|| ServerError::MissingArgument(name))
    }
}

async fn request_wrapper(
    req: Query<HashMap<String, String>>,
) -> Result<Response, Response> {
    // Rust can infer simple `?` conversions, but the `request()` function
    // exceeds that limit. This is why we manually map this here
    request(req).await.map_err(|a| a.into()).map(|a| a.into())
}

pub trait OptionGet<V> {
    fn get(self, name: &'static str) -> Result<V, ServerError>;
}

impl<T> OptionGet<T> for Option<T> {
    fn get(self, name: &'static str) -> Result<T, ServerError> {
        self.ok_or_else(|| ServerError::MissingArgument(name))
    }
}

async fn request(
    Query(req_params): Query<HashMap<String, String>>,
) -> Result<ServerResponse, ServerError> {
    let request = req_params.get("req").get("request parameter")?;
    let db = get_db().await?;

    if request.len() < DEFAULT_CRYPTO_ID.len() + 5 {
        Err(ServerError::BadRequest)?;
    }

    let (crypto_id, encrypted_request) =
        request.split_at(DEFAULT_CRYPTO_ID.len());

    if encrypted_request.is_empty() {
        Err(ServerError::BadRequest)?;
    }

    let (player_id, crypto_key, server_id) =
        match crypto_id == DEFAULT_CRYPTO_ID {
            true => (-1, DEFAULT_CRYPTO_KEY.to_string(), -1),
            false => {
                let mut res = db
                    .query(
                        "SELECT character.id, cryptokey, character.server
                 FROM character
                 LEFT JOIN Logindata on logindata.id = character.logindata
                 WHERE cryptoid = ?1",
                        [crypto_id],
                    )
                    .await?;

                let row = res.next().await?;

                match row {
                    Some(row) => {
                        let id = row.get(0)?;
                        let cryptokey = row.get_str(1)?;
                        let server_id = row.get(2)?;
                        (id, cryptokey.to_string(), server_id)
                    }
                    None => Err(ServerError::InvalidAuth)?,
                }
            }
        };

    let request = decrypt_server_request(encrypted_request, &crypto_key);

    if request.len() < DEFAULT_SESSION_ID.len() + 5 {
        Err(ServerError::BadRequest)?;
    }

    let (_session_id, request) = request.split_at(DEFAULT_SESSION_ID.len());
    // TODO: Validate session id

    let request = request.trim_matches('|');

    let (command_name, command_args) = request.split_once(':').unwrap();
    if command_name != "Poll" {
        println!("Received: {command_name}: {}", command_args);
    }
    let command_args: Vec<_> = command_args.split('/').collect();
    let args = CommandArguments(command_args);
    let mut rng = fastrand::Rng::new();

    if player_id < 0
        && ![
            "AccountCreate", "AccountLogin", "AccountCheck", "AccountDelete",
        ]
        .contains(&command_name)
    {
        Err(ServerError::InvalidAuth)?;
    }

    match command_name {
        "PlayerSetFace" => player_set_face(&args, &db, player_id).await,
        "AccountCreate" => {
            let name = args.get_str(0, "name")?;
            let password = args.get_str(1, "password")?;
            let mail = args.get_str(2, "mail")?;
            let gender = args.get_int(3, "gender")?;
            let gender =
                Gender::from_i64(gender.saturating_sub(1)).get("gender")?;
            let race = Race::from_i64(args.get_int(4, "race")?).get("race")?;

            let class = args.get_int(5, "class")?;
            let class =
                Class::from_i64(class.saturating_sub(1)).get("class")?;

            let portrait_str = args.get_str(6, "portrait")?;
            let portrait = Portrait::parse(portrait_str).get("portrait")?;

            if is_invalid_name(name) {
                Err(ServerError::InvalidName)?;
            }

            // TODO: Do some more input validation
            let hashed_password = sha1_hash(&format!("{password}{HASH_CONST}"));

            let mut crypto_id = "0-".to_string();
            for _ in 2..DEFAULT_CRYPTO_ID.len() {
                let rc = rng.alphabetic();
                crypto_id.push(rc);
            }

            let crypto_key: String = (0..DEFAULT_CRYPTO_KEY.len())
                .map(|_| rng.alphanumeric())
                .collect();

            let session_id: String = (0..DEFAULT_SESSION_ID.len())
                .map(|_| rng.alphanumeric())
                .collect();

            let tx = db.transaction().await?;

            let res = tx
                .query(
                    "INSERT INTO LOGINDATA (mail, pwhash, SessionID, \
                     CryptoID, CryptoKey) VALUES (?1, ?2, ?3, ?4, ?5) \
                     returning ID",
                    params!(
                        mail, hashed_password, session_id, crypto_id,
                        crypto_key
                    ),
                )
                .await?;

            let login_id = first_int(res).await?;

            let mut quests = [0; 3];
            #[allow(clippy::needless_range_loop)]
            for i in 0..3 {
                let res = tx
                    .query(
                        "INSERT INTO QUEST (monster, location, length, xp, \
                         silver, mushrooms)
                        VALUES (?1, ?2, ?3, ?4, ?5, ?6) returning ID",
                        [139, 1, 60, 100, 100, 1],
                    )
                    .await?;
                quests[i] = first_int(res).await?;
            }

            let res = tx
                .query(
                    "INSERT INTO TAVERN (quest1, quest2, quest3)
                    VALUES (?1, ?2, ?3) returning ID",
                    quests,
                )
                .await?;
            let tavern_id = first_int(res).await?;

            let res = tx
                .query(
                    "INSERT INTO BAG (pos1) VALUES (NULL) returning ID",
                    params!(),
                )
                .await?;
            let bag_id = first_int(res).await?;

            let res = tx
                .query(
                    "INSERT INTO Attributes
                     ( Strength, Dexterity, Intelligence, Stamina, Luck )
                     VALUES (?1, ?2, ?3, ?4, ?5) returning ID",
                    [3, 6, 8, 2, 4],
                )
                .await?;
            let attr_id = first_int(res).await?;

            let res = tx
                .query(
                    "INSERT INTO Attributes
                    ( Strength, Dexterity, Intelligence, Stamina, Luck )
                    VALUES (?1, ?2, ?3, ?4, ?5) returning ID",
                    [0, 0, 0, 0, 0],
                )
                .await?;
            let attr_upgrades = first_int(res).await?;

            let res = tx
                .query(
                    "INSERT INTO PORTRAIT (Mouth, Hair, Brows, Eyes, Beards, \
                     Nose, Ears, Horns, extra) VALUES (?1, ?2, ?3, ?4, ?5, \
                     ?6, ?7, ?8, ?9) returning ID",
                    params!(
                        portrait.mouth, portrait.hair, portrait.eyebrows,
                        portrait.eyes, portrait.beard, portrait.nose,
                        portrait.ears, portrait.horns, portrait.extra
                    ),
                )
                .await?;
            let portrait_id = first_int(res).await?;

            let res = tx
                .query(
                    "INSERT INTO Activity (typ) VALUES (0) RETURNING ID",
                    params!(),
                )
                .await?;
            let activity_id = first_int(res).await?;

            let res = tx
                .query(
                    "INSERT INTO Equipment (hat) VALUES (null) RETURNING ID",
                    params!(),
                )
                .await?;
            let equip_id = first_int(res).await?;

            let mut res = tx
                .query(
                    "INSERT INTO Character
                    (Name, Class, Attributes, AttributesBought, LoginData,
                    Tavern, Bag, Portrait, Gender, Race, Activity, Equipment)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                RETURNING ID",
                    params!(
                        name,
                        class as i32 + 1,
                        attr_id,
                        attr_upgrades,
                        login_id,
                        tavern_id,
                        bag_id,
                        portrait_id,
                        gender as i32 + 1,
                        race as i32,
                        activity_id,
                        equip_id
                    ),
                )
                .await?;

            let r = res.next().await?;
            drop(r);
            drop(res);

            tx.commit().await?;

            ResponseBuilder::default()
                .add_key("tracking.s")
                .add_str("signup")
                .build()
        }
        "AccountLogin" => {
            let name = args.get_str(0, "name")?;
            let full_hash = args.get_str(1, "password hash")?;
            let login_count = args.get_int(2, "login count")?;

            let res = db
                .query(
                    "SELECT character.id, pwhash, character.logindata FROM
                      character LEFT JOIN logindata on logindata.id =
                      character.logindata WHERE lower(name) = lower(?1)",
                    params!(name),
                )
                .await?;
            let info = first_row(res).await?;
            let id = info.get(0)?;
            let pwhash = info.get_str(1)?;
            let logindata: i32 = info.get(2)?;

            let correct_full_hash =
                sha1_hash(&format!("{}{login_count}", pwhash));
            if correct_full_hash != full_hash {
                Err(ServerError::WrongPassword)?;
            }

            let session_id: String = (0..DEFAULT_SESSION_ID.len())
                .map(|_| rng.alphanumeric())
                .collect();

            let mut crypto_id = "0-".to_string();
            for _ in 2..DEFAULT_CRYPTO_ID.len() {
                let rc = rng.alphabetic();
                crypto_id.push(rc);
            }

            db.query(
                "UPDATE logindata
                SET sessionid = ?2, cryptoid = ?3
                WHERE id = ?1",
                params!(logindata, session_id, crypto_id),
            )
            .await?;

            player_poll(id, "accountlogin", &db, Default::default()).await
        }
        "AccountSetLanguage" => {
            // NONE
            Ok(ServerResponse::Success)
        }
        "PlayerSetDescription" => {
            let description = args.get_str(0, "description")?;
            let _description = from_sf_string(description);
            db.query(
                "UPDATE Character SET description = ?1 WHERE id = ?2",
                params!(&description, player_id),
            )
            .await?;
            Ok(player_poll(player_id, "", &db, Default::default()).await?)
        }
        "PlayerHelpshiftAuthtoken" => ResponseBuilder::default()
            .add_key("helpshiftauthtoken")
            .add_val("+eZGNZyCPfOiaufZXr/WpzaaCNHEKMmcT7GRJOGWJAU=")
            .build(),
        "GroupGetHallOfFame" => {
            let rank = args.get_int(0, "rank").unwrap_or_default();
            let pre = args.get_int(2, "pre").unwrap_or_default();
            let post = args.get_int(3, "post").unwrap_or_default();
            let name = args.get_str(1, "name or rank");

            let rank = match rank {
                1.. => rank,
                _ => {
                    // TODO:
                    1
                    // let name = name?;
                    // let res = db
                    //     .query(
                    //         "WITH selected_character AS (
                    //              SELECT honor, id FROM character WHERE name = \
                    //          ?1
                    //         )
                    //          SELECT
                    //              (SELECT COUNT(*) FROM character WHERE honor > \
                    //          (SELECT honor FROM selected_character)
                    //                  OR (honor = (SELECT honor FROM \
                    //          selected_character)
                    //                      AND id <= (SELECT id FROM \
                    //          selected_character))
                    //              ) AS rank",
                    //         [name],
                    //     )
                    //     .await?;
                    // first_int(res).await?
                }
            };

            let offset = (rank - pre).max(1) - 1;
            let limit = (pre + post).min(30);

            let mut res = db
                .query(
                    "SELECT
                        g.name,
                        COALESCE(c.name, 'None') as leader,
                        (SELECT count(*) FROM guildmember WHERE guild = g.id),
                        g.honor,
                        g.attacking
                        FROM GUILD as g
                        LEFT JOIN guildmember as gm on gm.guild = g.id
                        LEFT JOIN character as c on gm.id = c.guild
                        WHERE server = ?3 AND RANK = 3
                        ORDER BY g.honor desc, g.id asc
                        LIMIT ?2 OFFSET ?1",
                    [offset, limit, server_id],
                )
                .await?;

            let mut players = String::new();
            let mut entry_idx = 0;
            while let Some(row) = res.next().await? {
                let player = format!(
                    "{},{},{},{},{},{};",
                    entry_idx,
                    row.get_str(0)?,
                    row.get_str(1)?,
                    row.get::<i32>(2)?,
                    row.get::<i32>(3)?,
                    row.get::<Option<i32>>(4)?.map_or(0, |_| 1),
                );
                players.push_str(&player);
                entry_idx += 1
            }

            ResponseBuilder::default()
                .add_key("ranklistgroup.r")
                .add_str(&players)
                .build()
        }
        "PlayerGetHallOfFame" => {
            let rank = args.get_int(0, "rank").unwrap_or_default();
            let pre = args.get_int(2, "pre").unwrap_or_default();
            let post = args.get_int(3, "post").unwrap_or_default();
            let name = args.get_str(1, "name or rank");

            let rank = match rank {
                1.. => rank,
                _ => {
                    let name = name?;
                    let res = db
                        .query(
                            "WITH selected_character AS
                              (SELECT honor,
                                      id
                               FROM CHARACTER
                               WHERE name = ?1
                                 AND server = ?2)
                            SELECT
                              (SELECT count(*)
                               FROM CHARACTER
                               WHERE server = ?3
                                 AND honor >
                                   (SELECT honor
                                    FROM selected_character)
                                 OR (honor =
                                       (SELECT honor
                                        FROM selected_character)
                                     AND id <=
                                       (SELECT id
                                        FROM selected_character))) AS rank",
                            params!(name, server_id),
                        )
                        .await?;
                    first_int(res).await?
                }
            };

            let offset = (rank - pre).max(1) - 1;
            let limit = (pre + post).min(30);

            let mut res = db
                .query(
                    "SELECT name, level, honor, class
                     FROM CHARACTER
                     WHERE server = ?3
                     ORDER BY honor desc, id asc
                     LIMIT ?2 OFFSET ?1",
                    [offset, limit, server_id],
                )
                .await?;

            let mut players = String::new();
            let mut entry_idx = 0;
            while let Some(row) = res.next().await? {
                let player = format!(
                    "{},{},{},{},{},{},{};",
                    offset + entry_idx + 1,
                    row.get_str(0)?,
                    "",
                    row.get::<i32>(1)?,
                    row.get::<i32>(2)?,
                    row.get::<i32>(3)?,
                    "bg"
                );
                players.push_str(&player);
                entry_idx += 1
            }

            ResponseBuilder::default()
                .add_key("Ranklistplayer.r")
                .add_str(&players)
                .build()
        }
        "AccountDelete" => {
            if true {
                return Ok(ServerResponse::Success);
            }

            let name = args.get_str(0, "account name")?;
            let full_hash = args.get_str(1, "pw hash")?;
            let login_count = args.get_int(2, "login count")?;
            let mail = args.get_str(3, "account mail")?;

            let mut res = db
                .query(
                    "SELECT character.id, pwhash
                    FROM character
                    LEFT JOIN logindata on logindata.id = character.logindata
                    WHERE lower(name) = lower(?1) and mail = ?2",
                    [name, mail],
                )
                .await?;
            let Some(first_line) = res.next().await? else {
                // In case we reset db and char is still in the ui
                return Ok(ServerResponse::Success);
            };

            let id: i32 = first_line.get(0)?;
            let pwhash = first_line.get_str(1)?;
            let correct_full_hash =
                sha1_hash(&format!("{}{login_count}", pwhash));
            if correct_full_hash != full_hash {
                return Err(ServerError::WrongPassword);
            }
            db.query("DELETE FROM character WHERE id = ?1", [id])
                .await?;
            Ok(ServerResponse::Success)
        }
        "PendingRewardView" => {
            let _id = args.get_int(0, "msg_id")?;
            let mut resp = ResponseBuilder::default();
            resp.add_key("pendingrewardressources");
            for v in 1..=6 {
                let val = v;
                resp.add_val(val);
                resp.add_val(999);
            }
            resp.add_key("pendingreward.item(0)");

            resp.build()
        }
        "PlayerGambleGold" => {
            let mut silver = args.get_int(0, "gold value")?;

            let tx = db.transaction().await?;
            let res = tx
                .query(
                    "SELECT silver FROM character where id = ?1",
                    [player_id],
                )
                .await?;
            let player_silver = first_int(res).await?;

            if silver < 0 || player_silver < silver {
                return Err(ServerError::BadRequest);
            }

            if rng.bool() {
                silver *= 2;
            } else {
                silver = -silver;
            }

            let mut res = tx
                .query(
                    "UPDATE character SET silver = ?1 WHERE id = ?2",
                    [player_silver + silver, player_id as i64],
                )
                .await?;
            let row = res.next().await?;
            drop(row);

            tx.commit().await?;

            ResponseBuilder::default()
                .add_key("gamblegoldvalue")
                .add_val(silver)
                .build()
        }
        "PlayerAdventureStart" => {
            let quest = args.get_int(0, "quest")?;
            let skip_inv = args.get_int(1, "skip_inv")?;

            if !(1..=3).contains(&quest) || !(0..=1).contains(&skip_inv) {
                Err(ServerError::BadRequest)?;
            }

            let tx = db.transaction().await?;

            let res = tx
                .query(
                    "SELECT
                typ,
                mount,
                mountend,

                q1.length as ql1,
                q2.Length as ql2,
                q3.length as ql3,

                activity.id as activityid,
                character.tavern as tavern_id,
                tfa

                FROM character LEFT JOIN activity ON activity.id = \
                     character.activity LEFT JOIN TAVERN on character.tavern \
                     = tavern.id LEFT JOIN Quest as q1 on q1.id = \
                     tavern.Quest1 LEFT JOIN Quest as q2 on q2.id = \
                     tavern.Quest2 LEFT JOIN Quest as q3 on q3.id = \
                     tavern.Quest3 WHERE character.id = ?1",
                    [player_id],
                )
                .await?;

            let row = first_row(res).await?;
            let typ: i32 = row.get(0)?;
            if typ != 0 {
                return Err(ServerError::StillBusy);
            }

            let mut mount = row.get(1)?;
            let mut mount_end = row.get(2)?;
            let mount_effect = effective_mount(&mut mount_end, &mut mount);

            let quest_length = match quest {
                1 => row.get::<i32>(3)?,
                2 => row.get::<i32>(4)?,
                _ => row.get::<i32>(5)?,
            } as f32
                * mount_effect.ceil().max(0.0);
            let quest_length = quest_length as i32;

            let activity_id: i32 = row.get(6)?;
            let tavern_id: i32 = row.get(7)?;
            let tfa: i32 = row.get(8)?;

            if tfa < quest_length {
                // TODO: Actual error
                return Err(ServerError::StillBusy);
            }

            drop(row);

            let mut res = tx
                .query(
                    "UPDATE activity SET TYP = 2, SUBTYP = ?2, BUSYUNTIL = \
                     ?3, STARTED = CURRENT_TIMESTAMP WHERE id = ?1",
                    params!(
                        activity_id,
                        quest as i32,
                        in_seconds(quest_length as i64)
                    ),
                )
                .await?;

            let row = res.next().await?;

            drop(row);
            drop(res);

            // TODO: We should keep track of how much we deduct here, so that
            // we can accurately refund this on cancel
            let mut res = tx
                .query(
                    "UPDATE tavern as t
                 SET tfa = max(0, tfa - ?2)
                 WHERE t.id = ?1",
                    params!(tavern_id, quest_length),
                )
                .await?;
            let row = res.next().await?;
            drop(row);
            drop(res);

            tx.commit().await?;

            player_poll(player_id, "", &db, Default::default()).await
        }
        "PlayerAdventureFinished" => {
            let tx = db.transaction().await?;

            let res = tx
                .query(
                    "SELECT
                        activity.typ,
                        activity.busyuntil,
                        activity.subtyp,

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

                    level,--24
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

                    character.race,
                    character.gender,
                    character.class,

                    activity.id as activity_id,

                    experience,
                    portrait.influencer

                     FROM CHARACTER LEFT JOIN PORTRAIT ON character.portrait = \
                     portrait.id LEFT JOIN tavern on tavern.id = \
                     character.tavern LEFT JOIN quest as q1 on tavern.quest1 \
                     = q1.id LEFT JOIN quest as q2 on tavern.quest2 = q2.id \
                     LEFT JOIN quest as q3 on tavern.quest2 = q3.id LEFT JOIN \
                     ACTIVITY ON activity.id = character.activity WHERE \
                     character.id = ?1",
                    [player_id],
                )
                .await?;

            let row = first_row(res).await?;
            let typ: i32 = row.get(0)?;
            if typ != 2 {
                // We are not actually questing
                return Err(ServerError::StillBusy);
            }
            let busyuntil: i64 = row.get(1)?;

            if busyuntil > now() {
                // Quest is still going
                return Err(ServerError::StillBusy);
            }

            let subtyp: i32 = row.get(2)?;

            let base_index = (7 * (subtyp - 1)) + 3;
            let (_item, _length, location, monster, mush, silver, quest_xp) = (
                row.get::<Option<i64>>(base_index)?,
                row.get::<i64>(base_index + 1)?,
                row.get::<i64>(base_index + 2)?,
                row.get::<i64>(base_index + 3)?,
                row.get::<i64>(base_index + 4)?,
                row.get::<i64>(base_index + 5)?,
                row.get::<i32>(base_index + 6)?,
            );

            let honor_won = 10;

            let mut resp = ResponseBuilder::default();

            resp.add_key("fightresult.battlereward");
            resp.add_val(true as u8); // won
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
            let mut player_lvl: i32 = row.get(24)?;
            let starting_player_xp: i32 = row.get(39)?;

            let mut total_xp = quest_xp + starting_player_xp as i32;
            let mut required_xp = xp_for_next_level(player_lvl);
            // Level up the player
            while total_xp > required_xp {
                player_lvl += 1;
                total_xp -= required_xp;
                required_xp = xp_for_next_level(player_lvl);
            }

            let player_attributes = [1, 1, 1, 1, 1];
            let monster_attributes = [1, 1, 1, 1, 1];
            let monster_hp = 10_000;
            let player_hp = 10_000;

            resp.add_val(1);
            resp.add_val(0);
            resp.add_val(0);

            // Location
            resp.add_val(location);

            resp.add_val(1);
            resp.add_val(player_id);
            resp.add_str(row.get_str(25)?);
            resp.add_val(player_lvl);
            for _ in 0..2 {
                resp.add_val(player_hp);
            }
            for val in player_attributes {
                resp.add_val(val);
            }

            // Portrait
            for portrait_offset in 26..26 + 9 {
                resp.add_val(row.get::<i32>(portrait_offset)?);
            }

            resp.add_val(row.get::<i32>(40)?); // special influencer portraits

            resp.add_val(row.get::<i32>(35)?); // race
            resp.add_val(row.get::<i32>(36)?); // gender
            resp.add_val(row.get::<i32>(37)?); // class

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
            resp.add_val(player_lvl); // monster lvl
            resp.add_val(monster_hp);
            resp.add_val(monster_hp);
            for attr in monster_attributes {
                resp.add_val(attr);
            }
            resp.add_val(monster_id);
            for _ in 0..11 {
                resp.add_val(0);
            }
            resp.add_val(3); // Class?

            // Probably also items
            // This means just changing the portrait into the player
            resp.add_val(-1);
            for _ in 0..23 {
                resp.add_val(0);
            }

            resp.add_key("fight.r");
            resp.add_str(&format!("{player_id},0,-1000"));

            // TODO: actually simulate fight

            resp.add_key("winnerid");
            resp.add_val(player_id);

            resp.add_key("fightversion");
            resp.add_val(1);

            let activity_id = row.get::<i32>(38)?;

            drop(row);
            let mut res = tx
                .query(
                    "UPDATE activity as a
                 SET typ = 0, subtyp = 0, started = 0, busyuntil = 0
                 WHERE a.id = ?1",
                    [activity_id],
                )
                .await?;
            let row = res.next().await?;
            drop(row);
            drop(res);

            let mut res = tx
                .query(
                    "UPDATE character as c
                    SET silver = silver + ?2, mushrooms = mushrooms + ?3, \
                     honor = honor + ?4, level = ?5, Experience = ?6
                 WHERE c.id = ?1",
                    params!(
                        player_id, silver, mush, honor_won, player_lvl,
                        total_xp
                    ),
                )
                .await?;
            let row = res.next().await?;
            drop(row);
            drop(res);

            // TODO: Reroll quests, add item & save fight somewhere for rewatch (save)

            tx.commit().await?;

            player_poll(player_id, "", &db, resp).await
        }
        "PlayerMountBuy" => {
            todo!();
            // let mount = command_args.get_int(0, "mount")?;
            // let mount = mount as i32;

            // let Ok(mut tx) = db.begin().await else {
            //     return INTERNAL_ERR;
            // };

            // let Ok(player) = sqlx::query!(
            //     "SELECT silver, mushrooms, mount, mountend FROM CHARACTER \
            //      WHERE id = ?1",
            //     player_id
            // )
            // .fetch_one(&mut *tx)
            // .await
            // else {
            //     _ = tx.rollback().await;
            //     return INTERNAL_ERR;
            // };

            // let mut silver = player.silver;
            // let mut mushrooms = player.mushrooms;

            // let price = match mount {
            //     0 => 0,
            //     1 => 100,
            //     2 => 500,
            //     3 => 0,
            //     4 => 0, // TODO: Reward
            //     _ => {
            //         return ServerError::BadRequest.resp();
            //     }
            // };

            // let mush_price = match mount {
            //     3 => 1,
            //     4 => 25,
            //     _ => 0,
            // };
            // if mushrooms < mush_price {
            //     return ServerError::NotEnoughMoney.resp();
            // }
            // mushrooms -= mush_price;

            // if silver < price {
            //     return ServerError::NotEnoughMoney.resp();
            // }
            // silver -= price;

            // let now = Local::now().naive_local();
            // let mount_start = match player.mountend {
            //     Some(x) if player.mount == mount => now.max(x),
            //     _ => now,
            // };

            // if sqlx::query!(
            //     "UPDATE Character SET mount = ?1, mountend = ?2, mushrooms = \
            //      ?4, silver = ?5 WHERE id = ?3",
            //     mount,
            //     mount_start + Duration::from_secs(60 * 60 * 24 * 14),
            //     player_id,
            //     mushrooms,
            //     silver,
            // )
            // .execute(&mut *tx)
            // .await
            // .is_err()
            // {
            //     _ = tx.rollback().await;
            //     return INTERNAL_ERR;
            // };

            // match tx.commit().await {
            //     Err(_) => INTERNAL_ERR,
            //     Ok(_) => {
            //         player_poll(player_id, "", &db, Default::default()).await
            //     }
            // }
        }
        "PlayerTutorialStatus" => {
            let status = args.get_int(0, "tutorial status")?;
            if !(0..=0xFFFFFFF).contains(&status) {
                Err(ServerError::BadRequest)?;
            }
            db.query(
                "UPDATE CHARACTER SET tutorialstatus = ?1 WHERE ID = ?2",
                [status as i32, player_id],
            )
            .await?;
            Ok(ServerResponse::Success)
        }
        "Poll" => {
            Ok(player_poll(player_id, "poll", &db, Default::default()).await?)
        }
        "UserSettingsUpdate" => Ok(ServerResponse::Success),
        "PlayerWhisper" => {
            let name = args.get_str(0, "name")?.to_lowercase();
            if name != "server" {
                todo!()
            }
            let res =
                CheatCmd::try_parse_from(args.get_str(1, "args")?.split(' '))
                    .map_err(|_| ServerError::BadRequest)?;

            match res.command {
                Command::Level { level } => {
                    if level < 1 {
                        return Err(ServerError::BadRequest);
                    }
                    db.query(
                        "UPDATE character set level = ?1, experience = 0 \
                         WHERE id = ?2",
                        params!(level, player_id),
                    )
                    .await?;
                }
                Command::Class { class } => {
                    let class =
                        Class::from_i16(class - 1).get("command class")?;
                    db.query(
                        "UPDATE character set class = ?1 WHERE id = ?2",
                        params!(class as i32 + 1, player_id),
                    )
                    .await?;
                }
                Command::SetPassword { new } => {
                    let hashed_password =
                        sha1_hash(&format!("{new}{HASH_CONST}"));
                    db.query(
                        "UPDATE LOGINDATA as l
                        SET pwhash = ?1
                        WHERE l.id = (
                            SELECT character.logindata
                            FROM character
                            WHERE id = ?2
                        )",
                        params!(hashed_password, player_id),
                    )
                    .await?;
                }
            }
            Ok(player_poll(player_id, "", &db, Default::default()).await?)
        }
        "AccountCheck" => {
            let name = args.get_str(0, "name")?;

            if is_invalid_name(name) {
                return Err(ServerError::InvalidName)?;
            }

            let res = db
                .query("SELECT COUNT(*) FROM CHARACTER WHERE name = ?1", [name])
                .await?;
            let count = first_int(res).await?;

            match count {
                0 => ResponseBuilder::default()
                    .add_key("serverversion")
                    .add_val(SERVER_VERSION)
                    .add_key("preregister")
                    .add_val(0)
                    .add_val(0)
                    .build(),
                _ => Err(ServerError::CharacterExists),
            }
        }
        _ => {
            println!("Unknown command: {command_name} - {:?}", args);
            Err(ServerError::UnknownRequest)?
        }
    }
}

async fn player_set_face(
    command_args: &CommandArguments<'_>,
    db: &libsql::Connection,
    player_id: i32,
) -> Result<ServerResponse, ServerError> {
    let race = Race::from_i64(command_args.get_int(0, "race")?)
        .ok_or_else(|| ServerError::BadRequest)?;
    let gender = command_args.get_int(1, "gender")?;
    let gender = Gender::from_i64(gender.saturating_sub(1))
        .ok_or_else(|| ServerError::BadRequest)?;
    let portrait_str = command_args.get_str(2, "portrait")?;
    let portrait =
        Portrait::parse(portrait_str).ok_or_else(|| ServerError::BadRequest)?;

    let tx = db.transaction().await?;

    let res = tx
        .query(
            "UPDATE CHARACTER SET gender = ?1, race = ?2, mushrooms = \
             mushrooms - 1 WHERE id = ?3 RETURNING portrait, mushrooms",
            params!(gender as i32 + 1, race as i32, player_id),
        )
        .await?;
    let row = first_row(res).await?;

    let portrait_id: i32 = row.get(0)?;
    let mushrooms: i32 = row.get(1)?;
    if mushrooms < 0 {
        tx.rollback().await?;
        return Err(ServerError::NotEnoughMoney);
    }

    let mut res = db
        .query(
            "UPDATE PORTRAIT SET Mouth = ?1, Hair = ?2, Brows = ?3, Eyes = \
             ?4, Beards = ?5, Nose = ?6, Ears = ?7, Horns = ?8, extra = ?9 \
             WHERE ID = ?10",
            params!(
                portrait.mouth, portrait.hair, portrait.eyebrows,
                portrait.eyes, portrait.beard, portrait.nose, portrait.ears,
                portrait.horns, portrait.extra, portrait_id
            ),
        )
        .await?;

    drop(row);
    let row = res.next().await?;
    drop(res);
    drop(row);

    tx.commit().await?;

    Ok(ServerResponse::Success)
}

pub(crate) const HASH_CONST: &str = "ahHoj2woo1eeChiech6ohphoB7Aithoh";

pub(crate) fn sha1_hash(val: &str) -> String {
    use sha1::{Digest, Sha1};
    let mut hasher = Sha1::new();
    hasher.update(val.as_bytes());
    let hash = hasher.finalize();
    let mut result = String::with_capacity(hash.len() * 2);
    for byte in hash.iter() {
        result.push_str(&format!("{byte:02x}"));
    }
    result
}

pub struct Portrait {
    mouth: i32,
    hair: i32,
    eyebrows: i32,
    eyes: i32,
    beard: i32,
    nose: i32,
    ears: i32,
    extra: i32,
    horns: i32,
}

#[derive(Debug, FromPrimitive, Clone, Copy, Serialize, Deserialize)]
pub enum RawItemTyp {
    Weapon = 1,
    Shield,
    BreastPlate,
    FootWear,
    Gloves,
    Hat,
    Belt,
    Amulet,
    Ring,
    Talisman,
    UniqueItem,
    Useable,
    Scrapbook,
    Gem = 15,
    PetItem,
    QuickSandGlassOrGral,
    HeartOfDarkness,
    WheelOfFortune,
    Mannequin,
}

#[derive(Debug, FromPrimitive, Clone, Copy, Serialize, Deserialize)]
pub enum SubItemTyp {
    DungeonKey1 = 1,
    DungeonKey2 = 2,
    DungeonKey3 = 3,
    DungeonKey4 = 4,
    DungeonKey5 = 5,
    DungeonKey6 = 6,
    DungeonKey7 = 7,
    DungeonKey8 = 8,
    DungeonKey9 = 9,
    DungeonKey10 = 10,
    DungeonKey11 = 11,
    DungeonKey17 = 17,
    DungeonKey19 = 19,
    DungeonKey22 = 22,
    DungeonKey69 = 69,
    DungeonKey70 = 70,
    ToiletKey = 20,
    ShadowDungeonKey51 = 51,
    ShadowDungeonKey52 = 52,
    ShadowDungeonKey53 = 53,
    ShadowDungeonKey54 = 54,
    ShadowDungeonKey55 = 55,
    ShadowDungeonKey56 = 56,
    ShadowDungeonKey57 = 57,
    ShadowDungeonKey58 = 58,
    ShadowDungeonKey59 = 59,
    ShadowDungeonKey60 = 60,
    ShadowDungeonKey61 = 61,
    ShadowDungeonKey62 = 62,
    ShadowDungeonKey63 = 63,
    ShadowDungeonKey64 = 64,
    ShadowDungeonKey67 = 67,
    ShadowDungeonKey68 = 68,
    EpicItemBag = 10000,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum GemValue {
    Legendary = 4,
    Strength1 = 10,
    Strength2 = 20,
    Strength3 = 30,
    Dexterity1 = 11,
    Dexterity2 = 21,
    Dexterity3 = 31,
    Intelligence1 = 12,
    Intelligence2 = 22,
    Intelligence3 = 32,
    Constitution1 = 13,
    Constitution2 = 23,
    Constitution3 = 33,
    Luck1 = 14,
    Luck2 = 24,
    Luck3 = 34,
    All1 = 15,
    All2 = 25,
    All3 = 35,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AtrTuple {
    atr_typ: AtrTyp,
    atr_val: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum AtrEffect {
    Simple([Option<AtrTuple>; 3]),
    Amount(i64),
    Expires(i64),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum AtrTyp {
    Strength = 1,
    Dexterity = 2,
    Intelligence = 3,
    Constitution = 4,
    Luck = 5,
    All = 6,
    StrengthConstitutionLuck = 21,
    DexterityConstitutionLuck = 22,
    IntelligenceConstitutionLuck = 23,
    QuestGold = 31,
    EpicChance,
    ItemQuality,
    QuestXP,
    ExtraHitPoints,
    FireResistance,
    ColdResistence,
    LightningResistance,
    TotalResistence,
    FireDamage,
    ColdDamage,
    LightningDamage,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum MainClass {
    Warrior = 0,
    Mage = 1,
    Scout = 2,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RawItem {
    item_typ: RawItemTyp,
    enchantment: Option<Enchantment>,
    gem_val: i64,

    sub_ident: Option<SubItemTyp>,
    class: Option<MainClass>,
    modelid: i32,

    effect_1: i32,
    effect_2: i32,

    atrs: AtrEffect,

    silver: i32,
    mushrooms: i32,
    gem_pwr: i32,
}

impl RawItem {
    pub fn serialize_response(&self, resp: &mut ResponseBuilder) {
        let mut ident = self.item_typ as i64;
        ident |= self.enchantment.map(|a| a as i64).unwrap_or_default() << 24;
        ident |= self.gem_val << 16;
        resp.add_val(ident);

        let mut sub_ident =
            self.sub_ident.map(|a| a as i64).unwrap_or_default();
        sub_ident |= self.class.map(|a| a as i64 * 1000).unwrap_or_default();
        sub_ident |= self.modelid as i64;
        resp.add_val(sub_ident);

        resp.add_val(self.effect_1 as i64);
        resp.add_val(self.effect_2 as i64);

        match &self.atrs {
            AtrEffect::Simple(atrs) => {
                for atr in atrs {
                    match atr {
                        Some(x) => resp.add_val(x.atr_typ as i64),
                        None => resp.add_val(0),
                    };
                }
                for atr in atrs {
                    match atr {
                        Some(x) => resp.add_val(x.atr_val),
                        None => resp.add_val(0),
                    };
                }
            }
            AtrEffect::Amount(amount) => {
                for _ in 0..3 {
                    resp.add_val(0);
                }
                resp.add_val(amount);
                for _ in 0..2 {
                    resp.add_val(0);
                }
            }
            AtrEffect::Expires(expires) => {
                resp.add_val(expires);
                for _ in 0..5 {
                    resp.add_val(0);
                }
            }
        }

        resp.add_val(self.silver as i64);
        resp.add_val(self.mushrooms as i64 | (self.gem_pwr as i64) << 16);
    }
}

impl Portrait {
    pub fn parse(portrait: &str) -> Option<Portrait> {
        let mut portrait_vals: Vec<i32> = Vec::new();
        for v in portrait.split(',') {
            let Ok(opt) = v.parse() else {
                return None;
            };
            portrait_vals.push(opt);
        }

        if portrait_vals.len() != 9 {
            return None;
        }

        Some(Portrait {
            mouth: portrait_vals[0],
            hair: portrait_vals[1],
            eyebrows: portrait_vals[2],
            eyes: portrait_vals[3],
            beard: portrait_vals[4],
            nose: portrait_vals[5],
            ears: portrait_vals[6],
            extra: portrait_vals[7],
            horns: portrait_vals[8],
        })
    }
}

async fn player_poll(
    pid: i32,
    tracking: &str,
    db: &libsql::Connection,
    mut builder: ResponseBuilder,
) -> Result<ServerResponse, ServerError> {
    let resp = builder
        .add_key("serverversion")
        .add_val(SERVER_VERSION)
        .add_key("preregister")
        .add_val(0) // TODO: This has values
        .add_val(0)
        .skip_key();

    let res = db
        .query(
            "SELECT

        character.id, --0
        logindata.sessionid,
        character.level,
        character.experience,
        character.honor,

        portrait.mouth,
        portrait.Hair,
        portrait.Brows,
        portrait.Eyes,
        portrait.Beards,
        portrait.Nose, --10
        portrait.Ears,
        portrait.Extra,
        portrait.Horns,

        character.race,
        character.gender,
        character.class,

        activity.typ as activitytyp,
        activity.subtyp as activitysubtyp,
        activity.busyuntil,

        q1.Flavour1 as q1f1, -- 20
        q3.Flavour1 as q3f1,
        q2.Flavour1 as q2f1,

        q1.Flavour2 as q1f2,
        q2.Flavour2 as q2f2,
        q3.Flavour2 as q3f2,

        q1.Monster as q1monster,
        q2.Monster as q2monster,
        q3.Monster as q3monster,

        q1.Location as q1location,
        q2.Location as q2location, -- 30
        q3.Location as q3location,

        character.mountend,
        character.mount,

        q1.length as q1length,
        q2.length as q2length,
        q3.length as q3length,

        q1.XP as q1xp,
        q3.XP as q3xp,
        q2.XP as q2xp,

        q1.Silver as q1silver, --40
        q2.SILVER as q2silver,
        q3.SILVER as q3silver,

        tavern.tfa,
        tavern.BeerDrunk,

        TutorialStatus,

        tavern.DiceGameNextFree,
        tavern.DiceGamesRemaining,

        character.mushrooms,
        character.silver,
        tavern.QuickSand, -- 50

        description,
        character.name,

        logindata.cryptoid,
        logindata.cryptokey,
        logindata.logincount,
        portrait.influencer,

        (
        SELECT count(*)
        FROM CHARACTER AS x
        WHERE x.server = character.server
          AND (x.honor > character.honor
               OR (x.honor = character.honor
                   AND x.id <= character.id))
        ),
        (
        SELECT count(*)
        FROM CHARACTER AS x
        WHERE x.server = character.server
        )

        FROM CHARACTER LEFT JOIN logindata on logindata.id = \
             character.logindata LEFT JOIN activity on activity.id = \
             character.activity LEFT JOIN portrait on portrait.id = \
             character.portrait LEFT JOIN tavern on tavern.id = \
             character.tavern LEFT JOIN quest as q1 on tavern.quest1 = q1.id \
             LEFT JOIN quest as q2 on tavern.quest2 = q2.id LEFT JOIN quest \
             as q3 on tavern.quest2 = q3.id WHERE character.id = ?1",
            &[pid],
        )
        .await?;

    let res = first_row(res).await?;

    let calendar_info = "12/1/8/1/3/1/25/1/5/1/2/1/3/2/1/1/24/1/18/5/6/1/22/1/\
                         7/1/6/2/8/2/22/2/5/2/2/2/3/3/21/1";

    resp.add_key("messagelist.r");
    resp.add_str(";");

    resp.add_key("combatloglist.s");
    resp.add_str(";");

    resp.add_key("friendlist.r");
    resp.add_str(";");

    resp.add_key("login count");
    resp.add_val(res.get::<i64>(0)?);

    resp.skip_key();

    resp.add_key("sessionid");
    resp.add_str(res.get_str(1)?);

    resp.add_key("languagecodelist");
    resp.add_str(
        "ru,20;fi,8;ar,1;tr,23;nl,16;  \
         ,0;ja,14;it,13;sk,21;fr,9;ko,15;pl,17;cs,2;el,5;da,3;en,6;hr,10;de,4;\
         zh,24;sv,22;hu,11;pt,12;es,7;pt-br,18;ro,19;",
    );

    resp.add_key("languagecodelist.r");

    resp.add_key("maxpetlevel");
    resp.add_val(100);

    resp.add_key("calenderinfo");
    resp.add_val(calendar_info);

    resp.skip_key();

    resp.add_key("tavernspecial");
    resp.add_val(0);

    resp.add_key("tavernspecialsub");
    resp.add_val(0);

    resp.add_key("tavernspecialend");
    resp.add_val(-1);

    resp.add_key("attbonus1(3)");
    resp.add_str("0/0/0/0");
    resp.add_key("attbonus2(3)");
    resp.add_str("0/0/0/0");
    resp.add_key("attbonus3(3)");
    resp.add_str("0/0/0/0");
    resp.add_key("attbonus4(3)");
    resp.add_str("0/0/0/0");
    resp.add_key("attbonus5(3)");
    resp.add_str("0/0/0/0");

    resp.add_key("stoneperhournextlevel");
    resp.add_val(50);

    resp.add_key("woodperhournextlevel");
    resp.add_val(150);

    resp.add_key("fortresswalllevel");
    resp.add_val(5);

    resp.add_key("inboxcapacity");
    resp.add_val(100);

    resp.add_key("ownplayersave.playerSave");
    resp.add_val(403127023); // What is this?
    resp.add_val(pid);
    resp.add_val(0);
    resp.add_val(1708336503);
    resp.add_val(1292388336);
    resp.add_val(0);
    resp.add_val(0);
    let level = res.get::<i32>(2)?;
    resp.add_val(level); // Level | Arena << 16
    resp.add_val(res.get::<i64>(3)?); // Experience
    resp.add_val(xp_for_next_level(level)); // Next Level XP
    let honor: i32 = res.get(4)?;
    resp.add_val(honor); // Honor

    let rank = res.get::<i64>(57)?;
    resp.add_val(rank); // Rank

    resp.add_val(0); // 12?
    resp.add_val(10); // 13?
    resp.add_val(0); // 14?
    resp.add_val(15); // 15?
    resp.add_val(0); // 16?

    // Portrait start
    resp.add_val(res.get::<i64>(5)?); // mouth
    resp.add_val(res.get::<i64>(6)?); // hair
    resp.add_val(res.get::<i64>(7)?); // brows
    resp.add_val(res.get::<i64>(8)?); // eyes
    resp.add_val(res.get::<i64>(9)?); // beards
    resp.add_val(res.get::<i64>(10)?); // nose
    resp.add_val(res.get::<i64>(11)?); // ears
    resp.add_val(res.get::<i64>(12)?); // extra
    resp.add_val(res.get::<i64>(13)?); // horns
    resp.add_val(res.get::<i64>(56)?); // influencer
    resp.add_val(res.get::<i64>(14)?); // race
    resp.add_val(res.get::<i64>(15)?); // Gender & Mirror
    resp.add_val(res.get::<i64>(16)?); // class

    // Attributes
    for _ in 0..AttributeType::COUNT {
        resp.add_val(100); // 30..=34
    }

    // attribute_additions (aggregate from equipment)
    for _ in 0..AttributeType::COUNT {
        resp.add_val(0); // 35..=38
    }

    // attribute_times_bought
    for _ in 0..AttributeType::COUNT {
        resp.add_val(0); // 40..=44
    }

    resp.add_val(res.get::<i64>(17)?); // Current action
    resp.add_val(res.get::<i64>(18)?); // Secondary (time busy)
    resp.add_val(res.get::<i64>(19)?); // Busy until

    // Equipment
    for slot in [
        EquipmentSlot::Hat,
        EquipmentSlot::BreastPlate,
        EquipmentSlot::Gloves,
        EquipmentSlot::FootWear,
        EquipmentSlot::Amulet,
        EquipmentSlot::Belt,
        EquipmentSlot::Ring,
        EquipmentSlot::Talisman,
        EquipmentSlot::Weapon,
        EquipmentSlot::Shield,
    ] {
        resp.add_dyn_item(format!("{slot:?}").to_lowercase());
    }
    resp.add_dyn_item("inventory1");
    resp.add_dyn_item("inventory2");
    resp.add_dyn_item("inventory3");
    resp.add_dyn_item("inventory4");
    resp.add_dyn_item("inventory5");

    resp.add_val(in_seconds(60 * 60)); // 228

    // Ok, so Flavour 1, Flavour 2 & Monster ID decide =>
    // - The Line they say
    // - the quest name
    // - the quest giver
    resp.add_val(res.get::<i64>(20)?); // 229 Quest1 Flavour1
    resp.add_val(res.get::<i64>(21)?); // 230 Quest2 Flavour1
    resp.add_val(res.get::<i64>(22)?); // 231 Quest3 Flavour1

    resp.add_val(res.get::<i64>(23)?); // 233 Quest2 Flavour2
    resp.add_val(res.get::<i64>(24)?); // 232 Quest1 Flavour2
    resp.add_val(res.get::<i64>(25)?); // 234 Quest3 Flavour2

    resp.add_val(-res.get::<i64>(26)?); // 235 quest 1 monster
    resp.add_val(-res.get::<i64>(27)?); // 236 quest 2 monster
    resp.add_val(-res.get::<i64>(28)?); // 237 quest 3 monster

    resp.add_val(res.get::<i64>(29)?); // 238 quest 1 location
    resp.add_val(res.get::<i64>(30)?); // 239 quest 2 location
    resp.add_val(res.get::<i64>(31)?); // 240 quest 3 location

    let mut mount_end = res.get::<i64>(32)?;
    let mut mount: i32 = res.get(33)?;

    let mount_effect = effective_mount(&mut mount_end, &mut mount);

    resp.add_val((res.get::<i64>(34)? as f32 * mount_effect) as i32); // 241 quest 1 length
    resp.add_val((res.get::<i64>(35)? as f32 * mount_effect) as i32); // 242 quest 2 length
    resp.add_val((res.get::<i64>(36)? as f32 * mount_effect) as i32); // 243 quest 3 length

    // Quest 1..=3 items
    for _ in 0..3 {
        for _ in 0..12 {
            resp.add_val(0); // 244..=279
        }
    }

    resp.add_val(res.get::<i64>(37)?); // 280 quest 1 xp
    resp.add_val(res.get::<i64>(38)?); // 281 quest 2 xp
    resp.add_val(res.get::<i64>(39)?); // 282 quest 3 xp

    resp.add_val(res.get::<i64>(40)?); // 283 quest 1 silver
    resp.add_val(res.get::<i64>(41)?); // 284 quest 2 silver
    resp.add_val(res.get::<i64>(42)?); // 285 quest 3 silver

    resp.add_val(mount); // Mount?

    // Weapon shop
    resp.add_val(1708336503); // 287
    for _ in 0..6 {
        resp.add_dyn_item("weapon");
    }

    // Magic shop
    resp.add_val(1708336503); // 360
    for _ in 0..6 {
        resp.add_dyn_item("weapon");
    }

    resp.add_val(0); // 433
    resp.add_val(1); // 434 might be tutorial related?
    resp.add_val(0); // 435
    resp.add_val(0); // 436
    resp.add_val(0); // 437

    resp.add_val(0); // 438 scrapbook count
    resp.add_val(0); // 439
    resp.add_val(0); // 440
    resp.add_val(0); // 441
    resp.add_val(0); // 442

    resp.add_val(0); // 443 guild join date
    resp.add_val(0); // 444
    resp.add_val(0); // 445 player_hp_bonus << 24, damage_bonus << 16
    resp.add_val(0); // 446
    resp.add_val(0); // 447  Armor
    resp.add_val(6); // 448  Min damage
    resp.add_val(12); // 449 Max damage
    resp.add_val(112); // 450
    resp.add_val(mount_end); // 451 Mount end
    resp.add_val(0); // 452
    resp.add_val(0); // 453
    resp.add_val(0); // 454
    resp.add_val(1708336503); // 455
    resp.add_val(res.get::<i64>(43)?); // 456 Alu secs
    resp.add_val(res.get::<i64>(44)?); // 457 Beer drunk
    resp.add_val(0); // 458
    resp.add_val(0); // 459 dungeon_timer
    resp.add_val(1708336503); // 460 Next free fight
    resp.add_val(0); // 461
    resp.add_val(0); // 462
    resp.add_val(0); // 463
    resp.add_val(0); // 464
    resp.add_val(408); // 465
    resp.add_val(0); // 466
    resp.add_val(0); // 467
    resp.add_val(0); // 468
    resp.add_val(0); // 469
    resp.add_val(0); // 470
    resp.add_val(0); // 471
    resp.add_val(0); // 472
    resp.add_val(0); // 473
    resp.add_val(-111); // 474
    resp.add_val(0); // 475
    resp.add_val(0); // 476
    resp.add_val(4); // 477
    resp.add_val(1708336504); // 478
    resp.add_val(0); // 479
    resp.add_val(0); // 480
    resp.add_val(0); // 481
    resp.add_val(0); // 482
    resp.add_val(0); // 483
    resp.add_val(0); // 484
    resp.add_val(0); // 485
    resp.add_val(0); // 486
    resp.add_val(0); // 487
    resp.add_val(0); // 488
    resp.add_val(0); // 489
    resp.add_val(0); // 490

    resp.add_val(0); // 491 aura_level (0 == locked)
    resp.add_val(0); // 492 aura_now

    // Active potions
    for _ in 0..3 {
        resp.add_val(0); // typ & size
    }
    for _ in 0..3 {
        resp.add_val(0); // ??
    }
    for _ in 0..3 {
        resp.add_val(0); // expires
    }
    resp.add_val(0); // 502
    resp.add_val(0); // 503
    resp.add_val(0); // 504
    resp.add_val(0); // 505
    resp.add_val(0); // 506
    resp.add_val(0); // 507
    resp.add_val(0); // 508
    resp.add_val(0); // 509
    resp.add_val(0); // 510
    resp.add_val(6); // 511
    resp.add_val(2); // 512
    resp.add_val(0); // 513
    resp.add_val(0); // 514
    resp.add_val(100); // 515 aura_missing
    resp.add_val(0); // 516
    resp.add_val(0); // 517
    resp.add_val(0); // 518
    resp.add_val(100); // 519
    resp.add_val(0); // 520
    resp.add_val(0); // 521
    resp.add_val(0); // 522
    resp.add_val(0); // 523

    // Fortress
    // Building levels
    resp.add_val(0); // 524
    resp.add_val(0); // 525
    resp.add_val(0); // 526
    resp.add_val(0); // 527
    resp.add_val(0); // 528
    resp.add_val(0); // 529
    resp.add_val(0); // 530
    resp.add_val(0); // 531
    resp.add_val(0); // 532
    resp.add_val(0); // 533
    resp.add_val(0); // 534
    resp.add_val(0); // 535
    resp.add_val(0); // 536
    resp.add_val(0); // 537
    resp.add_val(0); // 538
    resp.add_val(0); // 539
    resp.add_val(0); // 540
    resp.add_val(0); // 541
    resp.add_val(0); // 542
    resp.add_val(0); // 543
    resp.add_val(0); // 544
    resp.add_val(0); // 545
    resp.add_val(0); // 546
                     // unit counts
    resp.add_val(0); // 547
    resp.add_val(0); // 548
    resp.add_val(0); // 549
                     // upgrade_began
    resp.add_val(0); // 550
    resp.add_val(0); // 551
    resp.add_val(0); // 552
                     // upgrade_finish
    resp.add_val(0); // 553
    resp.add_val(0); // 554
    resp.add_val(0); // 555

    resp.add_val(0); // 556
    resp.add_val(0); // 557
    resp.add_val(0); // 558
    resp.add_val(0); // 559
    resp.add_val(0); // 560
    resp.add_val(0); // 561

    // Current resource in store
    resp.add_val(0); // 562
    resp.add_val(0); // 563
    resp.add_val(0); // 564
                     // max_in_building
    resp.add_val(0); // 565
    resp.add_val(0); // 566
    resp.add_val(0); // 567
                     // max saved
    resp.add_val(0); // 568
    resp.add_val(0); // 569
    resp.add_val(0); // 570

    resp.add_val(0); // 571 building_upgraded
    resp.add_val(0); // 572 building_upgrade_finish
    resp.add_val(0); // 573 building_upgrade_began
                     // per hour
    resp.add_val(0); // 574
    resp.add_val(0); // 575
    resp.add_val(0); // 576
    resp.add_val(0); // 577 unknown time stamp
    resp.add_val(0); // 578

    resp.add_val(0); // 579 wheel_spins_today
    resp.add_val(now() + 60 * 10); // 580  wheel_next_free_spin

    resp.add_val(0); // 581 ft level
    resp.add_val(100); // 582 ft honor
    resp.add_val(0); // 583 rank
    resp.add_val(900); // 584
    resp.add_val(300); // 585
    resp.add_val(0); // 586

    resp.add_val(0); // 587 attack target
    resp.add_val(0); // 588 attack_free_reroll
    resp.add_val(0); // 589
    resp.add_val(0); // 590
    resp.add_val(0); // 591
    resp.add_val(0); // 592
    resp.add_val(3); // 593

    resp.add_val(0); // 594 gem_stone_target
    resp.add_val(0); // 595 gem_search_finish
    resp.add_val(0); // 596 gem_search_began
    resp.add_val(res.get::<i64>(45)?); // 597 Pretty sure this is a bit map of which messages have been seen
    resp.add_val(0); // 598

    // Arena enemies
    resp.add_val(0); // 599
    resp.add_val(0); // 600
    resp.add_val(0); // 601

    resp.add_val(0); // 602
    resp.add_val(0); // 603
    resp.add_val(0); // 604
    resp.add_val(0); // 605
    resp.add_val(0); // 606
    resp.add_val(0); // 607
    resp.add_val(0); // 608
    resp.add_val(0); // 609
    resp.add_val(1708336504); // 610
    resp.add_val(0); // 611
    resp.add_val(0); // 612
    resp.add_val(0); // 613
    resp.add_val(0); // 614
    resp.add_val(0); // 615
    resp.add_val(0); // 616
    resp.add_val(1); // 617
    resp.add_val(0); // 618
    resp.add_val(0); // 619
    resp.add_val(0); // 620
    resp.add_val(0); // 621
    resp.add_val(0); // 622
    resp.add_val(0); // 623 own_treasure_skill
    resp.add_val(0); // 624 own_instr_skill
    resp.add_val(0); // 625
    resp.add_val(30); // 626
    resp.add_val(0); // 627 hydra_next_battle
    resp.add_val(0); // 628 remaining_pet_battles
    resp.add_val(0); // 629
    resp.add_val(0); // 630
    resp.add_val(0); // 631
    resp.add_val(0); // 632
    resp.add_val(0); // 633
    resp.add_val(0); // 634
    resp.add_val(0); // 635
    resp.add_val(0); // 636
    resp.add_val(0); // 637
    resp.add_val(0); // 638
    resp.add_val(0); // 639
    resp.add_val(0); // 640
    resp.add_val(0); // 641
    resp.add_val(0); // 642
    resp.add_val(0); // 643
    resp.add_val(0); // 644
    resp.add_val(0); // 645
    resp.add_val(0); // 646
    resp.add_val(0); // 647
    resp.add_val(0); // 648
    resp.add_val(in_seconds(60 * 60)); // 649 calendar_next_possible
    resp.add_val(res.get::<i64>(46)?); // 650 dice_games_next_free
    resp.add_val(res.get::<i64>(47)?); // 651 dice_games_remaining
    resp.add_val(0); // 652
    resp.add_val(0); // 653 druid mask
    resp.add_val(0); // 654
    resp.add_val(0); // 655
    resp.add_val(0); // 656
    resp.add_val(6); // 657
    resp.add_val(0); // 658
    resp.add_val(2); // 659
    resp.add_val(0); // 660 pet dungeon timer
    resp.add_val(0); // 661
    resp.add_val(0); // 662
    resp.add_val(0); // 663
    resp.add_val(0); // 664
    resp.add_val(0); // 665
    resp.add_val(0); // 666
    resp.add_val(0); // 667
    resp.add_val(0); // 668
    resp.add_val(0); // 669
    resp.add_val(0); // 670
    resp.add_val(1950020000000i64); // 671
    resp.add_val(0); // 672
    resp.add_val(0); // 673
    resp.add_val(0); // 674
    resp.add_val(0); // 675
    resp.add_val(0); // 676
    resp.add_val(0); // 677
    resp.add_val(0); // 678
    resp.add_val(0); // 679
    resp.add_val(0); // 680
    resp.add_val(0); // 681
    resp.add_val(0); // 682
    resp.add_val(0); // 683
    resp.add_val(0); // 684
    resp.add_val(0); // 685
    resp.add_val(0); // 686
    resp.add_val(0); // 687
    resp.add_val(0); // 688
    resp.add_val(0); // 689
    resp.add_val(0); // 690
    resp.add_val(0); // 691
    resp.add_val(1); // 692
    resp.add_val(0); // 693
    resp.add_val(0); // 694
    resp.add_val(0); // 695
    resp.add_val(0); // 696
    resp.add_val(0); // 697
    resp.add_val(0); // 698
    resp.add_val(0); // 699
    resp.add_val(0); // 700
    resp.add_val(0); // 701 bard instrument
    resp.add_val(0); // 702
    resp.add_val(0); // 703
    resp.add_val(1); // 704
    resp.add_val(0); // 705
    resp.add_val(0); // 706
    resp.add_val(0); // 707
    resp.add_val(0); // 708
    resp.add_val(0); // 709
    resp.add_val(0); // 710
    resp.add_val(0); // 711
    resp.add_val(0); // 712
    resp.add_val(0); // 713
    resp.add_val(0); // 714
    resp.add_val(0); // 715
    resp.add_val(0); // 716
    resp.add_val(0); // 717
    resp.add_val(0); // 718
    resp.add_val(0); // 719
    resp.add_val(0); // 720
    resp.add_val(0); // 721
    resp.add_val(0); // 722
    resp.add_val(0); // 723
    resp.add_val(0); // 724
    resp.add_val(0); // 725
    resp.add_val(0); // 726
    resp.add_val(0); // 727
    resp.add_val(0); // 728
    resp.add_val(0); // 729
    resp.add_val(0); // 730
    resp.add_val(0); // 731
    resp.add_val(0); // 732
    resp.add_val(0); // 733
    resp.add_val(0); // 734
    resp.add_val(0); // 735
    resp.add_val(0); // 736
    resp.add_val(0); // 737
    resp.add_val(0); // 738
    resp.add_val(0); // 739
    resp.add_val(0); // 740
    resp.add_val(0); // 741
    resp.add_val(0); // 742
    resp.add_val(0); // 743
    resp.add_val(0); // 744
    resp.add_val(0); // 745
    resp.add_val(0); // 746
    resp.add_val(0); // 747
    resp.add_val(0); // 748
    resp.add_val(0); // 749
    resp.add_val(0); // 750
    resp.add_val(0); // 751
    resp.add_val(0); // 752
    resp.add_val(0); // 753
    resp.add_val(0); // 754
    resp.add_val(0); // 755
    resp.add_val(0); // 756
    resp.add_val(0); // 757
    resp.add_str(""); // 758

    resp.add_key("resources");
    resp.add_val(pid); // player_id
    resp.add_val(res.get::<i64>(48)?); // mushrooms
    resp.add_val(res.get::<i64>(49)?); // silver
    resp.add_val(0); // lucky coins
    resp.add_val(res.get::<i64>(50)?); // quicksand glasses
    resp.add_val(0); // wood
    resp.add_val(0); // ??
    resp.add_val(0); // stone
    resp.add_val(0); // ??
    resp.add_val(0); // metal
    resp.add_val(0); // arcane
    resp.add_val(0); // souls
                     // Fruits
    for _ in 0..5 {
        resp.add_val(0);
    }

    resp.add_key("owndescription.s");
    resp.add_str(&to_sf_string(res.get_str(51)?));

    resp.add_key("ownplayername.r");
    resp.add_str(res.get_str(52)?);

    let maxrank: i32 = res.get(58)?;

    resp.add_key("maxrank");
    resp.add_val(maxrank);

    resp.add_key("skipallow");
    resp.add_val(0);

    resp.add_key("skipvideo");
    resp.add_val(0);

    resp.add_key("fortresspricereroll");
    resp.add_val(18);

    resp.add_key("timestamp");

    resp.add_val(now());

    resp.add_key("fortressprice.fortressPrice(13)");
    resp.add_str(
        "900/1000/0/0/900/500/35/12/900/200/0/0/900/300/22/0/900/1500/50/17/\
         900/700/7/9/900/500/41/7/900/400/20/14/900/600/61/20/900/2500/40/13/\
         900/400/25/8/900/15000/30/13/0/0/0/0",
    );

    resp.skip_key();

    resp.add_key("unitprice.fortressPrice(3)");
    resp.add_str("600/0/15/5/600/0/11/6/300/0/19/3/");

    resp.add_key("upgradeprice.upgradePrice(3)");
    resp.add_val("28/270/210/28/720/60/28/360/180/");

    resp.add_key("unitlevel(4)");
    resp.add_val("5/25/25/25/");

    resp.skip_key();
    resp.skip_key();

    resp.add_key("petsdefensetype");
    resp.add_val(3);

    resp.add_key("singleportalenemylevel");
    resp.add_val(0);

    resp.skip_key();

    resp.add_key("wagesperhour");
    resp.add_val(10);

    resp.skip_key();

    resp.add_key("dragongoldbonus");
    resp.add_val(30);

    resp.add_key("toilettfull");
    resp.add_val(0);

    resp.add_key("maxupgradelevel");
    resp.add_val(20);

    resp.add_key("cidstring");
    resp.add_str("no_cid");

    if !tracking.is_empty() {
        resp.add_key("tracking.s");
        resp.add_str(tracking);
    }

    resp.add_key("calenderinfo");
    resp.add_str(calendar_info);

    resp.skip_key();

    resp.add_key("iadungeontime");
    resp.add_str("5/1702656000/1703620800/1703707200");

    resp.add_key("achievement(208)");
    resp.add_str(
        "0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/\
         0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/\
         0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/\
         0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/\
         0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/\
         0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/\
         0/0/0/0/",
    );

    resp.add_key("scrapbook.r");
    resp.add_str("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==");

    resp.add_key("smith");
    resp.add_str("5/0");

    resp.add_key("owntowerlevel");
    resp.add_val(0);

    resp.add_key("webshopid");
    resp.add_str("Q7tGCJhe$r464");

    resp.add_key("dailytasklist");
    resp.add_val(98);
    for typ_id in 1..=10 {
        resp.add_val(typ_id); // typ
        resp.add_val(0); // current
        resp.add_val(typ_id); // target
        resp.add_val(10); // reward
    }

    resp.add_key("eventtasklist");
    for typ_id in 1..=99 {
        if typ_id == 73 {
            continue;
        }
        resp.add_val(typ_id); // typ
        resp.add_val(0); // current
        resp.add_val(typ_id); // target
        resp.add_val(typ_id); // reward
    }

    resp.add_key("dailytaskrewardpreview");
    add_reward_previews(resp);

    resp.add_key("eventtaskrewardpreview");

    add_reward_previews(resp);

    resp.add_key("eventtaskinfo");
    resp.add_val(1708300800);
    resp.add_val(1798646399);
    resp.add_val(2); // event typ

    resp.add_key("unlockfeature");

    resp.add_key("dungeonprogresslight(30)");
    resp.add_str(
        "-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/0/-1/-1/-1/-1/-1/\
         -1/-1/-1/-1/-1/-1/-1/",
    );

    resp.add_key("dungeonprogressshadow(30)");
    resp.add_str(
        "-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/\
         -1/-1/-1/-1/-1/-1/-1/",
    );

    resp.add_key("dungeonenemieslight(6)");
    resp.add_str("400/15/2/401/15/2/402/15/2/550/18/0/551/18/0/552/18/0/");

    resp.add_key("currentdungeonenemieslight(2)");
    resp.add_key("400/15/200/1/0/550/18/200/1/0/");

    resp.add_key("dungeonenemiesshadow(0)");

    resp.add_key("currentdungeonenemiesshadow(0)");

    resp.add_key("portalprogress(3)");
    resp.add_val("0/0/0");

    resp.skip_key();

    resp.add_key("expeditions");
    resp.add_val(33);
    resp.add_val(71);
    resp.add_val(32);
    resp.add_val(91);
    resp.add_val(10);
    resp.add_val(5);
    resp.add_val(1500);
    resp.add_val(0);

    resp.add_val(124);
    resp.add_val(44);
    resp.add_val(91);
    resp.add_val(71);
    resp.add_val(16);
    resp.add_val(5);
    resp.add_val(6000);
    resp.add_val(0);

    resp.add_key("expeditionevent");
    resp.add_val(in_seconds(-60 * 60));
    resp.add_val(in_seconds(60 * 60));
    resp.add_val(1);
    resp.add_val(in_seconds(60 * 60));

    resp.add_key("usersettings");
    resp.add_str("en");
    resp.add_val(0);
    resp.add_val(0);
    resp.add_val(0);
    resp.add_str("0");
    resp.add_val(0);

    resp.add_key("mailinvoice");
    resp.add_str("a*******@a****.***");

    resp.add_key("cryptoid");
    resp.add_val(res.get_str(53)?);

    resp.add_key("cryptokey");
    resp.add_val(res.get_str(54)?);

    resp.add_key("pendingrewards");
    for i in 0..20 {
        resp.add_val(9999 + i);
        resp.add_val(2);
        resp.add_val(i);
        resp.add_val("Reward Name");
        resp.add_val(1717777586);
        resp.add_val(1718382386);
    }

    resp.build()
}

fn add_reward_previews(resp: &mut ResponseBuilder) {
    for i in 1..=3 {
        resp.add_val(0);
        resp.add_val(match i {
            1 => 400,
            2 => 123,
            _ => 999,
        });
        let count = 16;
        resp.add_val(count);
        // amount of rewards
        for i in 0..count {
            resp.add_val(i + 1); // typ
            resp.add_val(1000); // typ amount
        }
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time warp")
        .as_secs() as i64
}

fn effective_mount(mount_end: &mut i64, mount: &mut i32) -> f32 {
    if *mount_end > 0 && (*mount_end < now() || *mount == 0) {
        *mount = 0;
        *mount_end = 0;
    }

    match *mount {
        0 => 1.0,
        1 => 0.9,
        2 => 0.8,
        3 => 0.7,
        _ => 0.5,
    }
}

fn in_seconds(secs: i64) -> i64 {
    now() + secs
}

fn is_invalid_name(name: &str) -> bool {
    name.len() < 3
        || name.len() > 20
        || name.starts_with(' ')
        || name.ends_with(' ')
        || name.chars().all(|a| a.is_ascii_digit())
        || name.chars().any(|a| !(a.is_alphanumeric() || a == ' '))
}

fn decrypt_server_request(to_decrypt: &str, key: &str) -> String {
    let text = base64::engine::general_purpose::URL_SAFE
        .decode(to_decrypt)
        .unwrap();

    let mut my_key = [0; 16];
    my_key.copy_from_slice(&key.as_bytes()[..16]);

    let mut cipher = libaes::Cipher::new_128(&my_key);
    cipher.set_auto_padding(false);
    let decrypted = cipher.cbc_decrypt(CRYPTO_IV.as_bytes(), &text);

    String::from_utf8(decrypted).unwrap()
}

fn get_row(
    input: Result<Option<Row>, libsql::Error>,
) -> Result<Row, ServerError> {
    input?.ok_or_else(|| ServerError::Internal)
}

async fn first_row(mut input: Rows) -> Result<Row, ServerError> {
    get_row(input.next().await)
}

async fn first_int(mut input: Rows) -> Result<i64, ServerError> {
    let row = get_row(input.next().await)?;
    let val = row.get_value(0)?;
    Ok(*val.as_integer().ok_or_else(|| ServerError::Internal)?)
}

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(about, version, no_binary_name(true))]
struct CheatCmd {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Level {
        level: i16,
    },
    Class {
        class: i16,
    },
    #[command(name = "set_password")]
    SetPassword {
        new: String,
    },
}

fn xp_for_next_level(level: i32) -> i32 {
    static LOOKUP: [i32; 392] = [
        400, 900, 1400, 1800, 2200, 2890, 3580, 4405, 5355, 6435, 7515, 8925,
        10335, 11975, 13715, 15730, 17745, 20250, 22755, 25620, 28660, 32060,
        35460, 39535, 43610, 48155, 52935, 58260, 63585, 69760, 75935, 82785,
        89905, 97695, 105485, 114465, 123445, 133260, 143425, 154545, 165665,
        178210, 190755, 204430, 218540, 233785, 249030, 266140, 283250, 301715,
        320685, 341170, 361655, 384360, 407065, 431545, 456650, 483530, 510410,
        540065, 569720, 601435, 633910, 668670, 703430, 741410, 779390, 819970,
        861400, 905425, 949450, 997485, 1045520, 1096550, 1148600, 1203920,
        1259240, 1319085, 1378930, 1442480, 1507225, 1575675, 1644125, 1718090,
        1792055, 1870205, 1949685, 2033720, 2117755, 2208040, 2298325, 2393690,
        2490600, 2592590, 2694580, 2803985, 2913390, 3028500, 3145390, 3268435,
        3391480, 3522795, 3654110, 3792255, 3932345, 4079265, 4226185, 4382920,
        4539655, 4703955, 4870500, 5045205, 5219910, 5405440, 5590970, 5785460,
        5982490, 6188480, 6394470, 6613125, 6831780, 7060320, 7291640, 7533530,
        7775420, 8031275, 8287130, 8554570, 8825145, 9107305, 9389465, 9687705,
        9985945, 10296845, 10611275, 10939230, 11267185, 11612760, 11958335,
        12318585, 12682650, 13061390, 13440130, 13839160, 14238190, 14653230,
        15072545, 15508870, 15945195, 16403485, 16861775, 17338505, 17819980,
        18319895, 18819810, 19344795, 19869780, 20414715, 20964770, 21536005,
        22107240, 22705735, 23304230, 23925545, 24552535, 25202340, 25852145,
        26532725, 27213305, 27918540, 28630050, 29367610, 30105170, 30875945,
        31646720, 32445505, 33251010, 34084530, 34918050, 35789075, 36660100,
        37561220, 38469755, 39410080, 40350405, 41330960, 42311515, 43326065,
        44348735, 45405405, 46462075, 47563900, 48665725, 49804020, 50951005,
        52136360, 53321715, 54555530, 55789345, 57064175, 58348500, 59673840,
        60999180, 62378435, 63757690, 65180715, 66614100, 68093535, 69572970,
        71110105, 72647240, 74233350, 75830465, 77476555, 79122645, 80832985,
        82543325, 84305910, 86080505, 87909870, 89739235, 91636870, 93534505,
        95490375, 97459260, 99486380, 101513500, 103616290, 105719080,
        107883715, 110062180, 112305475, 114548770, 116872700, 119196630,
        121589225, 123996780, 126473000, 128949220, 131514215, 134079210,
        136717090, 139371155, 142101400, 144831645, 147656105, 150480565,
        153385655, 156307860, 159310695, 162313530, 165420140, 168526750,
        171718645, 174929030, 178228565, 181528100, 184937365, 188346630,
        191849945, 195373130, 198990370, 202607610, 206345275, 210082940,
        213920015, 217778100, 221739815, 225701530, 229790630, 233879730,
        238078150, 242299140, 246629445, 250959750, 255429090, 259898430,
        264482960, 269091720, 273820565, 278549410, 283425105, 288300800,
        293302740, 298330180, 303483865, 308637550, 313951595, 319265640,
        324712695, 330187105, 335799860, 341412615, 347193920, 352975225,
        358901970, 364857940, 370959350, 377060760, 383345695, 389630630,
        396068325, 402536785, 409164155, 415791525, 422612215, 429432905,
        436420230, 443440385, 450627180, 457813975, 465210300, 472606625,
        480177945, 487784290, 495572280, 503360270, 511368340, 519376410,
        527574890, 535810100, 544235725, 552661350, 561325655, 569989960,
        578853765, 587756750, 596866840, 605976930, 615337095, 624697260,
        634274025, 643892430, 653727435, 663562440, 673667980, 683773520,
        694105920, 704481995, 715093150, 725704305, 736599015, 747493725,
        758634285, 769821230, 781254025, 792686820, 804425165, 816163510,
        828158780, 840203305, 852514095, 864824885, 877455675, 890086465,
        902995095, 915995220, 929193185, 942431150, 956014120, 969597090,
        983470400, 997398375, 1011626725, 1025885075, 1040443405, 1055031735,
        1069933505, 1084893105, 1100166150, 1115439195, 1131099560, 1146759925,
        1162747145, 1178794835, 1195180710, 1211566585, 1228357310, 1245148035,
        1262290985, 1279497900, 1297057040, 1314616180, 1332609455, 1350602730,
        1368963280, 1387391470, 1406199095, 1425006720, 1444267210, 1463527700,
        1483183310,
    ];

    (level - 1)
        .try_into()
        .ok()
        .and_then(|idx: usize| LOOKUP.get(idx).copied())
        .unwrap_or(1500000000)
}

fn get_debug_value(name: &str) -> i64 {
    std::fs::read_to_string(format!("values/{name}.txt"))
        .ok()
        .and_then(|a| a.trim().parse().ok())
        .unwrap_or(0)
}
