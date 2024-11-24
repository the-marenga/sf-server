use command::{poll, CommandArguments, Portrait};
use fastrand::Rng;
use num_traits::FromPrimitive;
use request::Session;
use sf_api::gamestate::character::{Class, Gender, Race};
use sqlx::Sqlite;

use crate::{
    misc::{sha1_hash, OptionGet, HASH_CONST},
    *,
};

pub(crate) async fn account_check(
    db: &sqlx::Pool<Sqlite>,
    args: CommandArguments<'_>,
) -> Result<ServerResponse, ServerError> {
    let name = args.get_str(0, "name")?;

    if is_invalid_name(name) {
        return Err(ServerError::InvalidName)?;
    }

    let count = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM CHARACTER WHERE name = $1", name
    )
    .fetch_one(db)
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

pub(crate) async fn account_create(
    session: Session,
    db: &sqlx::Pool<Sqlite>,
    args: CommandArguments<'_>,
) -> Result<ServerResponse, ServerError> {
    let mut rng = Rng::new();
    let name = args.get_str(0, "name")?;
    let password = args.get_str(1, "password")?;
    let mail = args.get_str(2, "mail")?;
    let gender = args.get_int(3, "gender")?;
    Gender::from_i64(gender.saturating_sub(1)).get("gender")?;
    let race = args.get_int(4, "race")?;
    Race::from_i64(race).get("race")?;

    let class = args.get_int(5, "class")?;
    let _class = Class::from_i64(class.saturating_sub(1)).get("class")?;

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
            "INSERT INTO QUEST (monster, location, length, xp, silver, \
             mushrooms)
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
        "INSERT INTO PORTRAIT (Mouth, Hair, Brows, Eyes, Beards, Nose, Ears, \
         Horns, extra, pid) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
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
        "INSERT INTO character (pid, world_id, pw_hash, name, class, race, \
         gender, attributes, attributes_bought, mail, crypto_key)
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
        "INSERT INTO SESSION (pid, session_id, crypto_id) VALUES ($1, $2, $3)",
        pid, session_id, crypto_id,
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    ResponseBuilder::default()
        .add_key("tracking.s")
        .add_str("signup")
        .build()
}

pub(crate) async fn account_delete(
    _session: Session,
    db: &sqlx::Pool<Sqlite>,
    args: CommandArguments<'_>,
) -> Result<ServerResponse, ServerError> {
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
    let correct_full_hash = sha1_hash(&format!("{}{login_count}", pwhash));
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

pub(crate) async fn account_login(
    mut session: Session,
    db: &sqlx::Pool<Sqlite>,
    args: CommandArguments<'_>,
) -> Result<ServerResponse, ServerError> {
    let mut rng = Rng::new();
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

    let correct_full_hash = sha1_hash(&format!("{}{login_count}", pwhash));
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

    session.crypto_id = crypto_id;
    session.crypto_key = info.crypto_key;
    session.session_id = session_id;
    session.player_id = pid;
    session.login_count = 1;

    poll(session, "accountlogin", db, Default::default()).await
}

fn is_invalid_name(name: &str) -> bool {
    name.len() < 3
        || name.len() > 20
        || name.starts_with(' ')
        || name.ends_with(' ')
        || name.chars().all(|a| a.is_ascii_digit())
        || name.chars().any(|a| !(a.is_alphanumeric() || a == ' '))
}
