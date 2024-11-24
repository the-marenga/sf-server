use num_derive::FromPrimitive;
use serde::{Deserialize, Serialize};
use sf_api::gamestate::items::Enchantment;

use super::ResponseBuilder;

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

pub fn add_debug_item(resp: &mut ResponseBuilder, name: impl AsRef<str>) {
    let path = format!("items/{}.json", name.as_ref());
    let Some(item) = std::fs::read_to_string(&path)
        .ok()
        .and_then(|a| serde_json::from_str::<RawItem>(&a).ok())
    else {
        for _ in 0..12 {
            resp.add_val(0);
        }
        return;
    };

    let mut ident = item.item_typ as i64;
    ident |= item.enchantment.map(|a| a as i64).unwrap_or_default() << 24;
    ident |= item.gem_val << 16;
    resp.add_val(ident);

    let mut sub_ident = item.sub_ident.map(|a| a as i64).unwrap_or_default();
    sub_ident |= item.class.map(|a| a as i64 * 1000).unwrap_or_default();
    sub_ident |= item.modelid as i64;
    resp.add_val(sub_ident);

    resp.add_val(item.effect_1 as i64);
    resp.add_val(item.effect_2 as i64);

    match &item.atrs {
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

    resp.add_val(item.silver as i64);
    resp.add_val(item.mushrooms as i64 | (item.gem_pwr as i64) << 16);
}
