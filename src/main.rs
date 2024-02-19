use std::time::Duration;

use actix_cors::Cors;
use actix_web::{get, web, App, HttpResponse, HttpServer, Responder};
use base64::Engine;
use num_traits::FromPrimitive;
use sf_api::gamestate::character::{Class, Gender, Race};
use sqlx::{postgres::PgPoolOptions, Pool, Postgres};

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

    let (crypto_id, encrypted_request) = request.split_at(DEFAULT_CRYPTO_ID.len());

    let crypto_key = match crypto_id == DEFAULT_CRYPTO_ID {
        true => DEFAULT_CRYPTO_KEY.to_string(),
        false => todo!("Handle logged in accounts"),
    };

    let request = decrypt_server_request(encrypted_request, &crypto_key);

    let (session_id, request) = request.split_at(DEFAULT_SESSION_ID.len());
    let request = request.trim_matches('|');

    let (command_name, command_args) = request.split_once(':').unwrap();
    let command_args: Vec<_> = command_args.split('/').collect();
    let command_args = CommandArguments(command_args);

    let db = connect_db().await.unwrap();

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
                println!("Bad race");
                return BAD_REQUEST;
            };

            let Some(class) = command_args
                .get_int(5)
                .map(|a| a.saturating_sub(1))
                .and_then(Class::from_i64)
            else {
                println!("Bad class");
                return BAD_REQUEST;
            };

            if is_invalid_name(name) {
                return Error::InvalidName.into_resp();
            }

            // TODO: Do some more input validation
            let hashed_password = password;

            let mut rng = fastrand::Rng::new();

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

            let Ok(id) = sqlx::query_scalar!(
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

            let mut builder = ResponseBuilder::default();
            let resp = builder
                .add_key("serverversion")
                .add_val(SERVER_VERSION)
                .add_key("preregister")
                .add_val(0) // TODO: This has values
                .add_val(0)
                .skip_key();

            resp.add_key("messagelist.r");
            // TODO: InboxEntries
            resp.add_str_val(";");


            resp.add_key("combatloglist.s");

            // combatloglist.s:;&friendlist.r:;&login count:3&&sessionid:ExqLe21hg9w2UVFKt0fQ1pdFFUExxf9L&languagecodelist.r:ru,20;fi,8;ar,1;tr,23;nl,16;  ,0;ja,14;it,13;sk,21;fr,9;ko,15;pl,17;cs,2;el,5;da,3;en,6;hr,10;de,4;zh,24;sv,22;hu,11;pt,12;es,7;pt-br,18;ro,19;&maxpetlevel:100&calenderinfo:12/1/8/1/3/1/25/1/5/1/2/1/3/2/1/1/24/1/18/5/6/1/22/1/7/1/6/2/8/2/22/2/5/2/2/2/3/3/21/1&&tavernspecial:0&tavernspecialsub:0&tavernspecialend:-1&dungeonlevel(26):0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0&shadowlevel(21):0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0&attbonus1(3):0/0/0/0&attbonus2(3):0/0/0/0&attbonus3(3):0/0/0/0&attbonus4(3):0/0/0/0&attbonus5(3):0/0/0/0&stoneperhournextlevel:50&woodperhournextlevel:150&fortresswalllevel:5&inboxcapacity:100&ownplayersave.playerSave:403127023/174281/0/1708336503/1292388336/0/0/1/0/400/100/0/0/10/0/15/0/2/305/305/3/302/3/5/12/0/0/3/1/10/10/8/17/14/16/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/1/1001/6/12/0/0/0/0/0/0/1/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/1708336504/1/1/1/1/2/4/-11/-103/-42/7/21/20/45/45/30/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/147/256/145/26/18/12/0/1708336503/5/1001/2/0/4/5/2/0/0/0/25/0/1/1001/8/10/4/3/5/0/0/0/27/0/7/1001/2/0/5/4/2/0/0/0/23/0/3/1001/2/0/4/3/5/0/0/0/23/0/4/1001/3/0/3/4/5/0/0/0/24/0/5/1001/2/0/3/1/4/0/0/0/24/0/1708336503/12/3/0/0/11/3/0/72/10/0/18/0/9/1/0/0/3/0/0/6/0/0/75/1/12/2/0/0/11/2/0/72/10/0/18/0/8/2/0/0/3/0/0/2/0/0/50/0/17/1/0/0/0/0/0/0/0/0/13/0/8/7/0/0/1/0/0/2/0/0/50/0/0/1/0/0/0/0/0/0/0/0/0/0/0/0/0/6/12/112/0/0/0/0/1708336503/6000/0/0/0/1708336503/0/0/0/0/408/0/0/0/0/0/0/0/0/-111/0/0/4/1708336504/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/6/2/0/0/100/0/0/0/100/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/1708336503/0/100/0/900/300/0/0/0/0/0/0/0/3/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/1708336504/0/0/0/0/0/0/1/0/0/0/0/0/0/0/0/30/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/1708387201/0/10/0/0/0/0/0/6/0/2/0/0/0/0/0/0/0/0/0/0/0/1950020000000/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/1/0/0/0/0/0/0/0/0/0/0/0/1/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/&resources:174281/30/100/0/100/0/0/0/0/0/0/0/0/0/0/0/0&owndescription.s:&ownplayername.r:guiseppo&maxrank:127270&skipallow:0&skipvideo:1&fortresspricereroll:18&timestamp:1708336504&fortressprice.fortressPrice(13):900/1000/0/0/900/500/35/12/900/200/0/0/900/300/22/0/900/1500/50/17/900/700/7/9/900/500/41/7/900/400/20/14/900/600/61/20/900/2500/40/13/900/400/25/8/900/15000/30/13/0/0/0/0&&unitprice.fortressPrice(3):600/0/15/5/600/0/11/6/300/0/19/3/&upgradeprice.upgradePrice(3):28/270/210/28/720/60/28/360/180/&unitlevel(4):5/25/25/25/&&&petsdefensetype:3&singleportalenemylevel:0&&wagesperhour:10&&dragongoldbonus:30&toilettfull:0&maxupgradelevel:20&cidstring:no_cid&tracking.s:accountlogin&calenderinfo:12/1/8/1/3/1/25/1/5/1/2/1/3/2/1/1/24/1/18/5/6/1/22/1/7/1/6/2/8/2/22/2/5/2/2/2/3/3/21/1&&iadungeontime:5/1702656000/1703620800/1703707200&achievement(208):0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/&scrapbook.r:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==&&smith:5/0&&owntowerlevel:0&&&&&&&&&webshopid:Q7tGCJhe$r464&&dailytasklist:6/1/0/10/1/3/0/10/1/4/0/20/1/1/0/3/2/4/0/1/2/1/0/1/2/4/0/5/2/14/0/3/4/25/0/3/4&eventtasklist:54/0/20/1/79/0/50/1/71/0/30/1/72/0/5/1&dailytaskrewardpreview:0/5/1/24/133/0/10/1/24/133/0/13/1/4/400&eventtaskrewardpreview:0/1/2/9/6/8/4/0/2/1/4/800/0/3/2/4/200/28/1&eventtaskinfo:1708300800/1708646399/6&unlockfeature:&dungeonprogresslight(30):-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/0/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/&dungeonprogressshadow(30):-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/-1/&dungeonenemieslight(6):400/15/2/401/15/2/402/15/2/550/18/0/551/18/0/552/18/0/&currentdungeonenemieslight(2):400/15/200/1/0/550/18/200/1/0/&dungeonenemiesshadow(0):&currentdungeonenemiesshadow(0):&portalprogress(3):0/0/0&&expeditionevent:0/0/0/0&cryptoid:0-92HXU8Q21f4gXh&cryptokey:47sF686zf0Z6aOf6



            return resp.build();
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
            todo!("Unknown command: {command_name}")
        }
    }

    Response::Success
}

fn is_invalid_name(name: &str) -> bool {
    println!("{name}");
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
