use std::time::{SystemTime, UNIX_EPOCH};

use account::{account_check, account_create, account_delete, account_login};
use guild::group_get_hof;
use log::{debug, error, warn};
use player::*;
use sqlx::Sqlite;
use update::poll;

use crate::{request::Session, response::*, SERVER_VERSION};

mod account;
mod debug;
mod guild;
mod item;
mod player;
mod update;

#[derive(Debug)]
pub struct CommandArguments<'a>(pub Vec<&'a str>);

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

pub(crate) async fn handle_command<'a>(
    db: &sqlx::Pool<Sqlite>,
    name: &'a str,
    args: CommandArguments<'a>,
    session: Session,
) -> Result<ServerResponse, ServerError> {
    if name != "Poll" {
        debug!("Received: {name}: {args:?}");
    }

    if !session.can_request(name) {
        // TODO: Validate provided session id
        warn!("{name} requires auth");
        Err(ServerError::InvalidAuth)?;
    }

    match name {
        "PlayerTwitchAuthtoken" => Ok(ServerResponse::Success),
        "AccountCheck" => account_check(db, args).await,
        "AccountCreate" => account_create(session, db, args).await,
        "AccountDelete" => account_delete(session, db, args).await,
        "AccountLogin" => account_login(session, db, args).await,
        "AccountSetLanguage" => Ok(ServerResponse::Success), // TODO:
        "GroupGetHallOfFame" => group_get_hof(session, db, args).await,
        "PendingRewardView" => pending_reward_view(session, db, args).await,
        "PlayerAdventureFinished" => player_finish_quest(session, db).await,
        "PlayerAdventureStart" => player_start_quest(session, db, args).await,
        "PlayerArenaEnemy" => poll(session, "", db, Default::default()).await,
        "PlayerArenaFight" => player_arena_fight(session, db, args).await,
        "PlayerLookAt" => player_look_at(session, db, args).await,
        "PlayerGambleGold" => player_gamble_gold(session, db, args).await,
        "PlayerGetHallOfFame" => player_get_hof(session, db, args).await,
        "PlayerHelpshiftAuthtoken" => player_helpshift_auth_token(),
        "PlayerMountBuy" => player_mount_buy(session, db, args).await,
        "PlayerPollScrapbook" => Ok(ServerResponse::Success), // TODO:
        "PlayerSetDescription" => player_set_descr(session, db, args).await,
        "PlayerSetFace" => player_set_face(session, db, args).await,
        "PlayerTutorialStatus" => player_tutorial(session, db, args).await,
        "PlayerWhisper" => player_whisper(session, db, args).await,
        "Poll" => poll(session, "poll", db, Default::default()).await,
        "UserSettingsUpdate" => Ok(ServerResponse::Success), // TODO:
        "getserverversion" => get_server_version(session, db).await,
        _ => {
            error!("Unknown command: {name} - {args:?}");
            Err(ServerError::UnknownRequest(name.into()))
        }
    }
}

async fn pending_reward_view(
    _session: Session,
    _db: &sqlx::Pool<Sqlite>,
    args: CommandArguments<'_>,
) -> Result<ServerResponse, ServerError> {
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

fn player_helpshift_auth_token() -> Result<ServerResponse, ServerError> {
    ResponseBuilder::default()
        .add_key("helpshiftauthtoken")
        .add_val("+eZGNZyCPfOiaufZXr/WpzaaCNHEKMmcT7GRJOGWJAU=")
        .build()
}

async fn get_server_version(
    session: Session,
    db: &sqlx::Pool<Sqlite>,
) -> Result<ServerResponse, ServerError> {
    let res = sqlx::query!(
        "SELECT
                (SELECT COUNT(*) FROM Character WHERE world_id = $1) as \
         `charactercount!: i64`,
                    (SELECT COUNT(*) FROM Guild WHERE world_id = $1) as \
         `guildcount!: i64`
                    ",
        session.world_id
    )
    .fetch_one(db)
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

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time warp")
        .as_secs() as i64
}

fn in_seconds(secs: i64) -> i64 {
    now() + secs
}

#[allow(unused)]
fn get_debug_value(name: &str) -> i64 {
    std::fs::read_to_string(format!("values/{name}.txt"))
        .ok()
        .and_then(|a| a.trim().parse().ok())
        .unwrap_or(0)
}

#[allow(unused)]
fn get_debug_value_default(name: &str, default: i64) -> i64 {
    std::fs::read_to_string(format!("values/{name}.txt"))
        .ok()
        .and_then(|a| a.trim().parse().ok())
        .unwrap_or(default)
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

pub(crate) fn xp_for_next_level(level: i64) -> i64 {
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
