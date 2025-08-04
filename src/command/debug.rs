use clap::{Parser, Subcommand};
use num_traits::FromPrimitive;
use sf_api::{
    gamestate::character::Class,
    misc::{HASH_CONST, sha1_hash},
};
use sqlx::Sqlite;

use super::{ServerError, ServerResponse, update::poll};
use crate::{misc::OptionGet, request::Session};

#[derive(Debug, Parser)]
#[command(about, version, no_binary_name(true))]
pub struct CheatCmd {
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

pub(crate) async fn handle_cheat_command(
    session: Session,
    db: &sqlx::Pool<Sqlite>,
    command: CheatCmd,
) -> Result<ServerResponse, ServerError> {
    match command.command {
        Command::AddWorld { world_name } => {
            sqlx::query!("INSERT INTO world (ident) VALUES ($1)", world_name)
                .execute(db)
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
                "UPDATE character set level = $1, experience = 0 WHERE pid = \
                 $2",
                level,
                session.player_id
            )
            .execute(db)
            .await?;
        }
        Command::Class { class } => {
            Class::from_i16(class - 1).get("command class")?;
            sqlx::query!(
                "UPDATE character set class = $1 WHERE pid = $2", class,
                session.player_id,
            )
            .execute(db)
            .await?;
        }
        Command::SetPassword { new } => {
            let hashed_password = sha1_hash(&format!("{new}{HASH_CONST}"));
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
    poll(session, "", db, Default::default()).await
}
