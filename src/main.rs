use std::time::Duration;

use actix_cors::Cors;
use actix_web::{get, web, App, HttpResponse, HttpServer, Responder};
use base64::Engine;
use num_traits::FromPrimitive;
use sf_api::gamestate::character::{Class, Gender, Race};
use sqlx::{
    postgres::PgPoolOptions,
    types::chrono::{self, DateTime, Local, NaiveDateTime},
    Pool, Postgres,
};

use crate::response::*;

pub mod response;

const BAD_REQUEST: Response = Response::Error(Error::BadRequest);

const CRYPTO_IV: &str = "jXT#/vz]3]5X7Jl\\";
const DEFAULT_CRYPTO_ID: &str = "0-00000000000000";
const DEFAULT_SESSION_ID: &str = "00000000000000000000000000000000";
const DEFAULT_CRYPTO_KEY: &str = "[_/$VV&*Qg&)r?~g";
const SERVER_VERSION: u32 = 2001;

pub async fn connect_db() -> Result<Pool<Postgres>, Box<dyn std::error::Error>> {
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

    let (crypto_id, encrypted_request) = request.split_at(DEFAULT_CRYPTO_ID.len());

    let (player_id, crypto_key) = match crypto_id == DEFAULT_CRYPTO_ID {
        true => (0, DEFAULT_CRYPTO_KEY.to_string()),
        false => {
            match sqlx::query!(
                "SELECT cryptokey, id FROM character WHERE cryptoid = $1",
                crypto_id
            )
            .fetch_one(&db)
            .await
            {
                Ok(val) => (val.id, val.cryptokey),
                Err(_) => return BAD_REQUEST,
            }
        }
    };

    let request = decrypt_server_request(encrypted_request, &crypto_key);

    let (session_id, request) = request.split_at(DEFAULT_SESSION_ID.len());
    // TODO: Validate session id

    let request = request.trim_matches('|');

    let (command_name, command_args) = request.split_once(':').unwrap();
    let command_args: Vec<_> = command_args.split('/').collect();
    let command_args = CommandArguments(command_args);

    let mut rng = fastrand::Rng::new();

    if player_id == 0 && !["AccountCreate", "AccountLogin", "AccountCheck"].contains(&command_name)
    {
        return BAD_REQUEST;
    }

    match command_name {
        "AccountCreate" => {
            let Some(name) = command_args.get_str(0) else {
                return BAD_REQUEST;
            };
            let Some(password) = command_args.get_str(1) else {
                return BAD_REQUEST;
            };
            let Some(mail) = command_args.get_str(2) else {
                return BAD_REQUEST;
            };
            let Some(gender) = command_args
                .get_int(3)
                .map(|a| a.saturating_sub(1))
                .and_then(Gender::from_i64)
            else {
                return BAD_REQUEST;
            };
            let Some(race) = command_args.get_int(4).and_then(Race::from_i64) else {
                return BAD_REQUEST;
            };

            let Some(class) = command_args
                .get_int(5)
                .map(|a| a.saturating_sub(1))
                .and_then(Class::from_i64)
            else {
                return BAD_REQUEST;
            };

            if is_invalid_name(name) {
                return Error::InvalidName.into_resp();
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

            let Ok(pid) = sqlx::query_scalar!(
                "INSERT INTO Character 
                (mail, PWHash, Name, Class, SessionID, CryptoID, CryptoKey)
            VALUES ($1, $2, $3, $4, $5, $6, $7) returning ID",
                mail,
                hashed_password,
                name,
                class as i32,
                session_id,
                crypto_id,
                crypto_key
            )
            .fetch_one(&db)
            .await
            else {
                return BAD_REQUEST;
            };

            return player_poll(pid, "signup", &db).await;
        }
        "AccountLogin" => {
            let Some(name) = command_args.get_str(0) else {
                return BAD_REQUEST;
            };
            let Some(full_hash) = command_args.get_str(1) else {
                return BAD_REQUEST;
            };
            let Some(login_count) = command_args.get_int(2) else {
                return BAD_REQUEST;
            };

            // TODO: Index this
            let Ok(info) =
                sqlx::query!("SELECT id, pwhash from character where name ilike $1", name)
                    .fetch_one(&db)
                    .await
            else {
                return BAD_REQUEST;
            };

            let correct_full_hash = sha1_hash(&format!("{}{login_count}", info.pwhash));
            if correct_full_hash != full_hash {
                return Error::WrongPassword.into_resp();
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
                "UPDATE character 
                    set sessionid = $2, cryptoid = $3
                    where id = $1",
                info.id,
                session_id,
                crypto_id
            )
            .execute(&db)
            .await
            .is_err()
            {
                return BAD_REQUEST;
            };

            return player_poll(info.id, "accountlogin", &db).await;
        }

        "AccountSetLanguage" => {
            // NONE
            return Response::Success;
        }
        "PlayerHelpshiftAuthtoken" => {
            return ResponseBuilder::default()
                .add_key("helpshiftauthtoken")
                .add_val("+eZGNZyCPfOiaufZXr/WpzaaCNHEKMmcT7GRJOGWJAU=")
                .build();
        }
        "PlayerGetHallOfFame" => {
            let mut rb = ResponseBuilder::default();
            rb.add_key("Ranklistplayer.r");

            // TODO: Actually use the args

            // TODO: fetch rank & stuff
            let Ok(info) = sqlx::query!("Select name from character where id = $1", player_id)
                .fetch_one(&db)
                .await
            else {
                return BAD_REQUEST;
            };

            rb.add_str(&format!("1,{},1,60,9,;", &info.name));

            return rb.build();
        }
        "PlayerTutorialStatus" => {
            return Response::Success;
        }
        "Poll" => {
            return player_poll(player_id, "poll", &db).await;
        }
        "AccountCheck" => {
            let Some(name) = command_args.get_str(0) else {
                return BAD_REQUEST;
            };

            if is_invalid_name(name) {
                return Error::InvalidName.into_resp();
            }

            let count = sqlx::query_scalar!("SELECT COUNT(*) FROM CHARACTER WHERE name = $1", name)
                .fetch_one(&db)
                .await
                .unwrap()
                .unwrap_or_default();

            if count == 0 {
                return ResponseBuilder::default()
                    .add_key("serverversion")
                    .add_val(SERVER_VERSION)
                    .add_key("preregister")
                    .add_val(0)
                    .add_val(0)
                    .build();
            }
            return Error::CharacterExists.into_resp();
        }
        _ => {
            println!("Unknown command: {command_name} - {:?}", command_args);
            return Error::BadRequest.into_resp();
        }
    }

    Response::Success
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

async fn player_poll(pid: i32, tracking: &str, db: &Pool<Postgres>) -> Response {
    let mut builder = ResponseBuilder::default();
    let resp = builder
        .add_key("serverversion")
        .add_val(SERVER_VERSION)
        .add_key("preregister")
        .add_val(0) // TODO: This has values
        .add_val(0)
        .skip_key();

    let Ok(player) = sqlx::query!("SELECT * FROM CHARACTER WHERE id = $1", pid)
        .fetch_one(db)
        .await
    else {
        return Error::BadRequest.into_resp();
    };

    let calendar_info =
        "12/1/8/1/3/1/25/1/5/1/2/1/3/2/1/1/24/1/18/5/6/1/22/1/7/1/6/2/8/2/22/2/5/2/2/2/3/3/21/1";

    resp.add_key("messagelist.r");
    resp.add_str(";");

    resp.add_key("combatloglist.s");
    resp.add_str(";");

    resp.add_key("friendlist.r");
    resp.add_str(";");

    resp.add_key("login count");
    resp.add_val(1);

    resp.skip_key();

    resp.add_key("sessionid");
    resp.add_str(&player.sessionid);

    resp.add_key("languagecodelist");
    resp.add_str("ru,20;fi,8;ar,1;tr,23;nl,16;  ,0;ja,14;it,13;sk,21;fr,9;ko,15;pl,17;cs,2;el,5;da,3;en,6;hr,10;de,4;zh,24;sv,22;hu,11;pt,12;es,7;pt-br,18;ro,19;");

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
    resp.add_val("403127023/174281/0/1708336503/1292388336/0/0/1/0/400/100/0/0/10/0/15/0/2/305/305/3/302/3/5/12/0/0/3/1/10/10/8/17/14/16/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/1/1001/6/12/0/0/0/0/0/0/1/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/1708336504/1/1/1/1/2/4/-11/-103/-42/7/21/20/45/45/30/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/147/256/145/26/18/12/0/1708336503/5/1001/2/0/4/5/2/0/0/0/25/0/1/1001/8/10/4/3/5/0/0/0/27/0/7/1001/2/0/5/4/2/0/0/0/23/0/3/1001/2/0/4/3/5/0/0/0/23/0/4/1001/3/0/3/4/5/0/0/0/24/0/5/1001/2/0/3/1/4/0/0/0/24/0/1708336503/12/3/0/0/11/3/0/72/10/0/18/0/9/1/0/0/3/0/0/6/0/0/75/1/12/2/0/0/11/2/0/72/10/0/18/0/8/2/0/0/3/0/0/2/0/0/50/0/17/1/0/0/0/0/0/0/0/0/13/0/8/7/0/0/1/0/0/2/0/0/50/0/0/1/0/0/0/0/0/0/0/0/0/0/0/0/0/6/12/112/0/0/0/0/1708336503/6000/0/0/0/1708336503/0/0/0/0/408/0/0/0/0/0/0/0/0/-111/0/0/4/1708336504/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/6/2/0/0/100/0/0/0/100/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/1708336503/0/100/0/900/300/0/0/0/0/0/0/0/3/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/1708336504/0/0/0/0/0/0/1/0/0/0/0/0/0/0/0/30/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/1708387201/0/10/0/0/0/0/0/6/0/2/0/0/0/0/0/0/0/0/0/0/0/1950020000000/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/1/0/0/0/0/0/0/0/0/0/0/0/1/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/");

    resp.add_key("resources");
    resp.add_val(pid); //player_id
    resp.add_val(1000); // mushrooms
    resp.add_val(10000000); // silver
    resp.add_val(0); // lucky coins
    resp.add_val(100); // quicksand glasses
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

    resp.add_key("maxrank");
    resp.add_val(1);

    resp.add_key("skipallow");
    resp.add_val(0);

    resp.add_key("skipvideo");
    resp.add_val(1);

    resp.add_key("fortresspricereroll");
    resp.add_val(18);

    resp.add_key("timestamp");

    resp.add_val(to_seconds(Local::now()));

    resp.add_key("fortressprice.fortressPrice(13)");
    resp.add_str("900/1000/0/0/900/500/35/12/900/200/0/0/900/300/22/0/900/1500/50/17/900/700/7/9/900/500/41/7/900/400/20/14/900/600/61/20/900/2500/40/13/900/400/25/8/900/15000/30/13/0/0/0/0");

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
    resp.add_str("0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/");

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
    resp.add_str("6/1/0/10/1/3/0/10/1/4/0/20/1/1/0/3/2/4/0/1/2/1/0/1/2/4/0/5/2/14/0/3/4/25/0/3/4");

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
        "-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/0/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/",
    );

    resp.add_key("ungeonprogressshadow(30)");
    resp.add_str("-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/");

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

fn to_seconds(time: DateTime<Local>) -> i64 {
    let a = time.naive_local();
    let b = NaiveDateTime::from_timestamp_opt(0, 0).unwrap();
    let current_secs = (a - b).num_seconds();
    current_secs
}

fn is_invalid_name(name: &str) -> bool {
    name.len() < 3
        || name.len() > 20
        || name.starts_with(' ')
        || name.ends_with(' ')
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
