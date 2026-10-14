//! The application version, fixed at build time from `YOTO_APP_VERSION`.
//!
//! Release builds receive the tag's `X.Y.Z`; any other build is `nightly`. This file has
//! no dependencies so `build.rs` can include it and reject an invalid value before
//! compiling.

/// The version string of a build made without `YOTO_APP_VERSION`.
pub const NIGHTLY: &str = "nightly";

/// A strict `X.Y.Z` release number: ASCII digits without leading zeros, no pre-release
/// or build suffix, no surrounding whitespace.
pub fn parse_release(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.split('.');
    let mut next = || {
        let part = parts.next()?;
        if part.is_empty()
            || !part.bytes().all(|byte| byte.is_ascii_digit())
            || (part.len() > 1 && part.starts_with('0'))
        {
            return None;
        }
        part.parse().ok()
    };
    let version = (next()?, next()?, next()?);
    if parts.next().is_some() {
        return None;
    }
    Some(version)
}
