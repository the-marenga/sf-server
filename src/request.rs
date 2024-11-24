use std::collections::HashMap;

use axum::{extract::Query, response::Response};
use base64::Engine;
use log::error;

use crate::{
    command::{handle_command, CommandArguments},
    get_db,
    misc::OptionGet,
    ServerError, DEFAULT_CRYPTO_ID, DEFAULT_CRYPTO_KEY, DEFAULT_SESSION_ID,
};

pub async fn handle_cmd(
    req_params: Query<HashMap<String, String>>,
) -> Result<Response, Response> {
    let db = get_db().await?;
    let command_name = req_params.get("req").get("request")?.as_str();
    let crypto_id = req_params.get("sid").get("crypto_id")?;
    let command_args = req_params.get("params").get("command_args")?;
    let command_args = base64::engine::general_purpose::URL_SAFE
        .decode(command_args)
        .map_err(|e| {
            error!("Error while decoding command_args: {:?}", e);
            ServerError::BadRequest
        })?;
    let command_args = String::from_utf8(command_args).map_err(|e| {
        error!("Error while converting command_args to UTF-8: {:?}", e);
        ServerError::BadRequest
    })?;

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
            .map_err(|e| {
                error!("Database error while fetching world_id: {:?}", e);
                ServerError::DBError(e)
            })?;

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
            .map_err(|e| {
                error!("Database error while fetching session: {:?}", e);
                ServerError::DBError(e)
            })?;

            let Some(row) = res else {
                return Err(ServerError::InvalidAuth.into());
            };
            Session {
                player_id: row.pid,
                world_id: row.world_id,
                session_id: row.session_id,
                crypto_id: row.crypto_id,
                crypto_key: row.crypto_key,
                login_count: row.login_count,
            }
        }
    };
    let args = CommandArguments(command_args.split('/').collect());

    handle_command(&db, command_name, args, session)
        .await
        .map_err(|a| a.into())
        .map(|a| a.into())
}

pub async fn handle_req(
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
            .map_err(|e| {
                error!("Database error while fetching world_id: {:?}", e);
                ServerError::DBError(e)
            })?;

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
            .map_err(|e| {
                error!("Database error while fetching session: {:?}", e);
                ServerError::DBError(e)
            })?;

            let Some(row) = res else {
                return Err(ServerError::InvalidAuth.into());
            };
            Session {
                player_id: row.pid,
                world_id: row.world_id,
                session_id: row.session_id,
                crypto_id: row.crypto_id,
                crypto_key: row.crypto_key,
                login_count: row.login_count,
            }
        }
    };

    let request =
        decrypt_server_request(encrypted_request, &session.crypto_key)?;

    let Some((_session_id, request)) = request.split_once('|') else {
        return Err(ServerError::BadRequest.into());
    };

    let request = request.trim_matches('|');

    let Some((command_name, command_args)) = request.split_once(':') else {
        return Err(ServerError::BadRequest.into());
    };
    let args = CommandArguments(command_args.split('/').collect());

    handle_command(&db, command_name, args, session)
        .await
        .map_err(|a| a.into())
        .map(|a| a.into())
}

fn decrypt_server_request(
    to_decrypt: &str,
    key: &str,
) -> Result<String, ServerError> {
    let text = base64::engine::general_purpose::URL_SAFE
        .decode(to_decrypt)
        .map_err(|e| {
            error!("Error while decoding to_decrypt: {:?}", e);
            ServerError::BadRequest
        })?;

    let mut my_key = [0; 16];
    my_key.copy_from_slice(&key.as_bytes()[..16]);

    let mut cipher = libaes::Cipher::new_128(&my_key);
    cipher.set_auto_padding(false);
    const CRYPTO_IV: &str = "jXT#/vz]3]5X7Jl\\";
    let decrypted = cipher.cbc_decrypt(CRYPTO_IV.as_bytes(), &text);

    String::from_utf8(decrypted).map_err(|e| {
        error!("Error while converting decrypted text to UTF-8: {:?}", e);
        ServerError::BadRequest
    })
}

#[derive(Debug)]
pub struct Session {
    pub player_id: i64,
    pub world_id: i64,
    pub session_id: String,
    pub crypto_id: String,
    pub crypto_key: String,
    pub login_count: i64,
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
