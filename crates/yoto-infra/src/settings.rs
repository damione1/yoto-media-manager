//! Interface settings of the desktop app, kept in the profile next to the library.
//!
//! A small JSON file, separate from the encrypted catalogue: it holds no personal data,
//! is read before the window opens and must never stop the app from starting. A missing,
//! unreadable or older file reads as the defaults.
use crate::storage;
use std::path::Path;
use yoto_domain::Result;

/// The file name inside the profile directory.
pub const FILE: &str = "settings.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Interface language chosen by the user, as a code (`"fr"`, `"en"`). `None` follows
    /// the operating system's language.
    pub language: Option<String>,
    /// « Mode avancé »: show the technical settings.
    pub advanced_mode: bool,
}

impl Settings {
    /// The settings stored in `profile`, or the defaults when there are none yet or the
    /// file cannot be read.
    pub fn load(profile: &Path) -> Self {
        std::fs::read(profile.join(FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Store the settings in `profile`, replacing the previous file atomically so a crash
    /// never leaves a half-written file behind.
    pub fn save(&self, profile: &Path) -> Result<()> {
        std::fs::create_dir_all(profile).map_err(storage)?;
        let bytes = serde_json::to_vec_pretty(self).map_err(storage)?;
        let mut temp = tempfile::NamedTempFile::new_in(profile).map_err(storage)?;
        std::io::Write::write_all(&mut temp, &bytes).map_err(storage)?;
        temp.as_file().sync_all().map_err(storage)?;
        temp.persist(profile.join(FILE))
            .map_err(|e| storage(e.error))?;
        crate::assets::private(&profile.join(FILE))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_profile_reads_as_defaults() {
        let profile = tempfile::tempdir().unwrap();
        assert_eq!(Settings::load(profile.path()), Settings::default());
        assert_eq!(Settings::default().language, None);
        assert!(!Settings::default().advanced_mode);
    }

    #[test]
    fn saved_settings_are_read_back() {
        let profile = tempfile::tempdir().unwrap();
        let settings = Settings {
            language: Some("en".into()),
            advanced_mode: true,
        };
        settings.save(profile.path()).unwrap();
        assert_eq!(Settings::load(profile.path()), settings);
        let changed = Settings {
            language: Some("fr".into()),
            advanced_mode: false,
        };
        changed.save(profile.path()).unwrap();
        assert_eq!(Settings::load(profile.path()), changed);
    }

    #[test]
    fn a_damaged_or_partial_file_never_blocks_the_app() {
        let profile = tempfile::tempdir().unwrap();
        std::fs::write(profile.path().join(FILE), b"{not json").unwrap();
        assert_eq!(Settings::load(profile.path()), Settings::default());
        std::fs::write(profile.path().join(FILE), br#"{"advanced_mode":true}"#).unwrap();
        assert_eq!(
            Settings::load(profile.path()),
            Settings {
                language: None,
                advanced_mode: true
            }
        );
        std::fs::write(
            profile.path().join(FILE),
            br#"{"language":"en","future_setting":3}"#,
        )
        .unwrap();
        assert_eq!(
            Settings::load(profile.path()).language.as_deref(),
            Some("en")
        );
    }
}
