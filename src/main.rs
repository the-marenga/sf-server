use std::time::Duration;

use actix_cors::Cors;
use actix_web::{get, web, App, HttpResponse, HttpServer, Responder};
use base64::Engine;
use num_traits::FromPrimitive;
use sf_api::{
    command::AttributeType,
    gamestate::{
        character::{Class, Gender, Mount, Race},
        tavern::QuestLocation,
    },
};
use sqlx::{
    postgres::PgPoolOptions,
    types::chrono::{DateTime, Local, NaiveDateTime},
    Pool, Postgres,
};
use strum::EnumCount;

use crate::response::*;

pub mod misc;
pub mod response;

const INTERNAL_ERR: Response = Response::Error(Error::Internal);

const CRYPTO_IV: &str = "jXT#/vz]3]5X7Jl\\";
const DEFAULT_CRYPTO_ID: &str = "0-00000000000000";
const DEFAULT_SESSION_ID: &str = "00000000000000000000000000000000";
const DEFAULT_CRYPTO_KEY: &str = "[_/$VV&*Qg&)r?~g";
const SERVER_VERSION: u32 = 2001;

pub async fn connect_db() -> Result<Pool<Postgres>, Box<dyn std::error::Error>>
{
    Ok(PgPoolOptions::new()
        .max_connections(500)
        .acquire_timeout(Duration::from_secs(10))
        .connect(env!("DATABASE_URL"))
        .await?)
}

#[derive(Debug)]
pub struct CommandArguments<'a>(Vec<&'a str>);

impl<'a> CommandArguments<'a> {
    pub fn get_int(&self, pos: usize) -> Option<i64> {
        self.0.get(pos).and_then(|a| a.parse().ok())
    }

    pub fn get_str(&self, pos: usize) -> Option<&str> {
        self.0.get(pos).copied()
    }
}

