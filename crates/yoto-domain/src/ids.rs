use crate::{Failure, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
macro_rules! local_id {
    ($($name:ident),*) => {$ (
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
        #[serde(transparent)]
        pub struct $name(Uuid);
        // Allocating an identity is an explicit operation, never an implicit default.
        #[allow(clippy::new_without_default)]
        impl $name { pub fn new() -> Self { Self(Uuid::new_v4()) } }
        impl std::fmt::Display for $name { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { self.0.fmt(f) } }
    )*};
}
local_id!(
    MediaId, AssetId, RecipeId, PlaylistId, ChapterId, TrackId, JobId, LibraryId
);
macro_rules! remote_id {
    ($($name:ident),*) => {$ (
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);
        impl $name { pub fn new(value: impl Into<String>) -> Result<Self> { let value = value.into(); if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) { return Err(Failure::Validation(concat!(stringify!($name), " is invalid").into())); } Ok(Self(value)) } pub fn as_str(&self) -> &str { &self.0 } }
        impl TryFrom<String> for $name { type Error = Failure; fn try_from(s: String) -> Result<Self> { Self::new(s) } }
        impl From<$name> for String { fn from(v: $name) -> Self {v.0} }
        impl std::fmt::Display for $name { fn fmt(&self,f:&mut std::fmt::Formatter<'_>) -> std::fmt::Result {self.0.fmt(f)} }
    )*};
}
remote_id!(
    AccountId,
    CardId,
    DeviceId,
    UploadId,
    YotoMediaId,
    ChapterKey,
    TrackKey,
    GroupId,
    IconId,
    FamilyImageId
);

/// Yoto rejects chapter and track keys longer than this many characters.
pub const WIRE_KEY_MAX: usize = 20;
/// The form Yoto accepts for a wire key: unchanged when short enough, otherwise its first
/// [`WIRE_KEY_MAX`] characters, so a long key always shortens to the same value.
pub fn fit_wire_key(key: &str) -> &str {
    key.char_indices()
        .nth(WIRE_KEY_MAX)
        .map_or(key, |(end, _)| &key[..end])
}
fn fresh_wire_key() -> String {
    Uuid::new_v4().simple().to_string()[..16].to_owned()
}
impl ChapterKey {
    /// A random key unique within its playlist, short enough for Yoto.
    pub fn fresh() -> Self {
        Self(fresh_wire_key())
    }
}
impl TrackKey {
    /// A random key unique within its chapter, short enough for Yoto.
    pub fn fresh() -> Self {
        Self(fresh_wire_key())
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(try_from = "u64", into = "u64")]
/// Time in integer microseconds (1 second = 1_000_000). Nonnegative and bounded.
pub struct MediaTime(u64);
impl MediaTime {
    pub const ZERO: Self = Self(0);
    pub fn micros(self) -> u64 {
        self.0
    }
    pub fn seconds(self) -> f64 {
        self.0 as f64 / 1_000_000.0
    }
    pub fn from_seconds(s: f64) -> Result<Self> {
        if !s.is_finite() || !(0.0..=86_400_000.0).contains(&s) {
            return Err(Failure::Validation("Invalid audio duration".into()));
        }
        Self::try_from((s * 1_000_000.0).round() as u64)
    }
}
impl TryFrom<u64> for MediaTime {
    type Error = Failure;
    fn try_from(n: u64) -> Result<Self> {
        if n > 86_400_000_000_000 {
            return Err(Failure::Validation("Duration overflow".into()));
        }
        Ok(Self(n))
    }
}
impl From<MediaTime> for u64 {
    fn from(t: MediaTime) -> Self {
        t.0
    }
}
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct Revision(pub u64);
impl Revision {
    pub fn next(self) -> Result<Self> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or_else(|| Failure::Validation("Revision overflow".into()))
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "u8", into = "u8")]
/// Player volume as an integer from 0 through 100 inclusive.
pub struct Volume(u8);
impl Volume {
    pub fn new(v: u8) -> Result<Self> {
        if v > 100 {
            return Err(Failure::Validation(crate::messages::VOLUME_RANGE.into()));
        }
        Ok(Self(v))
    }
    pub fn value(self) -> u8 {
        self.0
    }
}
impl TryFrom<u8> for Volume {
    type Error = Failure;
    fn try_from(v: u8) -> Result<Self> {
        Self::new(v)
    }
}
impl From<Volume> for u8 {
    fn from(v: Volume) -> u8 {
        v.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "String", into = "String")]
pub struct LocalSha256(String);
impl LocalSha256 {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Failure::Validation(
                "SHA256 requires 64 hexadecimal characters".into(),
            ));
        }
        Ok(Self(value.to_ascii_lowercase()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for LocalSha256 {
    type Error = Failure;
    fn try_from(v: String) -> Result<Self> {
        Self::new(v)
    }
}
impl From<LocalSha256> for String {
    fn from(v: LocalSha256) -> Self {
        v.0
    }
}
impl std::ops::Deref for LocalSha256 {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}
impl std::fmt::Display for LocalSha256 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
