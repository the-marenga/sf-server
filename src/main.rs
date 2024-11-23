use std::{
    collections::HashMap,
    convert::TryInto,
    fmt::Write,
    net::SocketAddr,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::{Host, Query, Request},
    handler::HandlerWithoutStateExt,
    http::{Method, Uri},
    response::{IntoResponse, Redirect, Response},
    routing::get,
    Router,
};
use base64::Engine;
use log::{debug, warn};
use misc::{from_sf_string, to_sf_string};
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use reqwest::{header::CONTENT_TYPE, StatusCode};
use serde::{Deserialize, Serialize};
use sf_api::{
    command::AttributeType,
    gamestate::{
        character::{Class, Gender, Race},
        items::{Enchantment, EquipmentSlot},
    },
};
use sqlx::{sqlite::SqlitePoolOptions, Sqlite};
use strum::EnumCount;
use tower_http::cors::CorsLayer;

use crate::response::*;

#[allow(dead_code)]
#[derive(Clone, Copy)]
struct Ports {
    http: u16,
    https: u16,
}

// TODO: Make thise config, or env variables
static PROVIDE_HTTPS: bool = true;
static HTTP_PORT: u16 = 6767;
static HTTPS_PORT: u16 = 6768;

#[tokio::main]
async fn main() {
    // initialize tracing
    tracing_subscriber::fmt::init();
    let cors = CorsLayer::new()
        .allow_headers(tower_http::cors::Any)
        .allow_methods([Method::GET, Method::POST])
        .allow_origin(tower_http::cors::Any);

    let app = Router::new()
        .route("/cmd.php", get(handle_cmd))
        .route("/req.php", get(handle_req))
        .route("/*key", get(forward))
        .route("/", get(forward))
        .layer(cors);

    if !PROVIDE_HTTPS {
        let addr = SocketAddr::from(([127, 0, 0, 1], HTTP_PORT));
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        axum::serve(listener, app).await.unwrap();
    } else {
        tokio::spawn(redirect_http_to_https());

        use axum_server::tls_rustls::RustlsConfig;
        let config = RustlsConfig::from_pem_file(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("certs")
                .join("localhost.crt"),
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("certs")
                .join("localhost.key"),
        )
        .await
        .unwrap();

        let addr = SocketAddr::from(([127, 0, 0, 1], HTTPS_PORT));
        println!("listening on https://{}", addr);
        axum_server::bind_rustls(addr, config)
            .serve(app.into_make_service())
            .await
            .unwrap()
    }
}

#[allow(dead_code)]
async fn redirect_http_to_https() {
    fn make_https(host: String, uri: Uri) -> Result<Uri, axum::BoxError> {
        let mut parts = uri.into_parts();

        parts.scheme = Some(axum::http::uri::Scheme::HTTPS);

        if parts.path_and_query.is_none() {
            parts.path_and_query = Some("/".parse().unwrap());
        }

        let https_host =
            host.replace(&HTTP_PORT.to_string(), &HTTPS_PORT.to_string());
        parts.authority = Some(https_host.parse()?);

        Ok(Uri::from_parts(parts)?)
    }

    let redirect = move |Host(host): Host, uri: Uri| async move {
        match make_https(host, uri) {
            Ok(uri) => Ok(Redirect::permanent(&uri.to_string())),
            Err(error) => {
                warn!("failed to convert URI to HTTPS: {error}");
                Err(StatusCode::BAD_REQUEST)
            }
        }
    };

    let addr = SocketAddr::from(([127, 0, 0, 1], HTTP_PORT));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    debug!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, redirect.into_make_service())
        .await
        .unwrap();
}

