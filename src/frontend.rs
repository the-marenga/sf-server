use std::collections::HashMap;

use axum::{extract::Request, response::*};
use log::error;
use once_cell::sync::OnceCell;
use reqwest::{header::CONTENT_TYPE, Client, StatusCode};
use serde::{Deserialize, Serialize};

/// In order to provide the S&F interface without actually hosting and thus
/// infringing on their copyrighted material, we just forward requests to our
/// server to theirs. Since CORS is annoying, we must manually fetch some of
/// the config stuff. In addition, we must modify some of them, to fix
/// issues around the otherwise invalid server domain
pub async fn forward(req: Request) -> Result<Response, StatusCode> {
    let uri = req
        .uri()
        .path_and_query()
        .ok_or(StatusCode::NOT_FOUND)?
        .as_str();

    let sfgame_url = || format!("https://sfgame.net{uri}");
    let playa_cdn_url = || format!("https://cdn.playa-games.com{uri}");

    const INTERNAL_ERR: StatusCode = StatusCode::INTERNAL_SERVER_ERROR;

    if [".webp", ".png", ".jpg"]
        .into_iter()
        .any(|a| uri.ends_with(a))
    {
        // They CORS allow any origin for images, but block some other stuff
        return Ok(Redirect::temporary(&sfgame_url()).into_response());
    }

    let client = get_client();
    let get_text = |url: String| async {
        let resp = client.get(url).send().await.map_err(|e| {
            error!("Error while sending request: {:?}", e);
            INTERNAL_ERR
        })?;
        let status = resp.status();
        if !status.is_success() {
            return Err(status);
        }
        resp.text().await.map_err(|e| {
            error!("Error while reading response text: {:?}", e);
            INTERNAL_ERR
        })
    };

    if uri == "/js/build.json" {
        let text = get_text(sfgame_url()).await?;
        let mut json: HashMap<String, String> = serde_json::from_str(&text)
            .map_err(|e| {
                error!("Error while parsing JSON: {:?}", e);
                INTERNAL_ERR
            })?;
        let fw = json.get_mut("frameworkUrl").ok_or(INTERNAL_ERR)?;
        *fw = fw.split_once(".com").ok_or(INTERNAL_ERR)?.1.into();
        Ok(axum::Json(json).into_response())
    } else if uri.ends_with(".framework.js.gz") {
        let fixed_framework = get_text(playa_cdn_url())
            .await?
            .replacen("return hasConsentForVendor(vendorID)", "return true", 1);
        Response::builder()
            .status(200)
            .header(CONTENT_TYPE, "application/javascript")
            .body(axum::body::Body::from(fixed_framework))
            .map_err(|e| {
                error!("Error while building response body: {:?}", e);
                INTERNAL_ERR
            })
    } else if uri == "/config.json" {
        // Rewrite the server list to our server(s)
        let text = get_text(sfgame_url()).await?;
        let mut config: SFConfig =
            serde_json::from_str(&text).map_err(|e| {
                error!("Error while parsing config JSON: {:?}", e);
                INTERNAL_ERR
            })?;

        config.servers.clear();

        let db = crate::get_db().await.map_err(|_| INTERNAL_ERR)?;

        let servers = sqlx::query!("SELECT * FROM world")
            .fetch_all(&db)
            .await
            .map_err(|e| {
                error!("Database query error: {:?}", e);
                INTERNAL_ERR
            })?;

        for server in servers {
            let server_host = req
                .uri()
                .authority()
                .map(|a| a.as_str())
                .ok_or(INTERNAL_ERR)?;
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
        }
        Ok(axum::Json(config).into_response())
    } else {
        // Just forward whatever the server sends us
        let Ok(resp) = client.get(sfgame_url()).send().await else {
            return Err(INTERNAL_ERR);
        };
        let mut builder = Response::builder().status(resp.status());
        for (key, value) in resp.headers().iter() {
            builder = builder.header(key, value);
        }
        // We to this stream thing here in case there are large assets, that
        // would obliterate any simple "write to memory, then send out" logic
        builder
            .body(axum::body::Body::from_stream(resp.bytes_stream()))
            .map_err(|e| {
                error!("Error while building response body: {:?}", e);
                INTERNAL_ERR
            })
    }
}

/// Using one client allows it to reuse some connection stuff, instead of
/// doing a new TCP handshake or whatever for every request. Read the reqwest
/// docs for more info
pub fn get_client() -> Client {
    static CLIENT: OnceCell<Client> = OnceCell::new();
    CLIENT.get_or_init(Client::new).clone()
}

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
