use aho_corasick::AhoCorasick;
use once_cell::sync::Lazy;

use crate::ServerError;

/// Converts a  s&f string from the server to their original unescaped
/// representation
pub(crate) fn from_sf_string(val: &str) -> String {
    pattern_replace::<true>(val)
}

/// Makes a user controlled string, like the character description safe to use
/// in a request
pub(crate) fn to_sf_string(val: &str) -> String {
    pattern_replace::<false>(val)
}

/// Calling .replace() a bunch of times is bad, as that generates a bunch of
/// strings. regex!() -> replace_all()  would be better, as that uses cow<>
/// irrc, but we can replace pattern with a linear search on one string, using
/// this extra crate. We call this function a bunch, so optimizing this is
/// probably worth it
fn pattern_replace<const FROM: bool>(str: &str) -> String {
    static A: Lazy<(AhoCorasick, &'static [&'static str; 11])> =
        Lazy::new(|| {
            let l = sf_str_lookups();
            (AhoCorasick::new(l.0).unwrap(), l.1)
        });

    static B: Lazy<(AhoCorasick, &'static [&'static str; 11])> =
        Lazy::new(|| {
            let l = sf_str_lookups();
            (AhoCorasick::new(l.1).unwrap(), l.0)
        });

    let (from, to) = match FROM {
        true => A.clone(),
        false => B.clone(),
    };
    let mut wtr = vec![];
    from.try_stream_replace_all(str.as_bytes(), &mut wtr, to)
        .expect("stream_replace_all failed");

    String::from_utf8(wtr).unwrap_or_default()
}

/// The mappings to convert between a normal and a sf string
const fn sf_str_lookups(
) -> (&'static [&'static str; 11], &'static [&'static str; 11]) {
    (
        &[
            "$b", "$c", "$P", "$s", "$p", "$+", "$q", "$r", "$C", "$S", "$d",
        ],
        &["\n", ":", "%", "/", "|", "&", "\"", "#", ",", ";", "$"],
    )
}

pub trait OptionGet<V> {
    fn get(self, name: &'static str) -> Result<V, ServerError>;
}

impl<T> OptionGet<T> for Option<T> {
    fn get(self, name: &'static str) -> Result<T, ServerError> {
        self.ok_or_else(|| ServerError::MissingArgument(name))
    }
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
