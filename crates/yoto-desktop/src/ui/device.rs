use super::*;
use gpui_kit::component::{
    button::{ButtonCustomVariant, ButtonRounded},
    checkbox::Checkbox,
    menu::{DropdownMenu, PopupMenuItem},
    slider::{Slider, SliderEvent, SliderState},
};
use theme::{Height, Look};
/// Night-light colors offered as swatches: label, RGB.
const LIGHTS: [(&str, [u8; 3]); 6] = [
    ("Orange doux", [0xFF, 0xB0, 0x66]),
    ("Rose", [0xFF, 0x9E, 0xB5]),
    ("Violet", [0xB4, 0x9C, 0xFF]),
    ("Bleu", [0x7F, 0xB2, 0xFF]),
    ("Vert", [0x7E, 0xD9, 0xA6]),
    ("Éteinte", [0, 0, 0]),
];
/// Sleep timer choices: label, seconds (0 cancels the timer).
const TIMERS: [(&str, u32); 4] = [("Non", 0), ("15 min", 900), ("30 min", 1800), ("1 h", 3600)];
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Field {
    Name,
    Light,
    Day,
    Night,
    DayLimit,
    NightLimit,
    NightLight,
    Dim,
    Shutdown,
    DayShortcut,
    NightShortcut,
    VolumeLevel,
    BluetoothEnabled,
    ClockFace,
    Locale,
    HourFormat,
    DayBrightness,
    NightBrightness,
    DimBrightness,
    DayDaily,
    NightDaily,
    DayRadio,
    NightRadio,
}
impl Field {
    const ALL: &'static [Self] = &[
        Self::Name,
        Self::Light,
        Self::Day,
        Self::Night,
        Self::DayLimit,
        Self::NightLimit,
        Self::NightLight,
        Self::Dim,
        Self::Shutdown,
        Self::DayShortcut,
        Self::NightShortcut,
        Self::VolumeLevel,
        Self::BluetoothEnabled,
        Self::ClockFace,
        Self::Locale,
        Self::HourFormat,
        Self::DayBrightness,
        Self::NightBrightness,
        Self::DimBrightness,
        Self::DayDaily,
        Self::NightDaily,
        Self::DayRadio,
        Self::NightRadio,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::VolumeLevel => "Niveau de volume (valeur Yoto)",
            Self::BluetoothEnabled => "Bluetooth configuré (valeur Yoto)",
            Self::ClockFace => "Cadran (valeur Yoto)",
            Self::Locale => "Langue / locale",
            Self::HourFormat => "Format horaire (valeur Yoto)",
            Self::DayBrightness => "Luminosité jour (valeur Yoto)",
            Self::NightBrightness => "Luminosité nuit (valeur Yoto)",
            Self::DimBrightness => "Luminosité atténuée (valeur Yoto)",
            Self::DayDaily => "Yoto Daily jour",
            Self::NightDaily => "Yoto Daily nuit",
            Self::DayRadio => "Yoto Radio jour",
            Self::NightRadio => "Yoto Radio nuit",
            Self::Name => "Nom du lecteur",
            Self::Light => "Lumière #RRGGBB",
            Self::Day => "Début du jour HH:MM",
            Self::Night => "Début de la nuit HH:MM",
            Self::DayLimit => "Volume maximal jour",
            Self::NightLimit => "Volume maximal nuit",
            Self::NightLight => "Lumière de nuit #RRGGBB",
            Self::Dim => "Délai avant atténuation",
            Self::Shutdown => "Délai avant arrêt",
            Self::DayShortcut => "Raccourcis jour : carte/chapitre/piste, séparés par des virgules",
            Self::NightShortcut => {
                "Raccourcis nuit : carte/chapitre/piste, séparés par des virgules"
            }
        }
    }
    fn wire(self) -> Option<&'static str> {
        match self {
            Self::VolumeLevel => Some("volumeLevel"),
            Self::BluetoothEnabled => Some("bluetoothEnabled"),
            Self::ClockFace => Some("clockFace"),
            Self::Locale => Some("locale"),
            Self::HourFormat => Some("hourFormat"),
            Self::DayBrightness => Some("dayDisplayBrightness"),
            Self::NightBrightness => Some("nightDisplayBrightness"),
            Self::DimBrightness => Some("displayDimBrightness"),
            Self::DayDaily => Some("dayYotoDaily"),
            Self::NightDaily => Some("nightYotoDaily"),
            Self::DayRadio => Some("dayYotoRadio"),
            Self::NightRadio => Some("nightYotoRadio"),
            Self::Day => Some("dayTime"),
            Self::Night => Some("nightTime"),
            Self::DayLimit => Some("maxVolumeLimit"),
            Self::NightLimit => Some("nightMaxVolumeLimit"),
            Self::Light => Some("ambientColour"),
            Self::NightLight => Some("nightAmbientColour"),
            Self::Dim => Some("displayDimTimeout"),
            Self::Shutdown => Some("shutdownTimeout"),
            Self::Name | Self::DayShortcut | Self::NightShortcut => None,
        }
    }
}
pub struct DevicePanel {
    account: AccountId,
    device: DeviceId,
    service: Arc<Service>,
    runtime: Arc<tokio::runtime::Runtime>,
    inputs: BTreeMap<Field, Entity<InputState>>,
    repeat: bool,
    headphones: bool,
    limited: bool,
    loaded: bool,
    shortcut_base: Option<(String, String)>,
    name: String,
    volume: Entity<SliderState>,
    card: Option<CardId>,
    light: Option<usize>,
    timer: Option<u32>,
    night: bool,
    paused: bool,
    volume_set: bool,
    more_open: bool,
    loading: bool,
    tasks: Vec<Task<()>>,
    status: String,
    _subscriptions: Vec<Subscription>,
}
impl DevicePanel {
    pub fn new(
        account: AccountId,
        device: Device,
        service: Arc<Service>,
        runtime: Arc<tokio::runtime::Runtime>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let inputs = Field::ALL
            .iter()
            .map(|f| {
                (
                    *f,
                    cx.new(|cx| {
                        InputState::new(window, cx)
                            .placeholder(f.label())
                            .default_value(if *f == Field::Name {
                                device.name.clone()
                            } else {
                                String::new()
                            })
                    }),
                )
            })
            .collect();
        let volume = cx.new(|_| {
            SliderState::new()
                .min(0.)
                .max(100.)
                .step(1.)
                .default_value(50.)
        });
        let subscription = cx.subscribe(&volume, |v, _, event, cx| match event {
            SliderEvent::Release(value) => {
                let level = value.start().round().clamp(0., 100.) as u8;
                match Volume::new(level) {
                    Ok(value) => v.player(PlayerCommand::Volume { value }, cx),
                    Err(e) => {
                        v.status = failure_text(&e);
                        cx.notify();
                    }
                }
            }
            SliderEvent::Change(_) => {
                v.volume_set = true;
                cx.notify();
            }
        });
        Self {
            account,
            name: device.name.clone(),
            device: device.id,
            volume,
            card: None,
            light: None,
            timer: None,
            night: false,
            paused: false,
            volume_set: false,
            more_open: false,
            loading: false,
            _subscriptions: vec![subscription],
            service,
            runtime,
            inputs,
            repeat: false,
            headphones: false,
            limited: false,
            loaded: false,
            shortcut_base: None,
            tasks: vec![],
            status: String::new(),
        }
    }
    fn value(&self, f: Field, cx: &App) -> String {
        self.inputs[&f].read(cx).value().to_string()
    }
    fn send(&mut self, command: Command, cx: &mut Context<Self>) {
        let service = self.service.clone();
        let rt = self.runtime.clone();
        // Pause and resume change the button only once the player confirms them.
        let paused_after = if matches!(
            &command,
            Command::Player {
                command: PlayerCommand::Pause,
                ..
            }
        ) {
            Some(true)
        } else if matches!(
            &command,
            Command::Player {
                command: PlayerCommand::Resume,
                ..
            }
        ) {
            Some(false)
        } else {
            None
        };
        self.status = "Connexion au lecteur…".into();
        self.tasks.push(cx.spawn(async move |weak, cx| {
            let r = rt
                .spawn(async move { service.execute(command).await })
                .await;
            let _ = weak.update(cx, |v, cx| {
                v.loading = false;
                match r {
                    Ok(Ok(Reply::Data(data))) => {
                        if let Some(config) = data.pointer("/device/config") {
                            v.loaded = true;
                            let show = |mode: &str| -> String {
                                data.pointer(&format!("/device/shortcuts/modes/{mode}/content"))
                                    .and_then(serde_json::Value::as_array)
                                    .map(|items| {
                                        items
                                            .iter()
                                            .map(|item| {
                                                if item
                                                    .get("cmd")
                                                    .and_then(serde_json::Value::as_str)
                                                    != Some("track-play")
                                                {
                                                    return serde_json::to_string(item)
                                                        .unwrap_or_default();
                                                }
                                                let p = &item["params"];
                                                format!(
                                                    "{}/{}/{}",
                                                    p["card"].as_str().unwrap_or(""),
                                                    p["chapter"].as_str().unwrap_or(""),
                                                    p["track"].as_str().unwrap_or("")
                                                )
                                            })
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    })
                                    .unwrap_or_default()
                            };
                            let day = show("day");
                            let night = show("night");
                            v.shortcut_base = Some((day.clone(), night.clone()));
                            for (field, value) in
                                [(Field::DayShortcut, day), (Field::NightShortcut, night)]
                            {
                                let input = v.inputs[&field].clone();
                                if let Some(window) = cx.windows().first().copied() {
                                    let _ = cx.update_window(window, move |_, window, cx| {
                                        input.update(cx, |s, cx| s.set_value(value, window, cx))
                                    });
                                }
                            }
                            // Input projections are applied in the next window update, never in render.
                            for field in Field::ALL {
                                if let Some(key) = field.wire()
                                    && let Some(value) =
                                        config.get(key).and_then(serde_json::Value::as_str)
                                {
                                    let input = v.inputs[field].clone();
                                    let value = value.to_owned();
                                    if let Some(window) = cx.windows().first().copied() {
                                        let _ = cx.update_window(window, move |_, window, cx| {
                                            input.update(cx, |s, cx| s.set_value(value, window, cx))
                                        });
                                    }
                                }
                            }
                            v.repeat = config
                                .get("repeatAll")
                                .and_then(serde_json::Value::as_bool)
                                .unwrap_or(false);
                            v.headphones = config
                                .get("btHeadphonesEnabled")
                                .and_then(serde_json::Value::as_bool)
                                .unwrap_or(false);
                            v.limited = config
                                .get("headphonesVolumeLimited")
                                .and_then(serde_json::Value::as_bool)
                                .unwrap_or(false);
                        }
                        v.status = if v.loaded {
                            "Réglages du lecteur à jour.".into()
                        } else {
                            "État du lecteur reçu.".into()
                        };
                    }
                    Ok(Ok(Reply::Player(result))) => {
                        if let Some(paused) = paused_after
                            && matches!(
                                result,
                                yoto_infra::players::CommandResult::Accepted
                                    | yoto_infra::players::CommandResult::Observed { .. }
                            )
                        {
                            v.paused = paused;
                        }
                        v.status = match result {
                            yoto_infra::players::CommandResult::Sent => {
                                "Commande envoyée ; effet non confirmé.".into()
                            }
                            yoto_infra::players::CommandResult::Accepted => {
                                "Commande acceptée.".into()
                            }
                            yoto_infra::players::CommandResult::Observed { .. } => {
                                "Commande reçue par le lecteur.".into()
                            }
                        }
                    }
                    Ok(Ok(
                        Reply::AppUpdate(_)
                        | Reply::YoutubeTool(_)
                        | Reply::Updated
                        | Reply::Saved(_)
                        | Reply::OpenedPlaylist(_)
                        | Reply::PlaylistOperation { .. }
                        | Reply::Entries(_)
                        | Reply::Waveform(_)
                        | Reply::Prepared(_)
                        | Reply::Asset(_)
                        | Reply::PixelArt(_)
                        | Reply::Image(_)
                        | Reply::Images(_),
                    )) => v.status = "Réglages envoyés.".into(),
                    Ok(Err(e)) => v.status = failure_text(&e),
                    Err(e) => v.status = failure_text(&Failure::Storage(e.to_string())),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    /// Read the player's current volume so the slider starts where the player is. This
    /// needs a permission some accounts lack, so a failure just leaves the slider as it is.
    fn read_volume(&mut self, cx: &mut Context<Self>) {
        let service = self.service.clone();
        let rt = self.runtime.clone();
        let command = Command::DeviceStatus {
            account: self.account.clone(),
            device: self.device.clone(),
        };
        self.tasks.push(cx.spawn(async move |weak, cx| {
            let result = rt
                .spawn(async move { service.execute(command).await })
                .await;
            let Ok(Ok(Reply::Data(status))) = result else {
                return;
            };
            let Some(level) = status
                .get("userVolumePercentage")
                .and_then(serde_json::Value::as_f64)
                .filter(|level| (0. ..=100.).contains(level))
            else {
                return;
            };
            let _ = weak.update(cx, |v, cx| {
                if v.volume_set {
                    return;
                }
                v.volume_set = true;
                let slider = v.volume.clone();
                if let Some(window) = cx.windows().first().copied() {
                    let _ = cx.update_window(window, move |_, window, cx| {
                        slider.update(cx, |s, cx| s.set_value(level as f32, window, cx))
                    });
                }
                cx.notify();
            });
        }));
    }
    /// Read the player's settings from Yoto.
    pub fn load(&mut self, cx: &mut Context<Self>) {
        self.read_volume(cx);
        self.loading = true;
        self.send(
            Command::DeviceConfig {
                account: self.account.clone(),
                device: self.device.clone(),
            },
            cx,
        );
        self.status = "Lecture des réglages du lecteur…".into();
    }
    pub fn start(&mut self, card: CardId, cx: &mut Context<Self>) {
        self.paused = false;
        self.player(
            PlayerCommand::Start {
                card,
                chapter: None,
                track: None,
                seconds: 0,
            },
            cx,
        );
    }
    fn player(&mut self, command: PlayerCommand, cx: &mut Context<Self>) {
        self.send(
            Command::Player {
                account: self.account.clone(),
                device: self.device.clone(),
                command,
            },
            cx,
        );
    }
    fn save(&mut self, cx: &mut Context<Self>) {
        let patch = DeviceConfiguration {
            volume_level: (!self.value(Field::VolumeLevel, cx).is_empty())
                .then(|| self.value(Field::VolumeLevel, cx)),
            bluetooth_enabled: (!self.value(Field::BluetoothEnabled, cx).is_empty())
                .then(|| self.value(Field::BluetoothEnabled, cx)),
            clock_face: (!self.value(Field::ClockFace, cx).is_empty())
                .then(|| self.value(Field::ClockFace, cx)),
            locale: (!self.value(Field::Locale, cx).is_empty())
                .then(|| self.value(Field::Locale, cx)),
            hour_format: (!self.value(Field::HourFormat, cx).is_empty())
                .then(|| self.value(Field::HourFormat, cx)),
            day_display_brightness: (!self.value(Field::DayBrightness, cx).is_empty())
                .then(|| self.value(Field::DayBrightness, cx)),
            night_display_brightness: (!self.value(Field::NightBrightness, cx).is_empty())
                .then(|| self.value(Field::NightBrightness, cx)),
            display_dim_brightness: (!self.value(Field::DimBrightness, cx).is_empty())
                .then(|| self.value(Field::DimBrightness, cx)),
            day_yoto_daily: (!self.value(Field::DayDaily, cx).is_empty())
                .then(|| self.value(Field::DayDaily, cx)),
            night_yoto_daily: (!self.value(Field::NightDaily, cx).is_empty())
                .then(|| self.value(Field::NightDaily, cx)),
            day_yoto_radio: (!self.value(Field::DayRadio, cx).is_empty())
                .then(|| self.value(Field::DayRadio, cx)),
            night_yoto_radio: (!self.value(Field::NightRadio, cx).is_empty())
                .then(|| self.value(Field::NightRadio, cx)),
            name: self.value(Field::Name, cx),
            day_time: self.value(Field::Day, cx),
            night_time: self.value(Field::Night, cx),
            max_volume_limit: self.value(Field::DayLimit, cx),
            night_max_volume_limit: self.value(Field::NightLimit, cx),
            ambient_colour: self.value(Field::Light, cx),
            night_ambient_colour: self.value(Field::NightLight, cx),
            display_dim_timeout: self.value(Field::Dim, cx),
            shutdown_timeout: self.value(Field::Shutdown, cx),
            repeat_all: self.repeat,
            bt_headphones_enabled: self.headphones,
            headphones_volume_limited: self.limited,
        };
        if let Err(e) = patch.validate() {
            self.status = failure_text(&e);
            cx.notify();
            return;
        }
        self.send(
            Command::SaveDeviceConfig {
                account: self.account.clone(),
                device: self.device.clone(),
                patch: Box::new(patch),
            },
            cx,
        );
    }
    fn shortcuts(&mut self, cx: &mut Context<Self>) {
        let Some((base_day, base_night)) = &self.shortcut_base else {
            self.status = "Chargez les réglages avant de modifier les raccourcis.".into();
            return;
        };
        let day = self.value(Field::DayShortcut, cx);
        let night = self.value(Field::NightShortcut, cx);
        let parse = |value: &str, base: &str| {
            if value == base {
                Ok(None)
            } else {
                shortcuts(value).map(Some)
            }
        };
        let r = parse(&day, base_day).and_then(|day| Ok((day, parse(&night, base_night)?)));
        match r {
            Ok((day, night)) => self.send(
                Command::SaveShortcuts {
                    account: self.account.clone(),
                    device: self.device.clone(),
                    day,
                    night,
                },
                cx,
            ),
            Err(e) => {
                self.status = failure_text(&e);
                cx.notify();
            }
        }
    }
}
/// Shortcuts typed as « carte/chapitre/piste », separated by commas or line breaks.
fn shortcuts(s: &str) -> yoto_domain::Result<Vec<Shortcut>> {
    s.split([',', '\n'])
        .filter(|s| !s.trim().is_empty())
        .map(|line| {
            let parts: Vec<_> = line.trim().split('/').collect();
            if parts.len() != 3 {
                return Err(Failure::Validation(
                    "Raccourci attendu carte/chapitre/piste".into(),
                ));
            }
            Ok(Shortcut {
                card: CardId::new(parts[0])?,
                chapter: ChapterKey::new(parts[1])?,
                track: TrackKey::new(parts[2])?,
            })
        })
        .collect()
}
/// Small caption above a value or a field, as in the design.
fn caption(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(12.))
        .font_weight(FontWeight::EXTRA_BOLD)
        .text_color(theme::c(theme::MUTED))
        .child(text.into())
}
/// Section label inside the play card (« Volume », « Veilleuse », « Minuterie »).
fn label(text: impl Into<SharedString>) -> Div {
    div().font_weight(FontWeight::EXTRA_BOLD).child(text.into())
}
/// A light color as Yoto stores it: « #RRGGBB ».
fn light_hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}
/// The swatch color drawn for a light; « off » is shown dark rather than pure black.
fn swatch_color([r, g, b]: [u8; 3]) -> u32 {
    if (r, g, b) == (0, 0, 0) {
        theme::SWATCH_OFF
    } else {
        u32::from_be_bytes([0, r, g, b])
    }
}
/// Ring drawn around the chosen night-light swatch: white gap, then ink.
fn swatch_ring() -> Vec<BoxShadow> {
    [(theme::SURFACE, 3.), (theme::INK, 5.)]
        .into_iter()
        .map(|(color, spread)| BoxShadow {
            color: theme::c(color),
            offset: point(px(0.), px(0.)),
            blur_radius: px(0.),
            spread_radius: px(spread),
            inset: false,
        })
        .collect()
}
impl DevicePanel {
    fn stored(&self, field: Field) -> &Entity<InputState> {
        &self.inputs[&field]
    }
    /// Day or night light color as swatches. A color set elsewhere that matches none of
    /// them stays shown first, selected.
    fn colour_picker(&self, field: Field, cx: &mut Context<Self>) -> Div {
        let current = self.value(field, cx).trim().to_ascii_uppercase();
        let mut row = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .py(px(4.));
        let known = LIGHTS.iter().any(|(_, rgb)| light_hex(*rgb) == current);
        if !known
            && let Some(rgb) = current
                .strip_prefix('#')
                .filter(|hex| hex.len() == 6)
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
        {
            row = row.child(
                div()
                    .size(px(28.))
                    .rounded_full()
                    .bg(theme::c(rgb))
                    .shadow(swatch_ring()),
            );
        }
        for (index, (name, rgb)) in LIGHTS.iter().enumerate() {
            let hex = light_hex(*rgb);
            let shown = swatch_color(*rgb);
            let input = self.stored(field).clone();
            let selected = hex == current;
            let swatch = Button::new(element_id(&format!("{field:?}-light"), index))
                .custom(
                    ButtonCustomVariant::new(cx)
                        .color(theme::c(shown))
                        .hover(theme::c(shown))
                        .active(theme::c(shown)),
                )
                .accessibility_label(format!("Couleur de la lumière : {name}"))
                .tooltip(*name)
                .toggled(selected)
                .bg(theme::c(shown))
                .rounded(ButtonRounded::Size(px(999.)))
                .size(px(28.))
                .on_click(cx.listener(move |_, _, window, cx| {
                    input.update(cx, |s, cx| s.set_value(hex.clone(), window, cx));
                    cx.notify();
                }));
            row = row.child(if selected {
                swatch.shadow(swatch_ring())
            } else {
                swatch
            });
        }
        row
    }
    /// The « Lecture sur … » card: playlist, play/pause/stop, volume, night light, timer.
    pub fn play_card(
        &mut self,
        remote: &[yoto_infra::api::RemoteCard],
        online: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let owner = cx.entity().downgrade();
        let chosen = self
            .card
            .as_ref()
            .and_then(|id| remote.iter().find(|card| &card.card == id));
        let chooser = if remote.is_empty() {
            theme::inset().child(theme::note(
                "Aucune playlist sur votre compte Yoto pour l’instant. Envoyez-en une depuis « Playlists », puis actualisez.",
            ))
        } else {
            let cards: Vec<(CardId, String)> = remote
                .iter()
                .map(|card| (card.card.clone(), card.title.clone()))
                .collect();
            let selected = self.card.clone();
            let (thumb, title, subtitle) = match chosen {
                Some(card) => (
                    theme::thumbnail(card.card.as_str(), 52., 14.),
                    card.title.clone(),
                    "Playlist de votre compte Yoto",
                ),
                None => (
                    theme::icon_tile(IconName::ListMusic, &theme::LILAC, 52., 14.),
                    "Choisir une playlist".to_owned(),
                    "Elle sera lancée sur ce lecteur.",
                ),
            };
            div().child(
                Button::new("device-playlist")
                    .custom(
                        ButtonCustomVariant::new(cx)
                            .color(theme::c(theme::CREAM))
                            .foreground(theme::c(theme::INK))
                            .hover(theme::c(theme::SAND))
                            .active(theme::c(theme::SAND)),
                    )
                    .accessibility_label(format!("Playlist à lancer : {title}"))
                    .bg(theme::c(theme::CREAM))
                    .rounded(ButtonRounded::Size(px(16.)))
                    .w_full()
                    .h(px(72.))
                    .px(px(10.))
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(thumb)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .font_weight(FontWeight::EXTRA_BOLD)
                                            .truncate()
                                            .child(title),
                                    )
                                    .child(theme::meta(subtitle)),
                            )
                            .child(
                                gpui_kit::component::Icon::new(IconName::ChevronDown)
                                    .size(px(18.))
                                    .text_color(theme::c(theme::MUTED_WARM)),
                            ),
                    )
                    .dropdown_menu(move |mut menu, _, _| {
                        menu = menu.scrollable(true).max_h(px(360.)).min_w(px(280.));
                        for (id, title) in &cards {
                            let owner = owner.clone();
                            let pick = id.clone();
                            menu = menu.item(
                                PopupMenuItem::new(title.clone())
                                    .checked(selected.as_ref() == Some(id))
                                    .on_click(move |_, _, cx| {
                                        let _ = owner.update(cx, |p, cx| {
                                            p.card = Some(pick.clone());
                                            cx.notify();
                                        });
                                    }),
                            );
                        }
                        menu
                    }),
            )
        };
        let playable = self.card.clone();
        let paused = self.paused;
        let level = self.volume.read(cx).value().start().round() as u8;
        let mut card = theme::card()
            .gap(px(14.))
            .child(theme::heading(
                format!("Lecture sur « {} »", self.name),
                19.,
            ))
            .child(chooser)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        theme::pill(
                            Button::new("device-play")
                                .icon(IconName::Play)
                                .label("Lire sur ce lecteur")
                                .on_click(cx.listener(move |p, _, _, cx| {
                                    if let Some(card) = playable.clone() {
                                        p.start(card, cx);
                                    }
                                })),
                            Look::Solid(&theme::MINT),
                            Height::Hero,
                            self.card.is_none(),
                            cx,
                        )
                        .flex_1(),
                    )
                    .child(theme::round(
                        Button::new("device-pause")
                            .icon(if paused {
                                IconName::Play
                            } else {
                                IconName::Pause
                            })
                            .accessibility_label(if paused {
                                "Reprendre la lecture"
                            } else {
                                "Mettre en pause"
                            })
                            .tooltip(if paused { "Reprendre" } else { "Pause" })
                            .on_click(cx.listener(|p, _, _, cx| {
                                let command = if p.paused {
                                    PlayerCommand::Resume
                                } else {
                                    PlayerCommand::Pause
                                };
                                p.player(command, cx);
                            })),
                        Look::Outline,
                        48.,
                        false,
                        cx,
                    ))
                    .child(theme::round(
                        Button::new("device-stop")
                            .icon(IconName::Square)
                            .accessibility_label("Arrêter la lecture")
                            .tooltip("Arrêter")
                            .on_click(cx.listener(|p, _, _, cx| {
                                p.paused = false;
                                p.player(PlayerCommand::Stop, cx);
                            })),
                        Look::Outline,
                        48.,
                        false,
                        cx,
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .child(label("Volume"))
                            .children(self.volume_set.then(|| {
                                div()
                                    .font_weight(FontWeight::EXTRA_BOLD)
                                    .text_color(theme::c(theme::SKY.ink))
                                    .child(format!("{level} %"))
                            })),
                    )
                    .child(Slider::new(&self.volume).bg(theme::c(theme::SKY.accent))),
            );
        let mut swatches = div().flex().flex_wrap().gap(px(8.));
        for (index, (name, rgb)) in LIGHTS.iter().enumerate() {
            let shown = swatch_color(*rgb);
            let [r, g, b] = *rgb;
            let swatch = Button::new(("device-light", index))
                .custom(
                    ButtonCustomVariant::new(cx)
                        .color(theme::c(shown))
                        .hover(theme::c(shown))
                        .active(theme::c(shown)),
                )
                .accessibility_label(format!("Veilleuse : {name}"))
                .tooltip(*name)
                .toggled(self.light == Some(index))
                .bg(theme::c(shown))
                .rounded(ButtonRounded::Size(px(999.)))
                .size(px(40.))
                .on_click(cx.listener(move |p, _, _, cx| {
                    p.light = Some(index);
                    p.player(PlayerCommand::Ambient { r, g, b }, cx);
                }));
            swatches = swatches.child(if self.light == Some(index) {
                swatch.shadow(swatch_ring())
            } else {
                swatch
            });
        }
        let mut timers = div().flex().flex_wrap().gap(px(6.));
        for (name, seconds) in TIMERS {
            timers = timers.child(theme::pill(
                Button::new(("device-timer", seconds as usize))
                    .label(name)
                    .on_click(cx.listener(move |p, _, _, cx| {
                        p.timer = Some(seconds);
                        p.player(PlayerCommand::SleepTimer { seconds }, cx);
                    })),
                if self.timer == Some(seconds) {
                    Look::Dark
                } else {
                    Look::Soft(&theme::LIGHT_SAND)
                },
                Height::Small,
                false,
                cx,
            ));
        }
        card = card
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(label("Veilleuse"))
                    .child(swatches),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(label("Minuterie"))
                    .child(timers),
            );
        if !online {
            card = card.child(theme::note(
                "Ce lecteur semble hors ligne : allumez-le et vérifiez qu’il est connecté au Wi-Fi.",
            ));
        }
        if !self.status.is_empty() {
            card = card.child(theme::note(self.status.clone()));
        }
        card.into_any_element()
    }
    /// « Jour et nuit », the collapsed « Plus de réglages » and their content.
    pub fn settings(&mut self, advanced: bool, cx: &mut Context<Self>) -> AnyElement {
        let night = self.night;
        let mut column = div().flex().flex_col().gap(px(12.));
        let mut card = theme::card().px(px(20.)).py(px(18.)).child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(10.))
                .child(theme::heading("Jour et nuit", 19.))
                .child(
                    theme::segmented()
                        .child(theme::segment(
                            Button::new("device-day")
                                .icon(IconName::Sun)
                                .label("Jour")
                                .on_click(cx.listener(|p, _, _, cx| {
                                    p.night = false;
                                    cx.notify();
                                })),
                            !night,
                            Height::Small,
                            cx,
                        ))
                        .child(theme::segment(
                            Button::new("device-night")
                                .icon(IconName::Moon)
                                .label("Nuit")
                                .on_click(cx.listener(|p, _, _, cx| {
                                    p.night = true;
                                    cx.notify();
                                })),
                            night,
                            Height::Small,
                            cx,
                        )),
                ),
        );
        if self.loaded {
            let tiles: [(&str, Field); 4] = if night {
                [
                    ("Commence à (HH:MM)", Field::Night),
                    ("Volume maximal", Field::NightLimit),
                    ("Luminosité de l’écran", Field::NightBrightness),
                    ("Couleur de la lumière", Field::NightLight),
                ]
            } else {
                [
                    ("Commence à (HH:MM)", Field::Day),
                    ("Volume maximal", Field::DayLimit),
                    ("Luminosité de l’écran", Field::DayBrightness),
                    ("Couleur de la lumière", Field::Light),
                ]
            };
            let mut grid = div().flex().flex_wrap().gap(px(10.));
            for (text, field) in tiles {
                let colour = matches!(field, Field::Light | Field::NightLight);
                let mut tile = div()
                    .flex_1()
                    .min_w(px(160.))
                    .flex_basis(relative(0.4))
                    .bg(theme::c(theme::CREAM))
                    .rounded(px(14.))
                    .px(px(12.))
                    .py(px(10.))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(caption(text));
                if colour {
                    tile = tile.child(self.colour_picker(field, cx));
                }
                if !colour || advanced {
                    tile = tile.child(theme::small_field(Input::new(self.stored(field))));
                }
                grid = grid.child(tile);
            }
            card = card.child(grid).child(
                div().flex().justify_end().child(theme::pill(
                    Button::new("save-device-config")
                        .label("Enregistrer ces réglages")
                        .on_click(cx.listener(|p, _, _, cx| p.save(cx))),
                    Look::Soft(&theme::SKY),
                    Height::Medium,
                    false,
                    cx,
                )),
            );
        } else if self.loading {
            card = card.child(theme::note("Lecture des réglages du lecteur…"));
        } else {
            card = card
                .child(theme::note(
                    "Les réglages de ce lecteur n’ont pas pu être lus. Vérifiez qu’il est allumé, puis réessayez.",
                ))
                .child(div().flex().justify_end().child(theme::pill(
                    Button::new("load-config")
                        .label("Lire les réglages")
                        .on_click(cx.listener(|p, _, _, cx| p.load(cx))),
                    Look::Soft(&theme::SKY),
                    Height::Medium,
                    false,
                    cx,
                )));
        }
        column = column.child(card).child(
            theme::disclosure(
                "device-more",
                "Plus de réglages",
                if advanced {
                    "Casque, arrêt auto, raccourcis, commandes techniques"
                } else {
                    "Nom, casque, arrêt automatique"
                },
                self.more_open,
                cx,
            )
            .on_click(cx.listener(|p, _, _, cx| {
                p.more_open = !p.more_open;
                cx.notify();
            })),
        );
        if self.more_open {
            column = column.child(self.more(advanced, cx));
        }
        column.into_any_element()
    }
    /// Everything behind « Plus de réglages »: rarely changed settings, the raw Yoto
    /// values, beta shortcuts and the technical player commands.
    fn more(&mut self, advanced: bool, cx: &mut Context<Self>) -> Div {
        let labelled = |text: &'static str, input: &Entity<InputState>| {
            div()
                .flex_1()
                .min_w(px(180.))
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(caption(text))
                .child(theme::small_field(Input::new(input)))
        };
        let row = || div().flex().flex_wrap().gap(px(10.));
        let mut card = theme::card();
        if self.loaded {
            card = card
                .child(theme::heading("Lecteur", 17.))
                .child(labelled("Nom du lecteur", self.stored(Field::Name)))
                .child(
                    row()
                        .child(labelled(
                            "Délai avant atténuation de l’écran",
                            self.stored(Field::Dim),
                        ))
                        .child(labelled(
                            "Délai avant arrêt auto",
                            self.stored(Field::Shutdown),
                        )),
                )
                .child(
                    Checkbox::new("repeat")
                        .label("Répéter les pistes")
                        .checked(self.repeat)
                        .on_click(cx.listener(|p, checked, _, cx| {
                            p.repeat = *checked;
                            cx.notify();
                        })),
                )
                .child(
                    Checkbox::new("headphones")
                        .label("Casque Bluetooth")
                        .checked(self.headphones)
                        .on_click(cx.listener(|p, checked, _, cx| {
                            p.headphones = *checked;
                            cx.notify();
                        })),
                )
                .child(
                    Checkbox::new("limited")
                        .label("Limiter le volume du casque")
                        .checked(self.limited)
                        .on_click(cx.listener(|p, checked, _, cx| {
                            p.limited = *checked;
                            cx.notify();
                        })),
                );
        }
        if self.loaded && advanced {
            card = card
                .child(theme::heading("Valeurs Yoto avancées", 17.))
                .child(theme::note(
                    "Valeurs transmises telles quelles à Yoto. Ne les modifiez que si vous savez ce qu’elles font.",
                ));
            let mut raw = row();
            for field in [
                Field::VolumeLevel,
                Field::BluetoothEnabled,
                Field::ClockFace,
                Field::Locale,
                Field::HourFormat,
                Field::DimBrightness,
                Field::DayDaily,
                Field::NightDaily,
                Field::DayRadio,
                Field::NightRadio,
            ] {
                raw = raw.child(labelled(field.label(), self.stored(field)));
            }
            card = card.child(raw);
        }
        if self.loaded {
            card = card.child(
                div().flex().justify_end().child(theme::pill(
                    Button::new("save-device-more")
                        .label("Enregistrer ces réglages")
                        .on_click(cx.listener(|p, _, _, cx| p.save(cx))),
                    Look::Soft(&theme::SKY),
                    Height::Medium,
                    false,
                    cx,
                )),
            );
        }
        if !advanced {
            return card.child(theme::note(
                "Les raccourcis et les commandes techniques du lecteur s’affichent avec le « Mode avancé », dans « Compte ».",
            ));
        }
        card = card
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(theme::heading("Raccourcis", 17.))
                    .child(theme::tag("bêta", &theme::BUTTER)),
            )
            .child(labelled(
                Field::DayShortcut.label(),
                self.stored(Field::DayShortcut),
            ))
            .child(labelled(
                Field::NightShortcut.label(),
                self.stored(Field::NightShortcut),
            ))
            .child(
                div().flex().justify_end().child(theme::pill(
                    Button::new("save-shortcuts")
                        .label("Enregistrer les raccourcis")
                        .on_click(cx.listener(|p, _, _, cx| p.shortcuts(cx))),
                    Look::Soft(&theme::SKY),
                    Height::Medium,
                    !self.loaded,
                    cx,
                )),
            )
            .child(theme::heading("Commandes techniques", 17.));
        let mut commands = div().flex().flex_wrap().gap(px(6.)).child(theme::pill(
            Button::new("rest-status")
                .label("Actualiser l’état du lecteur")
                .on_click(cx.listener(|p, _, _, cx| {
                    p.send(
                        Command::DeviceStatus {
                            account: p.account.clone(),
                            device: p.device.clone(),
                        },
                        cx,
                    )
                })),
            Look::Outline,
            Height::Small,
            false,
            cx,
        ));
        for command in [
            PlayerCommand::Status,
            PlayerCommand::Events,
            PlayerCommand::BluetoothState,
            PlayerCommand::BluetoothOff,
            PlayerCommand::BluetoothDisconnect,
            PlayerCommand::Reboot,
        ] {
            let label = command.label();
            let reboot = matches!(command, PlayerCommand::Reboot);
            commands = commands.child(theme::pill(
                Button::new(label)
                    .label(label)
                    .on_click(cx.listener(move |p, _, window, cx| {
                        if reboot {
                            let owner = cx.entity().downgrade();
                            let name = p.name.clone();
                            confirm_destructive(
                                window,
                                cx,
                                format!("Redémarrer « {name} » ?"),
                                "La lecture en cours s’arrête et le lecteur reste indisponible le temps de redémarrer.",
                                "Redémarrer",
                                move |_, cx| {
                                    let _ = owner.update(cx, |p, cx| {
                                        p.player(PlayerCommand::Reboot, cx)
                                    });
                                },
                            );
                        } else {
                            p.player(command.clone(), cx)
                        }
                    })),
                Look::Outline,
                Height::Small,
                false,
                cx,
            ));
        }
        card.child(commands)
    }
}

#[cfg(test)]
mod tests {
    use super::shortcuts;
    #[core::prelude::v1::test]
    fn shortcuts_accept_commas_and_line_breaks() {
        let parsed = shortcuts("card-1/ch-1/tr-1, card-2/ch-2/tr-2\ncard-3/ch-3/tr-3").unwrap();
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[1].card.as_str(), "card-2");
        assert_eq!(parsed[2].track.as_str(), "tr-3");
        assert!(shortcuts("  ").unwrap().is_empty());
        assert!(shortcuts("card/chapter").is_err());
    }
}