pub async fn forward(req: Request) -> Response {
    let uri = req.uri().path_and_query().unwrap().as_str();
    let target_url = format!("https://sfgame.net{}", uri);

    if [".webp", ".png", ".jpg"]
        .into_iter()
        .any(|a| target_url.ends_with(a))
    {
        // They cors allow any origin for images, but block some other stuff
        return Redirect::temporary(&target_url).into_response();
    }

    // info!("Intercepted: {uri}");

    let client = reqwest::Client::new();

    if uri == "/js/build.json" {
        let resp = client.get(&target_url).send().await.unwrap();
        let text = resp.text().await.unwrap();
        let mut json: HashMap<String, String> =
            serde_json::from_str(&text).unwrap();
        let fw = json.get_mut("frameworkUrl").unwrap();
        *fw = fw.split_once(".com").unwrap().1.to_string();

        Response::builder()
            .status(200)
            .header(CONTENT_TYPE, "application/json")
            .body(axum::body::Body::new(serde_json::to_string(&json).unwrap()))
            .unwrap()
    } else if uri.ends_with(".framework.js.gz") {
        let target_url = format!("https://cdn.playa-games.com{uri}");
        // info!("Rewrote: {target_url}");
        let resp = client.get(target_url).send().await.unwrap();
        let raw = resp.text().await.unwrap();

        Response::builder()
            .status(200)
            .header(CONTENT_TYPE, "application/javascript")
            .body(axum::body::Body::from(raw.replace(
                "return hasConsentForVendor(vendorID)", "return true",
            )))
            .unwrap()
    } else if uri == "/config.json" {
        let resp = client.get(&target_url).send().await.unwrap();
        let text = resp.text().await.unwrap();
        let mut config: SFConfig = serde_json::from_str(&text).unwrap();

        #[derive(Debug, Serialize, Deserialize)]
        struct SFConfig {
            servers: Vec<ServerEntry>,
            #[serde(flatten)]
            misc: HashMap<String, serde_json::Value>,
        }

        #[derive(Debug, Serialize, Deserialize)]
        struct ServerEntry {
            i: i64,
            d: String,
            c: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            md: Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            m: Option<String>,
        }

        config.servers.clear();

        let db = get_db().await.unwrap();

        let servers = sqlx::query!("SELECT * FROM world")
            .fetch_all(&db)
            .await
            .unwrap();

        for server in servers {
            let server_host = req
                .uri()
                .authority()
                .map(|a| a.as_str())
                .unwrap_or("localhost");
            let server_url = if !server.ident.is_empty() {
                format!("{}.{server_host}", server.ident)
            } else {
                server_host.to_string()
            };

            config.servers.push(ServerEntry {
                i: server.world_id * 2,
                d: server_url.clone(),
                c: "fu".into(),
                md: None,
                m: None,
            });
            // config.servers.push(ServerEntry {
            //     i: server.ID * 2 + 1,
            //     d: "This server is powered by: ".to_string(),
            //     c: "github.com/the-marenga/sf-server".into(),
            //     md: Some(server_url),
            //     m: Some("2023-09-07 19:59:59".into()),
            // });
        }

        Response::builder()
            .status(200)
            .header(CONTENT_TYPE, "application/json")
            .body(axum::body::Body::new(
                serde_json::to_string(&config).unwrap(),
            ))
            .unwrap()
    } else {
        let resp = client.get(target_url).send().await.unwrap();
        let mut builder = Response::builder().status(resp.status());
        for (key, value) in resp.headers().iter() {
            builder = builder.header(key, value);
        }
        let out_stream = resp.bytes_stream();
        builder
            .body(axum::body::Body::from_stream(out_stream))
            .unwrap()
    }
}

pub mod misc;
pub mod response;

const DEFAULT_CRYPTO_ID: &str = "0-00000000000000";
const DEFAULT_SESSION_ID: &str = "00000000000000000000000000000000";
const DEFAULT_CRYPTO_KEY: &str = "[_/$VV&*Qg&)r?~g";
const SERVER_VERSION: u32 = 2008;

pub async fn get_db() -> Result<sqlx::Pool<Sqlite>, ServerError> {
    use async_once_cell::OnceCell;
    static DB: OnceCell<sqlx::Pool<Sqlite>> = OnceCell::new();
    DB.get_or_try_init(async { connect_init_db().await })
        .await
        .cloned()
}

