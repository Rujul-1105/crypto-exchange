//! Stream identifiers + helpers shared across binaries.

use std::fmt;

/// A Redis Stream ID (e.g. "1693-0"). We don't parse it — just store and pass
/// through as a string. Strongly typed to avoid mixing up with random strings.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct StreamId(pub String);

impl StreamId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StreamId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for StreamId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for StreamId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl serde::Serialize for StreamId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for StreamId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self(s))
    }
}
