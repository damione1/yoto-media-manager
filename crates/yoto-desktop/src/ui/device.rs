use super::*;
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Field {
    Name,
    Volume,
    Light,
    Sleep,
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
        Self::Volume,
        Self::Light,
        Self::Sleep,
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
            Self::Volume => "Volume (0–100)",
            Self::Light => "Lumière #RRGGBB",
            Self::Sleep => "Minuterie (secondes)",
            Self::Day => "Début du jour HH:MM",
            Self::Night => "Début de la nuit HH:MM",
            Self::DayLimit => "Volume maximal jour",
            Self::NightLimit => "Volume maximal nuit",
            Self::NightLight => "Lumière de nuit #RRGGBB",
            Self::Dim => "Délai avant atténuation",
            Self::Shutdown => "Délai avant arrêt",
            Self::DayShortcut => "Raccourcis jour : carte/chapitre/piste (un par ligne)",
            Self::NightShortcut => "Raccourcis nuit : carte/chapitre/piste (un par ligne)",
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
            Self::Name | Self::Volume | Self::Sleep | Self::DayShortcut | Self::NightShortcut => {
                None
            }
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
    tasks: Vec<Task<()>>,
    status: String,
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
        Self {
            account,
            device: device.id,
            service,
            runtime,
            inputs,
            repeat: false,
            headphones: false,
            limited: false,
            loaded: false,
            shortcut_base: None,
            tasks: vec![],
            status: "Chargez les réglages pour les modifier.".into(),
        }
    }
    fn value(&self, f: Field, cx: &App) -> String {
        self.inputs[&f].read(cx).value().to_string()
    }
    fn send(&mut self, command: Command, cx: &mut Context<Self>) {
        let service = self.service.clone();
        let rt = self.runtime.clone();
        self.status = "Connexion au lecteur…".into();
        self.tasks.push(cx.spawn(async move |weak, cx| {
            let r = rt
                .spawn(async move { service.execute(command).await })
                .await;
            let _ = weak.update(cx, |v, cx| {
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
                                            .join("\n")
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
                            "Réglages chargés. Modifiez-les ci-dessous puis enregistrez.".into()
                        } else {
                            "État du lecteur reçu.".into()
                        };
                    }
                    Ok(Ok(Reply::Player(result))) => {
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
    pub fn start(&mut self, card: CardId, cx: &mut Context<Self>) {
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
    fn volume(&mut self, cx: &mut Context<Self>) {
        match self
            .value(Field::Volume, cx)
            .parse::<u8>()
            .map_err(|_| Failure::Validation("Volume attendu de 0 à 100".into()))
            .and_then(Volume::new)
        {
            Ok(value) => self.player(PlayerCommand::Volume { value }, cx),
            Err(e) => {
                self.status = failure_text(&e);
                cx.notify();
            }
        }
    }
    fn light(&mut self, cx: &mut Context<Self>) {
        match color(&self.value(Field::Light, cx)) {
            Ok([r, g, b]) => self.player(PlayerCommand::Ambient { r, g, b }, cx),
            Err(e) => {
                self.status = failure_text(&e);
                cx.notify();
            }
        }
    }
    fn sleep(&mut self, cx: &mut Context<Self>) {
        match self.value(Field::Sleep, cx).parse::<u32>() {
            Ok(seconds) => self.player(PlayerCommand::SleepTimer { seconds }, cx),
            Err(_) => {
                self.status = "Entrez un nombre entier de secondes.".into();
                cx.notify();
            }
        }
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
fn color(s: &str) -> yoto_domain::Result<[u8; 3]> {
    let s = s
        .strip_prefix('#')
        .ok_or_else(|| Failure::Validation("Couleur attendue #RRGGBB".into()))?;
    if s.len() != 6 || !s.is_ascii() {
        return Err(Failure::Validation("Couleur attendue #RRGGBB".into()));
    }
    let parse = |r| {
        u8::from_str_radix(&s[r], 16)
            .map_err(|_| Failure::Validation("Couleur attendue #RRGGBB".into()))
    };
    Ok([parse(0..2)?, parse(2..4)?, parse(4..6)?])
}
fn shortcuts(s: &str) -> yoto_domain::Result<Vec<Shortcut>> {
    s.lines()
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
impl Render for DevicePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_kit::component::checkbox::Checkbox;
        let mut body = div()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .border_1()
            .border_color(cx.theme().border)
            .rounded_lg()
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Lecteur sélectionné"),
            );
        let commands = [
            PlayerCommand::Pause,
            PlayerCommand::Resume,
            PlayerCommand::Stop,
            PlayerCommand::Reboot,
            PlayerCommand::Events,
            PlayerCommand::Status,
            PlayerCommand::BluetoothOff,
            PlayerCommand::BluetoothDisconnect,
            PlayerCommand::BluetoothState,
        ];
        let mut row = div().flex().flex_wrap().gap_2();
        for command in commands {
            row = row.child(
                Button::new(command.label())
                    .label(command.label())
                    .on_click(cx.listener(move |v, _, _, cx| v.player(command.clone(), cx))),
            );
        }
        body = body.child(row);
        body = body
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(Input::new(&self.inputs[&Field::Volume]))
                    .child(
                        Button::new("volume")
                            .label("Régler le volume")
                            .on_click(cx.listener(|v, _, _, cx| v.volume(cx))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(Input::new(&self.inputs[&Field::Light]))
                    .child(
                        Button::new("light")
                            .label("Régler la lumière")
                            .on_click(cx.listener(|v, _, _, cx| v.light(cx))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(Input::new(&self.inputs[&Field::Sleep]))
                    .child(
                        Button::new("sleep")
                            .label("Minuterie")
                            .on_click(cx.listener(|v, _, _, cx| v.sleep(cx))),
                    ),
            );
        body = body
            .child(
                Button::new("load-config")
                    .label("Charger les réglages du lecteur")
                    .on_click(cx.listener(|v, _, _, cx| {
                        v.send(
                            Command::DeviceConfig {
                                account: v.account.clone(),
                                device: v.device.clone(),
                            },
                            cx,
                        )
                    })),
            )
            .child(
                Button::new("rest-status")
                    .label("Actualiser l’état du lecteur")
                    .on_click(cx.listener(|v, _, _, cx| {
                        v.send(
                            Command::DeviceStatus {
                                account: v.account.clone(),
                                device: v.device.clone(),
                            },
                            cx,
                        )
                    })),
            );
        if self.loaded {
            for f in [
                Field::Name,
                Field::Day,
                Field::Night,
                Field::DayLimit,
                Field::NightLimit,
                Field::NightLight,
                Field::Dim,
                Field::Shutdown,
                Field::VolumeLevel,
                Field::BluetoothEnabled,
                Field::ClockFace,
                Field::Locale,
                Field::HourFormat,
                Field::DayBrightness,
                Field::NightBrightness,
                Field::DimBrightness,
                Field::DayDaily,
                Field::NightDaily,
                Field::DayRadio,
                Field::NightRadio,
            ] {
                body = body.child(f.label()).child(Input::new(&self.inputs[&f]));
            }
            body = body
                .child(
                    Checkbox::new("repeat")
                        .label("Répéter les pistes")
                        .checked(self.repeat)
                        .on_click(cx.listener(|v, checked, _, cx| {
                            v.repeat = *checked;
                            cx.notify();
                        })),
                )
                .child(
                    Checkbox::new("headphones")
                        .label("Casque Bluetooth")
                        .checked(self.headphones)
                        .on_click(cx.listener(|v, checked, _, cx| {
                            v.headphones = *checked;
                            cx.notify();
                        })),
                )
                .child(
                    Checkbox::new("limited")
                        .label("Limiter le volume du casque")
                        .checked(self.limited)
                        .on_click(cx.listener(|v, checked, _, cx| {
                            v.limited = *checked;
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("save-device-config")
                        .label("Enregistrer ces réglages")
                        .primary()
                        .on_click(cx.listener(|v, _, _, cx| v.save(cx))),
                );
        }
        body.child(Field::DayShortcut.label())
            .child(Input::new(&self.inputs[&Field::DayShortcut]))
            .child(Field::NightShortcut.label())
            .child(Input::new(&self.inputs[&Field::NightShortcut]))
            .child(
                Button::new("save-shortcuts")
                    .label("Enregistrer les raccourcis")
                    .disabled(!self.loaded)
                    .on_click(cx.listener(|v, _, _, cx| v.shortcuts(cx))),
            )
            .child(div().text_sm().child(self.status.clone()))
    }
}