pub async fn connect_init_db() -> Result<sqlx::Pool<Sqlite>, ServerError> {
    Ok(SqlitePoolOptions::new()
        .max_connections(50)
        .connect(env!("DATABASE_URL"))
        .await?)
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

async fn handle_cmd(
    req_params: Query<HashMap<String, String>>,
) -> Result<Response, Response> {
    let db = get_db().await?;
    let command_name = req_params.get("req").get("request")?.as_str();
    let crypto_id = req_params.get("sid").get("crypto_id")?;
    let command_args = req_params.get("params").get("command_args")?;
    let command_args = base64::engine::general_purpose::URL_SAFE
        .decode(command_args)
        .map_err(|_| ServerError::BadRequest)?;
    let command_args =
        String::from_utf8(command_args).map_err(|_| ServerError::BadRequest)?;

    let world = "";

    let session = match crypto_id == DEFAULT_CRYPTO_ID {
        true => {
            let world_id = sqlx::query_scalar!(
                "SELECT world_id
                     FROM world
                     WHERE ident = $1",
                world
            )
            .fetch_one(&db)
            .await
            .map_err(ServerError::DBError)?;

            Session::new_unauthed(world_id)
        }
        false => {
            let res = sqlx::query!(
                "SELECT character.pid, crypto_key, session_id, crypto_id, \
                 world_id, login_count
                     FROM character
                     NATURAL JOIN session
                     NATURAL JOIN world
                     WHERE crypto_id = $1 AND world.ident = $2",
                crypto_id,
                world
            )
            .fetch_optional(&db)
            .await
            .map_err(ServerError::DBError)?;

            match res {
                Some(row) => Session {
                    player_id: row.pid,
                    world_id: row.world_id,
                    session_id: row.session_id,
                    crypto_id: row.crypto_id,
                    crypto_key: row.crypto_key,
                    login_count: row.login_count,
                },
                None => Err(ServerError::InvalidAuth)?,
            }
        }
    };
    let args = CommandArguments(command_args.split('/').collect());

    handle_command(db, command_name, args, session)
        .await
        .map_err(|a| a.into())
        .map(|a| a.into())
}

async fn handle_req(
    req: Query<HashMap<String, String>>,
) -> Result<Response, Response> {
    let request = req.get("req").get("request parameter")?;
    let db = get_db().await?;

    if request.len() < DEFAULT_CRYPTO_ID.len() + 5 {
        Err(ServerError::BadRequest)?;
    }

    let (crypto_id, encrypted_request) =
        request.split_at(DEFAULT_CRYPTO_ID.len());

    if encrypted_request.is_empty() {
        Err(ServerError::BadRequest)?;
    }

    // TODO: Parse from request url
    let world = "";

    let session = match crypto_id == DEFAULT_CRYPTO_ID {
        true => {
            let world_id = sqlx::query_scalar!(
                "SELECT world_id
                     FROM world
                     WHERE ident = $1",
                world
            )
            .fetch_one(&db)
            .await
            .map_err(ServerError::DBError)?;

            Session::new_unauthed(world_id)
        }
        false => {
            let res = sqlx::query!(
                "SELECT character.pid, crypto_key, session_id, crypto_id, \
                 world_id, login_count
                     FROM character
                     NATURAL JOIN session
                     NATURAL JOIN world
                     WHERE crypto_id = $1 AND world.ident = $2",
                crypto_id,
                world
            )
            .fetch_optional(&db)
            .await
            .map_err(ServerError::DBError)?;

            match res {
                Some(row) => Session {
                    player_id: row.pid,
                    world_id: row.world_id,
                    session_id: row.session_id,
                    crypto_id: row.crypto_id,
                    crypto_key: row.crypto_key,
                    login_count: row.login_count,
                },
                None => Err(ServerError::InvalidAuth)?,
            }
        }
    };

    let request =
        decrypt_server_request(encrypted_request, &session.crypto_key);

    let Some((_session_id, request)) = request.split_once('|') else {
        return Err(ServerError::BadRequest)?;
    };

    let request = request.trim_matches('|');

    let (command_name, command_args) = request.split_once(':').unwrap();
    let command_args: Vec<_> = command_args.split('/').collect();
    let args = CommandArguments(command_args);

    handle_command(db, command_name, args, session)
        .await
        .map_err(|a| a.into())
        .map(|a| a.into())
}

pub trait OptionGet<V> {
    fn get(self, name: &'static str) -> Result<V, ServerError>;
}

impl<T> OptionGet<T> for Option<T> {
    fn get(self, name: &'static str) -> Result<T, ServerError> {
        self.ok_or_else(|| ServerError::MissingArgument(name))
    }
}

#[derive(Debug)]
struct Session {
    player_id: i64,
    world_id: i64,
    session_id: String,
    crypto_id: String,
    crypto_key: String,
    login_count: i64,
}

impl Session {
    pub fn new_unauthed(world_id: i64) -> Self {
        Self {
            player_id: -1,
            world_id,
            session_id: DEFAULT_SESSION_ID.to_string(),
            crypto_id: DEFAULT_CRYPTO_ID.to_string(),
            crypto_key: DEFAULT_CRYPTO_KEY.to_string(),
            login_count: 1,
        }
    }

    pub fn can_request(&self, command: &str) -> bool {
        self.player_id > 0
            || [
                "AccountCreate", "AccountLogin", "AccountCheck",
                "AccountDelete", "PlayerHelpshiftAuthtoken",
                "getserverversion", "PlayerWhisper",
            ]
            .contains(&command)
    }
}

async fn handle_command<'a>(
    db: sqlx::Pool<Sqlite>,
    command_name: &'a str,
    args: CommandArguments<'a>,
    session: Session,
) -> Result<ServerResponse, ServerError> {
    let mut rng = fastrand::Rng::new();

    if command_name != "Poll" {
        println!("Received: {command_name}: {:?}", args);
    }

    if !session.can_request(command_name) {
        // TODO: Validate provided session id
        println!("{command_name} requires auth");
        Err(ServerError::InvalidAuth)?;
    }

    match command_name {
        "PlayerSetFace" => {
            character_set_face(&args, &db, session.player_id).await
        }
        "AccountCreate" => {
            let name = args.get_str(0, "name")?;
            let password = args.get_str(1, "password")?;
            let mail = args.get_str(2, "mail")?;
            let gender = args.get_int(3, "gender")?;
            let _gender =
                Gender::from_i64(gender.saturating_sub(1)).get("gender")?;
            let race = args.get_int(4, "race")?;
            let _race = Race::from_i64(race).get("race")?;

            let class = args.get_int(5, "class")?;
            let _class =
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

            let mut tx = db.begin().await?;

            let mut quests = [0; 3];
            #[allow(clippy::needless_range_loop)]
            for i in 0..3 {
                let res = sqlx::query_scalar!(
                    "INSERT INTO QUEST (monster, location, length, xp, \
                     silver, mushrooms)
                    VALUES ($1, $2, $3, $4, $5, $6) returning ID",
                    139,
                    1,
                    60,
                    100,
                    100,
                    1,
                )
                .fetch_one(&mut *tx)
                .await?;
                quests[i] = res;
            }

            let pid = sqlx::query_scalar!(
                "INSERT INTO tavern (quest1, quest2, quest3)
                    VALUES ($1, $2, $3) returning pid",
                quests[0],
                quests[1],
                quests[2]
            )
            .fetch_one(&mut *tx)
            .await?;

            sqlx::query_scalar!("INSERT INTO BAG (pid) VALUES ($1)", pid)
                .execute(&mut *tx)
                .await?;

            let attr_id = sqlx::query_scalar!(
                "INSERT INTO Attributes
                     ( Strength, Dexterity, Intelligence, Stamina, Luck )
                     VALUES ($1, $2, $3, $4, $5) returning ID",
                3,
                6,
                8,
                2,
                4,
            )
            .fetch_one(&mut *tx)
            .await?;

            let attr_upgrades = sqlx::query_scalar!(
                "INSERT INTO Attributes
                    ( Strength, Dexterity, Intelligence, Stamina, Luck )
                    VALUES ($1, $2, $3, $4, $5) returning ID",
                0,
                0,
                0,
                0,
                0,
            )
            .fetch_one(&mut *tx)
            .await?;

            sqlx::query!(
                "INSERT INTO PORTRAIT (Mouth, Hair, Brows, Eyes, Beards, \
                 Nose, Ears, Horns, extra, pid) VALUES ($1, $2, $3, $4, $5, \
                 $6, $7, $8, $9, $10)",
                portrait.mouth,
                portrait.hair,
                portrait.eyebrows,
                portrait.eyes,
                portrait.beard,
                portrait.nose,
                portrait.ears,
                portrait.horns,
                portrait.extra,
                pid
            )
            .execute(&mut *tx)
            .await?;

            sqlx::query!("INSERT INTO Activity (pid) VALUES ($1)", pid)
                .execute(&mut *tx)
                .await?;

            sqlx::query!("INSERT INTO Equipment (pid) VALUES ($1)", pid)
                .execute(&mut *tx)
                .await?;

            sqlx::query!("INSERT INTO guild_upgrade (pid) VALUES ($1)", pid)
                .execute(&mut *tx)
                .await?;

            sqlx::query!(
                "INSERT INTO character (pid, world_id, pw_hash, name, class, \
                 race, gender, attributes, attributes_bought, mail, \
                 crypto_key)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
                pid,
                session.world_id,
                hashed_password,
                name,
                class,
                race,
                gender,
                attr_id,
                attr_upgrades,
                mail,
                crypto_key
            )
            .execute(&mut *tx)
            .await?;

            sqlx::query!(
                "INSERT INTO SESSION (pid, session_id, crypto_id) VALUES ($1, \
                 $2, $3)",
                pid,
                session_id,
                crypto_id,
            )
            .execute(&mut *tx)
            .await?;

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

            let mut tx = db.begin().await?;

            let info = sqlx::query!(
                "SELECT pid, pw_hash, crypto_key FROM
                character WHERE lower(name) = lower($1)",
                name
            )
            .fetch_one(&mut *tx)
            .await?;

            let pid = info.pid;
            let pwhash = info.pw_hash;

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

            sqlx::query!(
                "INSERT INTO session (pid, session_id, crypto_id)
                VALUES ($1, $2, $3)",
                pid,
                session_id,
                crypto_id,
            )
            .execute(&mut *tx)
            .await?;

            tx.commit().await?;

            let mut session = session;
            session.crypto_id = crypto_id;
            session.crypto_key = info.crypto_key;
            session.session_id = session_id;
            session.player_id = pid;
            session.login_count = 1;

            character_poll(session, "accountlogin", &db, Default::default())
                .await
        }
        "AccountSetLanguage" => {
            // NONE
            Ok(ServerResponse::Success)
        }
        "PlayerSetDescription" => {
            let description = args.get_str(0, "description")?;
            let description = from_sf_string(description);
            sqlx::query!(
                "UPDATE character SET description = $1 WHERE pid = $2",
                description, session.player_id
            )
            .execute(&db)
            .await?;
            Ok(character_poll(session, "", &db, Default::default()).await?)
        }
        "PlayerHelpshiftAuthtoken" => ResponseBuilder::default()
            .add_key("helpshiftauthtoken")
            .add_val("+eZGNZyCPfOiaufZXr/WpzaaCNHEKMmcT7GRJOGWJAU=")
            .build(),
        "GroupGetHallOfFame" => {
            let rank = args.get_int(0, "rank").unwrap_or_default();
            let pre = args.get_int(2, "pre").unwrap_or_default();
            let post = args.get_int(3, "post").unwrap_or_default();
            let _name = args.get_str(1, "name or rank");

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
                    //          $1
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

            let res = sqlx::query!(
                "SELECT
                    g.name,
                    c.name as leader,
                    g.honor,
                    (SELECT count(*) AS membercount FROM guild_member as gm \
                 WHERE gm.guild_id = g.id) as `membercount!: i64`,
                    g.attacking
                    FROM guild as g
                    JOIN guild_member as gm on gm.guild_id = g.id
                    NATURAL JOIN character as c
                    WHERE g.world_id = $3 AND RANK = 3
                    ORDER BY g.honor desc, g.id asc
                    LIMIT $2 OFFSET $1",
                offset,
                limit,
                session.world_id
            )
            .fetch_all(&db)
            .await?;

            let mut guilds = String::new();
            for (entry_idx, guild) in res.into_iter().enumerate() {
                guilds
                    .write_fmt(format_args!(
                        "{},{},{},{},{},{};",
                        entry_idx,
                        guild.name,
                        guild.leader,
                        guild.honor,
                        guild.membercount,
                        guild.attacking.map_or(0, |_| 1),
                    ))
                    .unwrap();
            }

            ResponseBuilder::default()
                .add_key("ranklistgroup.r")
                .add_str(&guilds)
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
                    .fetch_one(&db)
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
            .fetch_all(&db)
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
        "AccountDelete" => {
            if true {
                return Ok(ServerResponse::Success);
            }

            let name = args.get_str(0, "account name")?;
            let full_hash = args.get_str(1, "pw hash")?;
            let login_count = args.get_int(2, "login count")?;
            let mail = args.get_str(3, "account mail")?;

            let mut tx = db.begin().await?;

            let res = sqlx::query!(
                "SELECT pid, pw_hash
                    FROM character
                    WHERE lower(name) = lower($1) and mail = $2",
                name,
                mail,
            )
            .fetch_optional(&mut *tx)
            .await?;
            let Some(char) = res else {
                // In case we reset db and char is still in the ui
                return Ok(ServerResponse::Success);
            };

            let id = char.pid;
            let pwhash = char.pw_hash;
            let correct_full_hash =
                sha1_hash(&format!("{}{login_count}", pwhash));
            if correct_full_hash != full_hash {
                return Err(ServerError::WrongPassword);
            }

            // FIXME: Pick another guild leader

            sqlx::query!("DELETE FROM character WHERE pid = $1", id)
                .execute(&mut *tx)
                .await?;

            tx.commit().await?;
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

            let mut tx = db.begin().await?;
            let character_silver = sqlx::query_scalar!(
                "SELECT silver FROM character where pid = $1",
                session.player_id,
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
        "PlayerAdventureStart" => {
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

            character_poll(session, "", &db, Default::default()).await
        }
        "PlayerAdventureFinished" => {
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

            let (_item, location, monster, mush, silver, quest_xp) =
                match subtyp {
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

            resp.add_val(row.influencer); // special influencer portraits

            resp.add_val(row.race); // race
            resp.add_val(row.gender); // gender
            resp.add_val(row.class); // class

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
            resp.add_val(character_lvl); // monster lvl
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

            character_poll(session, "", &db, resp).await
        }
        "PlayerMountBuy" => {
            todo!();
            // let mount = command_args.get_int(0, "mount")?;
            // let mount = mount as i32;

            // let Ok(mut tx) = db.begin().await else {
            //     return INTERNAL_ERR;
            // };

            // let Ok(character) = sqlx::query!(
            //     "SELECT silver, mushrooms, mount, mountend FROM CHARACTER \
            //      WHERE id = $1",
            //     pid
            // )
            // .fetch_one(&mut *tx)
            // .await
            // else {
            //     _ = tx.rollback().await;
            //     return INTERNAL_ERR;
            // };

            // let mut silver = character.silver;
            // let mut mushrooms = character.mushrooms;

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
            // let mount_start = match character.mountend {
            //     Some(x) if character.mount == mount => now.max(x),
            //     _ => now,
            // };

            // if sqlx::query!(
            //     "UPDATE Character SET mount = $1, mountend = $2, mushrooms = \
            //      $4, silver = $5 WHERE id = $3",
            //     mount,
            //     mount_start + Duration::from_secs(60 * 60 * 24 * 14),
            //     pid,
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
            //         character_poll(pid, "", &db, Default::default()).await
            //     }
            // }
        }
        "PlayerTutorialStatus" => {
            let status = args.get_int(0, "tutorial status")?;
            if !(0..=0xFFFFFFF).contains(&status) {
                Err(ServerError::BadRequest)?;
            }
            sqlx::query!(
                "UPDATE CHARACTER SET tutorial_status = $1 WHERE pid = $2",
                status, session.player_id,
            )
            .execute(&db)
            .await?;
            Ok(ServerResponse::Success)
        }
        "Poll" => Ok(
            character_poll(session, "poll", &db, Default::default()).await?
        ),
        "getserverversion" => {
            let res = sqlx::query!(
                "SELECT
                (SELECT COUNT(*) FROM Character WHERE world_id = $1) as \
                 `charactercount!: i64`,
                    (SELECT COUNT(*) FROM Guild WHERE world_id = $1) as \
                 `guildcount!: i64`
                    ",
                session.world_id
            )
            .fetch_one(&db)
            .await?;

            ResponseBuilder::default()
                .add_key("serverversion")
                .add_val(SERVER_VERSION)
                .add_key("preregister")
                .add_val(now())
                .add_val(now())
                .add_str("Europe")
                .add_str("Berlin")
                .add_key("timestamp")
                .add_val(now())
                .add_key("rankmaxplayer")
                .add_val(res.charactercount)
                .add_key("rankmaxgroup")
                .add_val(res.guildcount)
                .add_key("country")
                .add_val("DE")
                .build()
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
                Command::AddWorld { world_name } => {
                    sqlx::query!(
                        "INSERT INTO world (ident) VALUES ($1)", world_name
                    )
                    .execute(&db)
                    .await?;
                    return Ok(ServerResponse::Success);
                }
                _ if session.player_id == -1 => {
                    return Err(ServerError::BadRequest);
                }
                Command::Level { level } => {
                    if level < 1 {
                        return Err(ServerError::BadRequest);
                    }
                    sqlx::query!(
                        "UPDATE character set level = $1, experience = 0 \
                         WHERE pid = $2",
                        level,
                        session.player_id
                    )
                    .execute(&db)
                    .await?;
                }
                Command::Class { class } => {
                    Class::from_i16(class - 1).get("command class")?;
                    sqlx::query!(
                        "UPDATE character set class = $1 WHERE pid = $2",
                        class, session.player_id,
                    )
                    .execute(&db)
                    .await?;
                }
                Command::SetPassword { new } => {
                    let hashed_password =
                        sha1_hash(&format!("{new}{HASH_CONST}"));
                    let mut tx = db.begin().await?;
                    sqlx::query!(
                        "UPDATE character as c
                        SET pw_hash = $1
                        WHERE pid = $2",
                        hashed_password,
                        session.player_id
                    )
                    .execute(&mut *tx)
                    .await?;
                }
            }
            Ok(character_poll(session, "", &db, Default::default()).await?)
        }
        "AccountCheck" => {
            let name = args.get_str(0, "name")?;

            if is_invalid_name(name) {
                return Err(ServerError::InvalidName)?;
            }

            let count = sqlx::query_scalar!(
                "SELECT COUNT(*) FROM CHARACTER WHERE name = $1", name
            )
            .fetch_one(&db)
            .await?;

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

async fn character_set_face(
    command_args: &CommandArguments<'_>,
    db: &sqlx::Pool<Sqlite>,
    pid: i64,
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
        pid,
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
        pid
    )
    .execute(&mut *tx)
    .await?;

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

async fn character_poll(
    session: Session,
    tracking: &str,
    db: &sqlx::Pool<Sqlite>,
    mut builder: ResponseBuilder,
) -> Result<ServerResponse, ServerError> {
    let resp = builder
        .add_key("serverversion")
        .add_val(SERVER_VERSION)
        .add_key("preregister")
        .add_val(0) // TODO: This has values
        .add_val(0)
        .skip_key();

    let char = sqlx::query!(
        "SELECT
        character.pid, --0
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
        activity.sub_type as activitysubtyp,
        activity.busy_until,

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

        character.mount_end,
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
        tavern.Beer_Drunk,

        Tutorial_Status,

        tavern.Dice_Game_Next_Free,
        tavern.Dice_Games_Remaining,

        character.mushrooms,
        character.silver,
        tavern.QuickSand, -- 50

        description,
        character.name,

        portrait.influencer,

        (
        SELECT count(*)
        FROM CHARACTER AS x
        WHERE x.world_id = character.world_id
          AND (x.honor > character.honor
               OR (x.honor = character.honor
                   AND x.pid <= character.pid))
        )  as `rank!: i64`,
        (
        SELECT count(*)
        FROM CHARACTER AS x
        WHERE x.world_id = character.world_id
        )  as `maxrank!: i64`

        FROM CHARACTER
         NATURAL JOIN activity
         NATURAL JOIN tavern
         NATURAL JOIN portrait
         JOIN quest as q1 on tavern.quest1 = q1.id
         JOIN quest as q2 on tavern.quest2 = q2.id
         JOIN quest as q3 on tavern.quest2 = q3.id
         WHERE character.pid = $1",
        session.player_id
    )
    .fetch_one(db)
    .await?;

    let calendar_info = "12/1/8/1/3/1/25/1/5/1/2/1/3/2/1/1/24/1/18/5/6/1/22/1/\
                         7/1/6/2/8/2/22/2/5/2/2/2/3/3/21/1";

    resp.add_key("messagelist.r");
    resp.add_str(";");

    resp.add_key("combatloglist.s");
    resp.add_str(";");

    resp.add_key("friendlist.r");
    resp.add_str(";");

    resp.add_key("login count");
    resp.add_val(session.login_count);

    resp.skip_key();

    resp.add_key("sessionid");
    resp.add_str(&session.session_id);

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
    resp.add_val(session.player_id);
    resp.add_val(0);
    resp.add_val(1708336503);
    resp.add_val(1292388336);
    resp.add_val(0);
    resp.add_val(0);
    let level = char.level;
    resp.add_val(level); // Level | Arena << 16
    resp.add_val(char.experience); // Experience
    resp.add_val(xp_for_next_level(level)); // Next Level XP
    let honor = char.honor;
    resp.add_val(honor); // Honor
    let rank = char.rank;
    resp.add_val(rank); // Rank

    resp.add_val(0); // 12?
    resp.add_val(10); // 13?
    resp.add_val(0); // 14?
    resp.add_val(char.mushrooms); // Mushroms gained
    resp.add_val(0); // 16?

    // Portrait start
    resp.add_val(char.mouth); // mouth
    resp.add_val(char.hair); // hair
    resp.add_val(char.brows); // brows
    resp.add_val(char.eyes); // eyes
    resp.add_val(char.beards); // beards
    resp.add_val(char.nose); // nose
    resp.add_val(char.ears); // ears
    resp.add_val(char.extra); // extra
    resp.add_val(char.horns); // horns
    resp.add_val(char.influencer); // influencer
    resp.add_val(char.race); // race
    resp.add_val(char.gender); // Gender & Mirror
    resp.add_val(char.class); // class

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

    resp.add_val(char.activitytyp); // Current action
    resp.add_val(char.activitysubtyp); // Secondary (time busy)
    resp.add_val(char.busy_until); // Busy until

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
    resp.add_val(char.q1f1); // 229 Quest1 Flavour1
    resp.add_val(char.q2f1); // 230 Quest2 Flavour1
    resp.add_val(char.q3f1); // 231 Quest3 Flavour1

    resp.add_val(char.q1f2); // 233 Quest2 Flavour2
    resp.add_val(char.q2f2); // 232 Quest1 Flavour2
    resp.add_val(char.q3f2); // 234 Quest3 Flavour2

    resp.add_val(-char.q1monster); // 235 quest 1 monster
    resp.add_val(-char.q2monster); // 236 quest 2 monster
    resp.add_val(-char.q3monster); // 237 quest 3 monster

    resp.add_val(char.q1location); // 238 quest 1 location
    resp.add_val(char.q2location); // 239 quest 2 location
    resp.add_val(char.q3location); // 240 quest 3 location

    let mut mount_end = char.mount_end;
    let mut mount = char.mount;

    let mount_effect = effective_mount(&mut mount_end, &mut mount);

    resp.add_val((char.q1length as f32 * mount_effect) as i32); // 241 quest 1 length
    resp.add_val((char.q2length as f32 * mount_effect) as i32); // 242 quest 2 length
    resp.add_val((char.q3length as f32 * mount_effect) as i32); // 243 quest 3 length

    // Quest 1..=3 items
    for _ in 0..3 {
        for _ in 0..12 {
            resp.add_val(0); // 244..=279
        }
    }

    resp.add_val(char.q1xp); // 280 quest 1 xp
    resp.add_val(char.q2xp); // 281 quest 2 xp
    resp.add_val(char.q3xp); // 282 quest 3 xp

    resp.add_val(char.q1silver); // 283 quest 1 silver
    resp.add_val(char.q2silver); // 284 quest 2 silver
    resp.add_val(char.q3silver); // 285 quest 3 silver

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
    resp.add_val(0); // 445 character_hp_bonus << 24, damage_bonus << 16
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
    resp.add_val(char.tfa); // 456 Alu secs
    resp.add_val(char.beer_drunk); // 457 Beer drunk
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
    resp.add_val(char.tutorial_status); // 597 Pretty sure this is a bit map of which messages have been seen
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
    resp.add_val(char.dice_game_next_free); // 650 dice_games_next_free
    resp.add_val(char.dice_games_remaining); // 651 dice_games_remaining
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
    resp.add_val(session.player_id); // pid
    resp.add_val(char.mushrooms); // mushrooms
    resp.add_val(char.silver); // silver
    resp.add_val(0); // lucky coins
    resp.add_val(char.quicksand); // quicksand glasses
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
    resp.add_str(&to_sf_string(&char.description));

    resp.add_key("ownplayername.r");
    resp.add_str(&char.name);

    let maxrank = char.maxrank;

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
    resp.add_val(session.crypto_id);

    resp.add_key("cryptokey");
    resp.add_val(session.crypto_key);

    // resp.add_key("pendingrewards");
    // for i in 0..10 {
    //     resp.add_val(9999 + i);
    //     resp.add_val(2);
    //     resp.add_val(i);
    //     resp.add_val("Reward Name");
    //     resp.add_val(1717777586);
    //     resp.add_val(1718382386);
    // }

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

fn effective_mount(mount_end: &mut i64, mount: &mut i64) -> f32 {
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
    AddWorld {
        world_name: String,
    },
}

fn xp_for_next_level(level: i64) -> i64 {
    static LOOKUP: [i64; 392] = [
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

#[allow(unused)]
fn get_debug_value(name: &str) -> i64 {
    std::fs::read_to_string(format!("values/{name}.txt"))
        .ok()
        .and_then(|a| a.trim().parse().ok())
        .unwrap_or(0)
}

fn decrypt_server_request(to_decrypt: &str, key: &str) -> String {
    let text = base64::engine::general_purpose::URL_SAFE
        .decode(to_decrypt)
        .unwrap();

    let mut my_key = [0; 16];
    my_key.copy_from_slice(&key.as_bytes()[..16]);

    let mut cipher = libaes::Cipher::new_128(&my_key);
    cipher.set_auto_padding(false);
    const CRYPTO_IV: &str = "jXT#/vz]3]5X7Jl\\";
    let decrypted = cipher.cbc_decrypt(CRYPTO_IV.as_bytes(), &text);

    String::from_utf8(decrypted).unwrap()
}
