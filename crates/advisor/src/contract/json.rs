//! ABOUTME: Strict JSON decoding for the legacy artifacts: no duplicate names, no inexact numbers, no trailing data.
//! ABOUTME: A structural pass walks the whole document first; only then is it decoded into the expected type.

use std::collections::HashSet;
use std::fmt;

use serde::de::{self, DeserializeOwned, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::value::RawValue;
use snafu::{ResultExt, Snafu};

/// How many containers may nest. It is serde_json's own limit (127 open containers), applied here to every raw span
/// the walk re-reads, because each re-read would otherwise start its own budget.
const MAX_DEPTH: usize = 127;

/// Why a document is not accepted. `Display` states the category only; the source can carry the text of the document
/// (a field name, a value) and must not be printed from a command.
#[derive(Debug, Snafu)]
pub enum DecodeError {
    #[snafu(display("the document is not valid UTF-8"))]
    NotUtf8 { source: std::str::Utf8Error },
    #[snafu(display("the document is not valid, strictly decodable JSON"))]
    Structure { source: serde_json::Error },
    #[snafu(display("the document has data after its first JSON value"))]
    Trailing { source: serde_json::Error },
    #[snafu(display("the document does not have the expected shape"))]
    Shape { source: serde_json::Error },
}

/// Decodes `bytes`, the exact content of one artifact file, into `T`.
///
/// Refused before `T` is looked at: a repeated member name at any depth (compared after unescaping), a decimal that is
/// out of range for an `f64` (`1e999`), a lone surrogate, invalid UTF-8, nesting beyond 127 containers, and anything after the first
/// value. An integer of any length passes, because it is never converted: only [`JsonValue`] keeps a value exactly,
/// and a typed field that cannot hold it (a `u64`) fails in the decode that follows.
///
/// This says nothing about whether the content is valid or approved. Never decode into `serde_json::Value`: its
/// reader takes an object whose first key is `$serde_json::private::RawValue` for a raw value.
///
/// # Errors
/// See [`DecodeError`].
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, DecodeError> {
    let text = std::str::from_utf8(bytes).context(NotUtf8Snafu)?;
    let mut reader = serde_json::Deserializer::from_str(text);
    let root = <&RawValue>::deserialize(&mut reader).context(StructureSnafu)?;
    reader.end().context(TrailingSnafu)?;
    validate(root, 0).context(StructureSnafu)?;
    serde_json::from_str(text).context(ShapeSnafu)
}

/// Checks one value and everything under it, reading each child from its own raw span so that no number is converted.
fn validate(raw: &RawValue, depth: usize) -> Result<(), serde_json::Error> {
    let text = raw.get();
    match text.as_bytes().first() {
        Some(b'{') => walk(text, depth, |reader, depth| reader.deserialize_map(Members { depth })),
        Some(b'[') => walk(text, depth, |reader, depth| reader.deserialize_seq(Members { depth })),
        Some(b'"') => serde_json::from_str::<String>(text).map(drop),
        Some(b't' | b'f' | b'n') => Ok(()),
        // A decimal or exponent is read as an f64 only to refuse one that is out of range (`1e999`): serde_json reports
        // that as an error, so an `f64` that comes back is finite. The value is dropped; an integer is never converted.
        _ if text.contains(['.', 'e', 'E']) => serde_json::from_str::<f64>(text).map(drop),
        _ => Ok(()),
    }
}

/// Re-reads the container `text` one level down.
fn walk(
    text: &str,
    depth: usize,
    visit: impl FnOnce(&mut serde_json::Deserializer<serde_json::de::StrRead<'_>>, usize) -> Result<(), serde_json::Error>,
) -> Result<(), serde_json::Error> {
    if depth >= MAX_DEPTH {
        return Err(de::Error::custom("nested too deeply"));
    }
    let mut reader = serde_json::Deserializer::from_str(text);
    visit(&mut reader, depth + 1)?;
    reader.end()
}

/// Visits the members or elements of one container and validates each from its raw span.
struct Members {
    depth: usize,
}

impl<'de> Visitor<'de> for Members {
    type Value = ();

    // Never shown: `validate` calls this visitor only on text whose first byte is `{` or `[`.
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON object or array")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        while let Some(element) = seq.next_element::<&RawValue>()? {
            validate(element, self.depth).map_err(de::Error::custom)?;
        }
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let mut seen = HashSet::new();
        while let Some(name) = map.next_key::<String>()? {
            if !seen.insert(name) {
                return Err(de::Error::custom("a repeated member name"));
            }
            validate(map.next_value::<&RawValue>()?, self.depth).map_err(de::Error::custom)?;
        }
        Ok(())
    }
}

/// A JSON value kept exactly as written: digits, spelling and key order survive, and a number of any length is not
/// converted. Use it for provider data this crate stores but does not read, such as `usage`.
///
/// Its text is checked by the same walk as a whole document. It compares by text, not by meaning, so no `PartialEq`.
#[derive(Debug, Clone)]
pub struct JsonValue(pub(super) Box<RawValue>);

impl JsonValue {
    /// Checks `json` and keeps it.
    ///
    /// # Errors
    /// See [`DecodeError`].
    pub fn parse(json: &str) -> Result<Self, DecodeError> {
        decode(json.as_bytes())
    }

    /// The value as written, with no surrounding whitespace.
    pub fn as_json(&self) -> &str {
        self.0.get()
    }
}

impl<'de> Deserialize<'de> for JsonValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Box::<RawValue>::deserialize(deserializer)?;
        validate(&raw, 0).map_err(de::Error::custom)?;
        Ok(Self(raw))
    }
}

impl Serialize for JsonValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

/// A member that may be missing, as opposed to present with any value, including `null`.
///
/// Use `Field<Option<T>>` where the wire has three states (missing, `null`, a value) and `#[serde(default,
/// skip_serializing_if = "Field::is_missing")]` on the member. Plain `Option<Option<T>>` cannot tell the first two
/// apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Field<T> {
    #[default]
    Missing,
    Present(T),
}

impl<T> Field<T> {
    pub const fn is_missing(&self) -> bool {
        matches!(self, Self::Missing)
    }

    pub const fn as_present(&self) -> Option<&T> {
        match self {
            Self::Present(value) => Some(value),
            Self::Missing => None,
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Field<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::deserialize(deserializer).map(Self::Present)
    }
}

impl<T: Serialize> Serialize for Field<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Present(value) => value.serialize(serializer),
            Self::Missing => Err(serde::ser::Error::custom(
                "a missing field has no value; skip it with `skip_serializing_if`",
            )),
        }
    }
}
