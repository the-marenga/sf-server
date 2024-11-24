use std::{net::SocketAddr, path::PathBuf};

use axum::{
    extract::Host,
    http::{Method, Uri},
    response::Redirect,
    routing::get,
};
use log::{debug, info, warn};
use request::{handle_cmd, handle_req};
use sqlx::{sqlite::SqlitePoolOptions, Sqlite};

use crate::response::*;

pub mod command;
pub mod frontend;
pub mod misc;
pub mod request;
pub mod response;

#[tokio::main]
async fn main() {
    // initialize tracing
    tracing_subscriber::fmt::init();
    let cors = tower_http::cors::CorsLayer::new()
        .allow_headers(tower_http::cors::Any)
        .allow_methods([Method::GET, Method::POST])
        .allow_origin(tower_http::cors::Any);

    let app = axum::Router::new()
        .route("/cmd.php", get(handle_cmd))
        .route("/req.php", get(handle_req))
        .route("/*key", get(frontend::forward))
        .route("/", get(frontend::forward))
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
        info!("listening on https://{}", addr);
        axum_server::bind_rustls(addr, config)
            .serve(app.into_make_service())
            .await
            .unwrap()
    }
}

// TODO: Make thise config, or env variables
static PROVIDE_HTTPS: bool = true;
static HTTP_PORT: u16 = 6767;
static HTTPS_PORT: u16 = 6768;

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
                Err(reqwest::StatusCode::BAD_REQUEST)
            }
        }
    };

    let addr = SocketAddr::from(([127, 0, 0, 1], HTTP_PORT));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    debug!("listening on {}", listener.local_addr().unwrap());
    use axum::handler::HandlerWithoutStateExt;
    axum::serve(listener, redirect.into_make_service())
        .await
        .unwrap();
}

pub const DEFAULT_CRYPTO_ID: &str = "0-00000000000000";
pub const DEFAULT_SESSION_ID: &str = "00000000000000000000000000000000";
pub const DEFAULT_CRYPTO_KEY: &str = "[_/$VV&*Qg&)r?~g";
pub const SERVER_VERSION: u32 = 2008;

pub async fn get_db() -> Result<sqlx::Pool<Sqlite>, ServerError> {
    use async_once_cell::OnceCell;
    static DB: OnceCell<sqlx::Pool<Sqlite>> = OnceCell::new();
    DB.get_or_try_init(async {
        SqlitePoolOptions::new()
            .max_connections(50)
            .connect(env!("DATABASE_URL"))
            .await
            .map_err(|a| a.into())
    })
    .await
    .cloned()
}