#[get("/req.php")]
async fn request(info: web::Query<Request>) -> impl Responder {
    let request = &info.req;
    let db = connect_db().await.unwrap();

    if request.len() < DEFAULT_CRYPTO_ID.len() + 5 {
        return Error::BadRequest.resp();
    }

    let (crypto_id, encrypted_request) =
        request.split_at(DEFAULT_CRYPTO_ID.len());

    if encrypted_request.is_empty() {
        return Error::BadRequest.resp();
    }

    let (player_id, crypto_key) = match crypto_id == DEFAULT_CRYPTO_ID {
        true => (0, DEFAULT_CRYPTO_KEY.to_string()),
        false => {
            match sqlx::query!(
                "SELECT cryptokey, character.id 
                 FROM character 
                 LEFT JOIN Logindata on logindata.id = character.logindata
                 WHERE cryptoid = $1",
                crypto_id
            )
            .fetch_optional(&db)
            .await
            {
                Ok(Some(val)) => (val.id, val.cryptokey),
                Ok(None) => return Error::InvalidAuth.resp(),
                Err(_) => return INTERNAL_ERR,
            }
        }
    };

    let request = decrypt_server_request(encrypted_request, &crypto_key);

    if request.len() < DEFAULT_SESSION_ID.len() + 5 {
        return Error::BadRequest.resp();
    }

    let (_session_id, request) = request.split_at(DEFAULT_SESSION_ID.len());
    // TODO: Validate session id

    let request = request.trim_matches('|');

    let (command_name, command_args) = request.split_once(':').unwrap();
    if command_name != "Poll" {
        println!("Received: {command_name}: {}", command_args);
    }
    let command_args: Vec<_> = command_args.split('/').collect();

    let command_args = CommandArguments(command_args);

    let mut rng = fastrand::Rng::new();

    if player_id == 0
        && ![
            "AccountCreate", "AccountLogin", "AccountCheck", "AccountDelete",
        ]
        .contains(&command_name)
    {
        return Error::InvalidAuth.resp();
    }

    match command_name {
        "PlayerSetFace" => {
            let Some(race) = command_args.get_int(0).and_then(Race::from_i64)
            else {
                return Error::MissingArgument("race").resp();
            };
            let Some(gender) = command_args
                .get_int(1)
                .map(|a| a.saturating_sub(1))
                .and_then(Gender::from_i64)
            else {
                return Error::MissingArgument("gender").resp();
            };
            let Some(portrait) = command_args.get_str(2) else {
                return Error::MissingArgument("portrait").resp();
            };

            let mut portrait_vals: Vec<u16> = Vec::new();
            for v in portrait.split(',') {
                let Ok(opt) = v.parse() else {
                    return Error::MissingArgument("portait option").resp();
                };
                portrait_vals.push(opt);
            }

            if portrait_vals.len() != 9 {
                return Error::MissingArgument("portait option").resp();
            }

            let mouth = portrait_vals[0];
            let hair = portrait_vals[1];
            let eyebrows = portrait_vals[2];
            let eyes = portrait_vals[3];
            let beard = portrait_vals[4];
            let nose = portrait_vals[5];
            let ears = portrait_vals[6];
            let extra = portrait_vals[7];
            let horns = portrait_vals[8];

            // TODO: Change

            Response::Success
        }
        "AccountCreate" => {
            let Some(name) = command_args.get_str(0) else {
                return Error::MissingArgument("name").resp();
            };
            let Some(password) = command_args.get_str(1) else {
                return Error::MissingArgument("password").resp();
            };
            let Some(mail) = command_args.get_str(2) else {
                return Error::MissingArgument("mail").resp();
            };
            let Some(gender) = command_args
                .get_int(3)
                .map(|a| a.saturating_sub(1))
                .and_then(Gender::from_i64)
            else {
                return Error::MissingArgument("gender").resp();
            };
            let Some(race) = command_args.get_int(4).and_then(Race::from_i64)
            else {
                return Error::MissingArgument("race").resp();
            };

            let Some(class) = command_args
                .get_int(5)
                .map(|a| a.saturating_sub(1))
                .and_then(Class::from_i64)
            else {
                return Error::MissingArgument("class").resp();
            };

            let Some(portrait) = command_args.get_str(6) else {
                return Error::MissingArgument("portrait").resp();
            };

            let mut portrait_vals: Vec<i32> = Vec::new();
            for v in portrait.split(',') {
                let Ok(opt) = v.parse() else {
                    return Error::MissingArgument("portait option").resp();
                };
                portrait_vals.push(opt);
            }

            if portrait_vals.len() != 9 {
                return Error::MissingArgument("portait option").resp();
            }

            let mouth = portrait_vals[0];
            let hair = portrait_vals[1];
            let eyebrows = portrait_vals[2];
            let eyes = portrait_vals[3];
            let beard = portrait_vals[4];
            let nose = portrait_vals[5];
            let ears = portrait_vals[6];
            let extra = portrait_vals[7];
            let horns = portrait_vals[8];

            if is_invalid_name(name) {
                return Error::InvalidName.resp();
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

            let Ok(mut tx) = db.begin().await else {
                return INTERNAL_ERR;
            };

            let Ok(login_id) = sqlx::query_scalar!(
                "INSERT INTO LOGINDATA (mail, pwhash, SessionID, CryptoID, \
                 CryptoKey) VALUES ($1, $2, $3, $4, $5) returning ID",
                mail,
                hashed_password,
                session_id,
                crypto_id,
                crypto_key
            )
            .fetch_one(&mut *tx)
            .await
            else {
                _ = tx.rollback().await;
                return INTERNAL_ERR;
            };

            let mut quests = [0; 3];
            #[allow(clippy::needless_range_loop)]
            for i in 0..3 {
                let Ok(quest_id) = sqlx::query_scalar!(
                    "INSERT INTO QUEST (monster, location, length) VALUES \
                     ($1, $2, $3) returning ID",
                    139,
                    1,
                    60,
                )
                .fetch_one(&mut *tx)
                .await
                else {
                    _ = tx.rollback().await;
                    return INTERNAL_ERR;
                };
                quests[i] = quest_id;
            }

            let Ok(tavern_id) = sqlx::query_scalar!(
                "INSERT INTO TAVERN (quest1, quest2, quest3) VALUES ($1, $2, \
                 $3) returning ID",
                quests[0],
                quests[1],
                quests[2],
            )
            .fetch_one(&mut *tx)
            .await
            else {
                _ = tx.rollback().await;
                return INTERNAL_ERR;
            };

            let Ok(bag_id) = sqlx::query_scalar!(
                "INSERT INTO BAG (pos1) VALUES (NULL) returning ID",
            )
            .fetch_one(&mut *tx)
            .await
            else {
                _ = tx.rollback().await;
                return INTERNAL_ERR;
            };

            let Ok(attr_id) = sqlx::query_scalar!(
                "INSERT INTO Attributes
                ( Strength, Dexterity, Intelligence, Stamina, Luck ) 
                VALUES ($1, $2, $3, $4, $5) returning ID",
                3,
                6,
                8,
                2,
                4
            )
            .fetch_one(&mut *tx)
            .await
            else {
                _ = tx.rollback().await;
                return INTERNAL_ERR;
            };

            let Ok(attr_upgrades) = sqlx::query_scalar!(
                "INSERT INTO Attributes
                ( Strength, Dexterity, Intelligence, Stamina, Luck ) 
                VALUES ($1, $2, $3, $4, $5) returning ID",
                0,
                0,
                0,
                0,
                0
            )
            .fetch_one(&mut *tx)
            .await
            else {
                _ = tx.rollback().await;
                return INTERNAL_ERR;
            };

            let Ok(portrait_id) = sqlx::query_scalar!(
                "INSERT INTO PORTRAIT (Mouth, Hair, Brows, Eyes, Beards, \
                 Nose, Ears, Horns, extra) VALUES ($1, $2, $3, $4, $5, $6, \
                 $7, $8, $9) returning ID",
                mouth,
                hair,
                eyebrows,
                eyes,
                beard,
                nose,
                ears,
                horns,
                extra
            )
            .fetch_one(&mut *tx)
            .await
            else {
                _ = tx.rollback().await;
                return INTERNAL_ERR;
            };

            let pid = match sqlx::query_scalar!(
                "INSERT INTO Character
                (Name, Class, Attributes, AttributesBought, LoginData, Tavern, \
                 Bag, Portrait, Gender, Race)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) returning ID",
                name,
                class as i32 + 1,
                attr_id,
                attr_upgrades,
                login_id,
                tavern_id,
                bag_id,
                portrait_id,
                gender as i32 + 1,
                race as i32
            )
            .fetch_one(&mut *tx)
            .await
            {
                Ok(pid) => pid,
                Err(_) => {
                    _ = tx.rollback().await;
                    return INTERNAL_ERR;
                }
            };

            if tx.commit().await.is_err() {
                return INTERNAL_ERR;
            };

            ResponseBuilder::default()
                .add_key("tracking.s")
                .add_str("signup")
                .build()
        }
        "AccountLogin" => {
            let Some(name) = command_args.get_str(0) else {
                return Error::MissingArgument("name").resp();
            };
            let Some(full_hash) = command_args.get_str(1) else {
                return Error::MissingArgument("password hash").resp();
            };
            let Some(login_count) = command_args.get_int(2) else {
                return Error::MissingArgument("login count").resp();
            };

            let Ok(info) = sqlx::query!(
                "SELECT character.id, pwhash, character.logindata FROM \
                 character LEFT JOIN logindata on logindata.id = \
                 character.logindata WHERE lower(name) = lower($1)",
                name
            )
            .fetch_one(&db)
            .await
            else {
                return INTERNAL_ERR;
            };

            let correct_full_hash =
                sha1_hash(&format!("{}{login_count}", info.pwhash));
            if correct_full_hash != full_hash {
                return Error::WrongPassword.resp();
            }

            let session_id: String = (0..DEFAULT_SESSION_ID.len())
                .map(|_| rng.alphanumeric())
                .collect();

            let mut crypto_id = "0-".to_string();
            for _ in 2..DEFAULT_CRYPTO_ID.len() {
                let rc = rng.alphabetic();
                crypto_id.push(rc);
            }

            if sqlx::query!(
                "UPDATE logindata 
                set sessionid = $2, cryptoid = $3
                where id = $1",
                info.logindata,
                session_id,
                crypto_id
            )
            .execute(&db)
            .await
            .is_err()
            {
                return INTERNAL_ERR;
            };

            player_poll(info.id, "accountlogin", &db).await
        }
        "AccountSetLanguage" => {
            // NONE
            Response::Success
        }
        "PlayerHelpshiftAuthtoken" => ResponseBuilder::default()
            .add_key("helpshiftauthtoken")
            .add_val("+eZGNZyCPfOiaufZXr/WpzaaCNHEKMmcT7GRJOGWJAU=")
            .build(),
        "PlayerGetHallOfFame" => {
            let rank = command_args.get_int(0).unwrap_or_default();
            let pre = command_args.get_int(2).unwrap_or_default();
            let post = command_args.get_int(3).unwrap_or_default();
            let name = command_args.get_str(1);

            let rank = match rank {
                1.. => rank,
                _ => {
                    let Some(name) = name else {
                        return Error::MissingArgument("name or rank").resp();
                    };

                    let Ok(info) = sqlx::query!(
                        "SELECT honor, id from character where name = $1", name
                    )
                    .fetch_one(&db)
                    .await
                    else {
                        return INTERNAL_ERR;
                    };

                    let Ok(Some(rank)) = sqlx::query_scalar!(
                        "SELECT count(*) from character where honor > $1 OR \
                         honor = $1 AND id <= $2",
                        info.honor,
                        info.id
                    )
                    .fetch_one(&db)
                    .await
                    else {
                        return INTERNAL_ERR;
                    };
                    rank
                }
            };

            let offset = (rank - pre).max(1) - 1;
            let limit = (pre + post).min(30);

            let Ok(results) = sqlx::query!(
                "SELECT id, level, name, class, honor FROM CHARACTER ORDER BY \
                 honor desc, id OFFSET $1
                 LIMIT $2",
                offset,
                limit,
            )
            .fetch_all(&db)
            .await
            else {
                return INTERNAL_ERR;
            };

            let mut players = String::new();
            for (pos, result) in results.into_iter().enumerate() {
                let player = format!(
                    "{},{},{},{},{},{},{};",
                    offset + pos as i64 + 1,
                    &result.name,
                    "",
                    result.level,
                    result.honor,
                    result.class,
                    ""
                );
                players.push_str(&player);
            }

            ResponseBuilder::default()
                .add_key("Ranklistplayer.r")
                .add_str(&players)
                .build()
        }
        "AccountDelete" => {
            let Some(name) = command_args.get_str(0) else {
                return Error::MissingArgument("name").resp();
            };
            let Some(full_hash) = command_args.get_str(1) else {
                return Error::MissingArgument("password hash").resp();
            };
            let Some(login_count) = command_args.get_int(2) else {
                return Error::MissingArgument("login count").resp();
            };
            let Some(_mail) = command_args.get_str(3) else {
                return Error::MissingArgument("mail").resp();
            };
            let Ok(info) = sqlx::query!(
                "SELECT character.id, pwhash FROM character LEFT JOIN \
                 logindata on logindata.id = character.logindata WHERE \
                 lower(name) = lower($1)",
                name,
            )
            .fetch_one(&db)
            .await
            else {
                return INTERNAL_ERR;
            };

            let correct_full_hash =
                sha1_hash(&format!("{}{login_count}", info.pwhash));
            if correct_full_hash != full_hash {
                return Error::WrongPassword.resp();
            }

            match sqlx::query!("DELETE FROM character WHERE id = $1", info.id)
                .execute(&db)
                .await
            {
                Ok(_) => Response::Success,
                Err(_) => INTERNAL_ERR,
            }
        }
        "PlayerTutorialStatus" => Response::Success,
        "Poll" => player_poll(player_id, "poll", &db).await,
        "AccountCheck" => {
            let Some(name) = command_args.get_str(0) else {
                return Error::MissingArgument("name").resp();
            };

            if is_invalid_name(name) {
                return Error::InvalidName.resp();
            }

            let Ok(count) = sqlx::query_scalar!(
                "SELECT COUNT(*) FROM CHARACTER WHERE name = $1", name
            )
            .fetch_one(&db)
            .await
            else {
                return INTERNAL_ERR;
            };

            match count {
                Some(0) => ResponseBuilder::default()
                    .add_key("serverversion")
                    .add_val(SERVER_VERSION)
                    .add_key("preregister")
                    .add_val(0)
                    .add_val(0)
                    .build(),
                _ => Error::CharacterExists.resp(),
            }
        }
        _ => {
            println!("Unknown command: {command_name} - {:?}", command_args);
            Error::UnknownRequest.resp()
        }
    }
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

async fn player_poll(
    pid: i32,
    tracking: &str,
    db: &Pool<Postgres>,
) -> Response {
    let mut builder = ResponseBuilder::default();
    let resp = builder
        .add_key("serverversion")
        .add_val(SERVER_VERSION)
        .add_key("preregister")
        .add_val(0) // TODO: This has values
        .add_val(0)
        .skip_key();

    let Ok(player) = sqlx::query!(
        "SELECT character.*, logindata.sessionid, logindata.cryptoid, 
            logindata.cryptokey, logindata.logincount, 
            portrait.mouth, portrait.Hair, portrait.Brows, portrait.Eyes, \
         portrait.Beards, portrait.Nose, portrait.Ears, portrait.Extra, \
         portrait.Horns, tavern.tfa, tavern.BeerDrunk, tavern.QuickSand, \
         tavern.DiceGamesRemaining, tavern.DiceGameNextFree
        FROM CHARACTER LEFT JOIN logindata on logindata.id = \
         character.logindata LEFT JOIN portrait on portrait.id = \
         character.portrait LEFT JOIN tavern on tavern.id = character.tavern \
         WHERE character.id = $1",
        pid
    )
    .fetch_one(db)
    .await
    else {
        return Error::BadRequest.resp();
    };

    let calendar_info = "12/1/8/1/3/1/25/1/5/1/2/1/3/2/1/1/24/1/18/5/6/1/22/1/\
                         7/1/6/2/8/2/22/2/5/2/2/2/3/3/21/1";

    resp.add_key("messagelist.r");
    resp.add_str(";");

    resp.add_key("combatloglist.s");
    resp.add_str(";");

    resp.add_key("friendlist.r");
    resp.add_str(";");

    resp.add_key("login count");
    resp.add_val(player.logincount);

    resp.skip_key();

    resp.add_key("sessionid");
    resp.add_str(&player.sessionid);

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

    resp.add_key("dungeonlevel(26)");
    resp.add_str("0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0");

    resp.add_key("shadowlevel(21)");
    resp.add_str("0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0");

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
    resp.add_val(player.level); // Level & arena
    resp.add_val(player.experience); // Experience
    resp.add_val(400); // Next Level XP
    resp.add_val(player.honor); // Honor

    let Ok(Some(rank)) = sqlx::query_scalar!(
        "SELECT count(*) from character where honor > $1 OR honor = $1 AND ID \
         <= $2",
        player.honor,
        pid
    )
    .fetch_one(db)
    .await
    else {
        return INTERNAL_ERR;
    };

    resp.add_val(rank); // Rank

    resp.add_val(0); // 12?
    resp.add_val(10); // 13?
    resp.add_val(0); // 14?
    resp.add_val(15); // 15?
    resp.add_val(0); // 16?

    // Portrait start
    resp.add_val(player.mouth);
    resp.add_val(player.hair);
    resp.add_val(player.brows);
    resp.add_val(player.eyes);
    resp.add_val(player.beards);
    resp.add_val(player.nose);
    resp.add_val(player.ears);
    resp.add_val(player.extra);
    resp.add_val(player.horns);
    resp.add_val(30); // 26?
    resp.add_val(player.race);
    resp.add_val(player.gender); // Gender & Mirror
    resp.add_val(player.class);

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

    resp.add_val(0); // Current action
    resp.add_val(0); // Secondary (time busy)
    resp.add_val(0); // Busy until

    // Equipment
    for _ in 0..10 {
        for _ in 0..12 {
            resp.add_val(0); // 48..=167
        }
    }

    // Inventory bag
    for _ in 0..5 {
        for _ in 0..12 {
            resp.add_val(0); // 168..=227
        }
    }

    resp.add_val(in_seconds(60 * 60)); // 228

    // Ok, so Flavour 1, Flavour 2 & Monster ID decide =>
    // - The Line they say
    // - the quest name
    // - the quest giver

    resp.add_val(4); // 229 Quest1 Flavour1
    resp.add_val(4); // 230 Quest2 Flavour1
    resp.add_val(4); // 231 Quest3 Flavour1

    resp.add_val(4); // 232 Quest1 Flavour2
    resp.add_val(4); // 233 Quest2 Flavour2
    resp.add_val(4); // 234 Quest3 Flavour2

    resp.add_val(-139); // 235 quest 1 monster
    resp.add_val(-139); // 236 quest 2 monster
    resp.add_val(-139); // 237 quest 3 monster

    resp.add_val(QuestLocation::SkullIsland as i32); // 238 quest 1 location
    resp.add_val(QuestLocation::SkullIsland as i32); // 239 quest 2 location
    resp.add_val(QuestLocation::SkullIsland as i32); // 240 quest 3 location

    resp.add_val(5); // 241 quest 1 length
    resp.add_val(5); // 242 quest 2 length
    resp.add_val(5); // 243 quest 3 length

    // Quest 1..=3 items
    for _ in 0..3 {
        for _ in 0..12 {
            resp.add_val(0); // 244..=279
        }
    }

    resp.add_val(1000); // 280 quest 1 xp
    resp.add_val(2000); // 281 quest 2 xp
    resp.add_val(3000); // 282 quest 3 xp

    resp.add_val(1000); // 283 quest 1 silver
    resp.add_val(2000); // 284 quest 2 silver
    resp.add_val(3000); // 285 quest 3 silver

    resp.add_val(Mount::Dragon as u32); // Mount?

    // Weapon shop
    resp.add_val(1708336503); // 287
    for _ in 0..6 {
        for _ in 0..12 {
            resp.add_val(0); // 288..=359
        }
    }

    // Magic shop
    resp.add_val(1708336503); // 360
    for _ in 0..6 {
        for _ in 0..12 {
            resp.add_val(0); // 361..=432
        }
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
    resp.add_val(to_seconds(
        Local::now() + Duration::from_secs(60 * 60 * 24 * 7),
    )); // 451 Mount end
    resp.add_val(0); // 452
    resp.add_val(0); // 453
    resp.add_val(0); // 454
    resp.add_val(1708336503); // 455
    resp.add_val(player.tfa); // 456 Alu secs
    resp.add_val(player.beerdrunk); // 457 Beer drunk
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
    resp.add_val(1708336503); // 580  wheel_next_free_spin

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
    resp.add_val(0xFFFFFFF); // 597 Pretty sure this is a bit map of which messages have been seen
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
    resp.add_val(1708387201); // 649 calendar_next_possible
    resp.add_val(0); // 650 dice_games_next_free
    resp.add_val(10); // 651 dice_games_remaining
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
    resp.add_val(player.mushrooms); // mushrooms
    resp.add_val(player.silver); // silver
    resp.add_val(0); // lucky coins
    resp.add_val(player.quicksand); // quicksand glasses
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
    resp.add_str("");

    resp.add_key("ownplayername.r");
    resp.add_str(&player.name);

    let Ok(Some(maxrank)) =
        sqlx::query_scalar!("SELECT count(*) from character",)
            .fetch_one(db)
            .await
    else {
        return INTERNAL_ERR;
    };

    resp.add_key("maxrank");
    resp.add_val(maxrank);

    resp.add_key("skipallow");
    resp.add_val(0);

    resp.add_key("skipvideo");
    resp.add_val(1);

    resp.add_key("fortresspricereroll");
    resp.add_val(18);

    resp.add_key("timestamp");

    resp.add_val(to_seconds(Local::now()));

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

    resp.add_key("tracking.s");
    resp.add_str(tracking);
    // resp.add_str("accountlogin");

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

    resp.skip_key();

    resp.add_key("smith");
    resp.add_str("5/0");

    resp.skip_key();

    resp.add_key("owntowerlevel");
    resp.add_val(0);

    for _ in 0..8 {
        resp.skip_key();
    }

    resp.add_key("webshopid");
    resp.add_str("Q7tGCJhe$r464");

    resp.skip_key();

    resp.add_key("dailytasklist");
    resp.add_str(
        "6/1/0/10/1/3/0/10/1/4/0/20/1/1/0/3/2/4/0/1/2/1/0/1/2/4/0/5/2/14/0/3/\
         4/25/0/3/4",
    );

    resp.add_key("eventtasklist");
    resp.add_str("54/0/20/1/79/0/50/1/71/0/30/1/72/0/5/1");

    resp.add_key("dailytaskrewardpreview");
    resp.add_str("0/5/1/24/133/0/10/1/24/133/0/13/1/4/400");

    resp.add_val("eventtaskrewardpreview");
    resp.add_str("0/1/2/9/6/8/4/0/2/1/4/800/0/3/2/4/200/28/1");

    resp.add_key("eventtaskinfo");
    resp.add_str("1708300800/1708646399/6");

    resp.add_key("unlockfeature");

    resp.add_key("dungeonprogresslight(30)");
    resp.add_str(
        "-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/0/-1/-1/-1/-1/-1/\
         -1/-1/-1/-1/-1/-1/-1/",
    );

    resp.add_key("ungeonprogressshadow(30)");
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

    resp.add_key("expeditionevent");
    resp.add_str("0/0/0/0");

    resp.add_key("cryptoid");
    resp.add_val(&player.cryptoid);

    resp.add_key("cryptokey");
    resp.add_val(&player.cryptokey);

    resp.build()
}

fn in_seconds(secs: u64) -> i64 {
    to_seconds(Local::now() + Duration::from_secs(secs))
}

fn to_seconds(time: DateTime<Local>) -> i64 {
    let a = time.naive_local();
    let b = NaiveDateTime::from_timestamp_opt(0, 0).unwrap();
    (a - b).num_seconds()
}

fn is_invalid_name(name: &str) -> bool {
    name.len() < 3
        || name.len() > 20
        || name.starts_with(' ')
        || name.ends_with(' ')
        || name.chars().all(|a| a.is_ascii_digit())
        || name.chars().any(|a| !(a.is_alphanumeric() || a == ' '))
}

#[get("/{tail:.*}")]
async fn unhandled(path: web::Path<String>) -> impl Responder {
    println!("Unhandled request: {path}");
    HttpResponse::NotFound()
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    HttpServer::new(|| {
        App::new()
            .wrap(Cors::permissive())
            .service(request)
            .service(unhandled)
    })
    .bind(("0.0.0.0", 6767))?
    .run()
    .await
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
