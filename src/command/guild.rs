use std::fmt::Write;

use sqlx::Sqlite;

use super::CommandArguments;
use crate::{ResponseBuilder, ServerError, ServerResponse, request::Session};

pub(crate) async fn group_get_hof(
    session: Session,
    db: &sqlx::Pool<Sqlite>,
    args: CommandArguments<'_>,
) -> Result<ServerResponse, ServerError> {
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
            (SELECT count(*) AS membercount FROM guild_member as gm WHERE \
         gm.guild_id = g.id) as `membercount!: i64`,
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
    .fetch_all(db)
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
        .start_section("ranklistgroup.r")
        .add_str(&guilds)
        .build()
}
