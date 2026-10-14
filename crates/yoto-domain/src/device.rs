use crate::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Device {
    pub id: DeviceId,
    pub name: String,
    pub online: bool,
    pub firmware: Option<String>,
    pub product: Option<String>,
    pub extra: Extra,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum PlayerCommand {
    Volume {
        value: Volume,
    },
    Ambient {
        r: u8,
        g: u8,
        b: u8,
    },
    SleepTimer {
        seconds: u32,
    },
    Reboot,
    Start {
        card: CardId,
        chapter: Option<ChapterKey>,
        track: Option<TrackKey>,
        seconds: u32,
    },
    Stop,
    Pause,
    Resume,
    BluetoothOn,
    BluetoothOff,
    BluetoothDeleteBond {
        address: String,
    },
    BluetoothConnect {
        address: String,
    },
    BluetoothDisconnect,
    BluetoothState,
    DisplayPreview {
        icon: YotoMediaId,
    },
    Events,
    Status,
}
impl PlayerCommand {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Volume { .. } => "Régler le volume",
            Self::Ambient { .. } => "Lumière",
            Self::SleepTimer { .. } => "Minuterie",
            Self::Reboot => "Redémarrer",
            Self::Start { .. } => "Lire sur le lecteur",
            Self::Stop => "Arrêter",
            Self::Pause => "Pause",
            Self::Resume => "Reprendre",
            Self::BluetoothOn => "Activer Bluetooth",
            Self::BluetoothOff => "Désactiver Bluetooth",
            Self::BluetoothDeleteBond { .. } => "Oublier Bluetooth",
            Self::BluetoothConnect { .. } => "Connecter Bluetooth",
            Self::BluetoothDisconnect => "Déconnecter Bluetooth",
            Self::BluetoothState => "État Bluetooth",
            Self::DisplayPreview { .. } => "Aperçu de l’icône",
            Self::Events => "Actualiser la lecture",
            Self::Status => "Actualiser l’état",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeviceConfiguration {
    pub name: String,
    #[serde(rename = "dayTime")]
    pub day_time: String,
    #[serde(rename = "nightTime")]
    pub night_time: String,
    #[serde(rename = "maxVolumeLimit")]
    pub max_volume_limit: String,
    #[serde(rename = "nightMaxVolumeLimit")]
    pub night_max_volume_limit: String,
    #[serde(rename = "ambientColour")]
    pub ambient_colour: String,
    #[serde(rename = "nightAmbientColour")]
    pub night_ambient_colour: String,
    #[serde(rename = "displayDimTimeout")]
    pub display_dim_timeout: String,
    #[serde(rename = "shutdownTimeout")]
    pub shutdown_timeout: String,
    #[serde(rename = "repeatAll")]
    pub repeat_all: bool,
    #[serde(rename = "btHeadphonesEnabled")]
    pub bt_headphones_enabled: bool,
    #[serde(rename = "headphonesVolumeLimited")]
    pub headphones_volume_limited: bool,
    /// Optional open value: omitted fields are preserved; no undocumented value catalogue is imposed.
    #[serde(default, rename = "clockFace")]
    pub clock_face: Option<String>,
    /// Documented writable open value; omitted leaves the current setting intact.
    #[serde(default, rename = "volumeLevel")]
    pub volume_level: Option<String>,
    /// Public PUT documents "1" as enabled; other server values remain open strings.
    #[serde(default, rename = "bluetoothEnabled")]
    pub bluetooth_enabled: Option<String>,
    /// Optional open value: omitted fields are preserved; no undocumented value catalogue is imposed.
    #[serde(default, rename = "locale")]
    pub locale: Option<String>,
    /// Optional open value: omitted fields are preserved; no undocumented value catalogue is imposed.
    #[serde(default, rename = "hourFormat")]
    pub hour_format: Option<String>,
    /// Optional open value: omitted fields are preserved; no undocumented value catalogue is imposed.
    #[serde(default, rename = "dayDisplayBrightness")]
    pub day_display_brightness: Option<String>,
    /// Optional open value: omitted fields are preserved; no undocumented value catalogue is imposed.
    #[serde(default, rename = "nightDisplayBrightness")]
    pub night_display_brightness: Option<String>,
    /// Optional open value: omitted fields are preserved; no undocumented value catalogue is imposed.
    #[serde(default, rename = "displayDimBrightness")]
    pub display_dim_brightness: Option<String>,
    /// Optional open value: omitted fields are preserved; no undocumented value catalogue is imposed.
    #[serde(default, rename = "dayYotoDaily")]
    pub day_yoto_daily: Option<String>,
    /// Optional open value: omitted fields are preserved; no undocumented value catalogue is imposed.
    #[serde(default, rename = "nightYotoDaily")]
    pub night_yoto_daily: Option<String>,
    /// Optional open value: omitted fields are preserved; no undocumented value catalogue is imposed.
    #[serde(default, rename = "dayYotoRadio")]
    pub day_yoto_radio: Option<String>,
    /// Optional open value: omitted fields are preserved; no undocumented value catalogue is imposed.
    #[serde(default, rename = "nightYotoRadio")]
    pub night_yoto_radio: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Shortcut {
    pub card: CardId,
    pub chapter: ChapterKey,
    pub track: TrackKey,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
pub enum FamilyImageSize {
    Square,
    Landscape,
}
impl FamilyImageSize {
    pub fn dimensions(self) -> (i32, i32) {
        match self {
            Self::Square => (320, 320),
            Self::Landscape => (640, 480),
        }
    }
}

impl DeviceConfiguration {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(Failure::Validation(
                crate::messages::DEVICE_NAME_EMPTY.into(),
            ));
        }
        for time in [&self.day_time, &self.night_time] {
            if !time.split_once(':').is_some_and(|(h, m)| {
                h.len() == 2
                    && m.len() == 2
                    && h.parse::<u8>().is_ok_and(|h| h < 24)
                    && m.parse::<u8>().is_ok_and(|m| m < 60)
            }) {
                return Err(Failure::Validation(crate::messages::TIME_FORMAT.into()));
            }
        }
        for color in [&self.ambient_colour, &self.night_ambient_colour] {
            if !color
                .strip_prefix('#')
                .is_some_and(|s| s.len() == 6 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            {
                return Err(Failure::Validation(crate::messages::COLOUR_FORMAT.into()));
            }
        }
        for number in [
            &self.max_volume_limit,
            &self.night_max_volume_limit,
            &self.display_dim_timeout,
            &self.shutdown_timeout,
        ] {
            if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
                return Err(Failure::Validation(
                    crate::messages::LIMITS_NONNEGATIVE.into(),
                ));
            }
        }
        Ok(())
    }
}
