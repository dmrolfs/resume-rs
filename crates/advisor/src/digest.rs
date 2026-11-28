//! ABOUTME: The SHA-256 identity of exact file bytes, as lowercase hexadecimal text.
//! ABOUTME: Approvals and fixtures bind to the bytes read, never to a re-serialised value.

use std::fmt;
use std::str::FromStr;

use serde::de::{self, Deserialize, Deserializer};
use serde::{Serialize, Serializer};
use sha2::{Digest, Sha256};
use snafu::Snafu;

/// A SHA-256 of raw bytes. Equal JSON with different whitespace, member order or escapes is a different digest, which
/// is the point: an approval names one file, not one meaning.
///
/// Building one from text checks its shape only. It is not an approval check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Sha256Digest([u8; 32]);

/// Why a string is not a digest: it must be exactly 64 lowercase hexadecimal digits, as Python's `hexdigest()` writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Snafu)]
#[snafu(display("a SHA-256 digest is exactly 64 lowercase hexadecimal digits"))]
pub struct DigestError;

impl Sha256Digest {
    pub fn of(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }
}

impl fmt::Display for Sha256Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}

impl FromStr for Sha256Digest {
    type Err = DigestError;

    fn from_str(text: &str) -> Result<Self, DigestError> {
        let hex = |digit: u8| match digit {
            b'0'..=b'9' => Some(digit - b'0'),
            b'a'..=b'f' => Some(digit - b'a' + 10),
            _ => None,
        };
        let digits = text.as_bytes();
        if digits.len() != 64 {
            return Err(DigestError);
        }
        let mut bytes = [0u8; 32];
        for (byte, pair) in bytes.iter_mut().zip(digits.as_chunks::<2>().0) {
            *byte = hex(pair[0]).ok_or(DigestError)? * 16 + hex(pair[1]).ok_or(DigestError)?;
        }
        Ok(Self(bytes))
    }
}

impl Serialize for Sha256Digest {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Sha256Digest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(|_| de::Error::custom(DigestError))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok};
    use pretty_assertions::assert_eq;

    #[test]
    fn known_vectors_hash_exact_bytes() {
        assert_eq!(
            Sha256Digest::of(b"").to_string(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            Sha256Digest::of(b"abc").to_string(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn equal_json_with_other_bytes_has_another_identity() {
        let compact = Sha256Digest::of(br#"{"a":1,"b":"\u0061"}"#);
        for other in [
            &br#"{ "a": 1, "b": "a" }"#[..],
            br#"{"b":"a","a":1}"#,
            b"{\"a\":1,\"b\":\"a\"}\n",
        ] {
            assert_ne!(compact, Sha256Digest::of(other));
        }
    }

    #[test]
    fn only_sixty_four_lowercase_hex_digits_parse() {
        let good = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let digest = assert_ok!(good.parse::<Sha256Digest>());
        assert_eq!(digest.to_string(), good);
        for bad in [
            "",
            &good[1..],
            &format!("{good}0"),
            &good.to_uppercase(),
            &good.replace('b', "g"),
            &format!(" {}", &good[1..]),
        ] {
            assert_err!(bad.parse::<Sha256Digest>(), "{bad}");
        }
    }

    #[test]
    fn wire_form_is_the_lowercase_string_and_nothing_else() {
        let good = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let digest: Sha256Digest = assert_ok!(serde_json::from_str(&format!("\"{good}\"")));
        assert_eq!(assert_ok!(serde_json::to_string(&digest)), format!("\"{good}\""));
        for bad in ["null", "1", &format!("\"{}\"", good.to_uppercase()), "\"abc\""] {
            assert_err!(serde_json::from_str::<Sha256Digest>(bad), "{bad}");
        }
    }
}
