//! ABOUTME: Objects that keep the members this version does not know, exactly as written.
//! ABOUTME: Members are read from raw spans, so a number of any length and a sentinel-looking key both survive a round trip.

use std::collections::BTreeMap;
use std::mem;

use serde::Deserialize;
use serde::de::{self, DeserializeOwned, Deserializer};
use serde::ser::{self, SerializeMap};
use serde_json::value::RawValue;

use super::{Field, JsonValue};

/// The members of an object that no field of its type names.
///
/// They are kept as written and written back after the known members. A member whose name is a known field cannot be
/// written as an extra, nor one named like the tag of a tagged object: serialising such an object fails instead of
/// writing the name twice.
#[derive(Debug, Clone, Default)]
pub struct Extra(BTreeMap<String, JsonValue>);

impl Extra {
    pub fn get(&self, name: &str) -> Option<&JsonValue> {
        self.0.get(name)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &JsonValue)> {
        self.0.iter().map(|(name, value)| (name.as_str(), value))
    }

    /// Adds a member, replacing and returning an earlier extra of that name.
    pub fn insert(&mut self, name: impl Into<String>, value: JsonValue) -> Option<JsonValue> {
        self.0.insert(name.into(), value)
    }

    pub(crate) fn write<M: SerializeMap>(
        &self,
        map: &mut M,
        known: &[&str],
        reserved: &[&str],
    ) -> Result<(), M::Error> {
        for (name, value) in &self.0 {
            if known.contains(&name.as_str()) || reserved.contains(&name.as_str()) {
                return Err(ser::Error::custom("an extra member repeats a member already written"));
            }
            map.serialize_entry(name, value)?;
        }
        Ok(())
    }
}

/// The raw members of one object, taken one by one into typed fields.
///
/// Reads from the document's own deserializer, never from a buffered copy, so no number is converted on the way.
/// Duplicate names are refused by [`decode`](super::decode), not here.
pub(crate) struct Members(BTreeMap<String, Box<RawValue>>);

impl Members {
    pub(crate) fn read<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        BTreeMap::<String, Box<RawValue>>::deserialize(deserializer).map(Self)
    }

    /// A member's value, read without taking it, to choose a variant before the rest are read. Reading by value, not by
    /// spelling, means `"pr\u006fbe"` is `"probe"`.
    pub(crate) fn peek<T: DeserializeOwned>(&self, name: &str) -> Result<Option<T>, serde_json::Error> {
        self.0.get(name).map(|raw| serde_json::from_str(raw.get())).transpose()
    }

    /// A member that must be present. `Option<T>` makes it required but nullable.
    pub(crate) fn req<T: DeserializeOwned>(&mut self, name: &'static str) -> Result<T, serde_json::Error> {
        let raw = self.0.remove(name).ok_or_else(|| de::Error::missing_field(name))?;
        serde_json::from_str(raw.get())
    }

    /// A member that may be missing; present with `null` is `Present(None)` when `T` is an `Option`.
    pub(crate) fn opt<T: DeserializeOwned>(&mut self, name: &'static str) -> Result<Field<T>, serde_json::Error> {
        match self.0.remove(name) {
            None => Ok(Field::Missing),
            Some(raw) => serde_json::from_str(raw.get()).map(Field::Present),
        }
    }

    pub(crate) fn rest(&mut self) -> Extra {
        Extra(
            mem::take(&mut self.0)
                .into_iter()
                .map(|(name, raw)| (name, JsonValue(raw)))
                .collect(),
        )
    }
}

macro_rules! take {
    (req, $members:ident, $name:expr) => {
        $members.req($name)?
    };
    (opt, $members:ident, $name:expr) => {
        $members.opt($name)?
    };
}

macro_rules! put {
    (req, $map:ident, $name:expr, $value:expr) => {
        $map.serialize_entry($name, $value)?
    };
    (opt, $map:ident, $name:expr, $value:expr) => {
        if let $crate::contract::Field::Present(value) = $value {
            $map.serialize_entry($name, value)?;
        }
    };
}

/// Declares a struct that reads and writes a JSON object: each field is `req` (must be present; `Option<T>` is then
/// required but nullable) or `opt` (a [`Field`], so missing, `null` and a value stay distinct), and every other member
/// is kept in `extra`.
macro_rules! open_object {
    ($(#[$meta:meta])* $vis:vis struct $name:ident {
        $($(#[$fmeta:meta])* $field:ident : $ty:ty = $kind:ident,)*
    }) => {
        $(#[$meta])*
        #[derive(Debug, Clone)]
        $vis struct $name {
            $($(#[$fmeta])* pub $field: $ty,)*
            /// Members this version does not know, kept as written.
            pub extra: $crate::contract::Extra,
        }

        impl $name {
            pub(crate) const KNOWN: &'static [&'static str] = &[$(stringify!($field)),*];

            pub(crate) fn from_members(
                members: &mut $crate::contract::open::Members,
            ) -> Result<Self, serde_json::Error> {
                Ok(Self {
                    $($field: $crate::contract::open::take!($kind, members, stringify!($field)),)*
                    extra: members.rest(),
                })
            }

            /// Writes every member. `reserved` are names an enclosing object already wrote, which no extra may repeat.
            pub(crate) fn write_fields<M: serde::ser::SerializeMap>(
                &self,
                map: &mut M,
                reserved: &[&str],
            ) -> Result<(), M::Error> {
                $($crate::contract::open::put!($kind, map, stringify!($field), &self.$field);)*
                self.extra.write(map, Self::KNOWN, reserved)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let mut members = $crate::contract::open::Members::read(deserializer)?;
                Self::from_members(&mut members).map_err(serde::de::Error::custom)
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                use serde::ser::SerializeMap;
                let mut map = serializer.serialize_map(None)?;
                self.write_fields(&mut map, &[])?;
                map.end()
            }
        }
    };
}

/// Declares an enum read from one object, by the string member `$tag`; each variant is an `open_object` without that
/// member. An unknown tag is refused without repeating it.
macro_rules! tagged_object {
    ($(#[$meta:meta])* $vis:vis enum $name:ident tag $tag:literal {
        $($(#[$vmeta:meta])* $variant:ident($ty:ty) = $wire:literal,)*
    }) => {
        $(#[$meta])*
        #[derive(Debug, Clone)]
        $vis enum $name {
            $($(#[$vmeta])* $variant($ty),)*
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let mut members = $crate::contract::open::Members::read(deserializer)?;
                let tag: String = members.req($tag).map_err(serde::de::Error::custom)?;
                match tag.as_str() {
                    $($wire => <$ty>::from_members(&mut members).map(Self::$variant),)*
                    _ => Err(serde::de::Error::custom("an unknown variant")),
                }
                .map_err(serde::de::Error::custom)
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                use serde::ser::SerializeMap;
                let mut map = serializer.serialize_map(None)?;
                match self {
                    $(Self::$variant(value) => {
                        map.serialize_entry($tag, $wire)?;
                        value.write_fields(&mut map, &[$tag])?;
                    })*
                }
                map.end()
            }
        }
    };
}

pub(crate) use {open_object, put, tagged_object, take};
