//! The « Pastel » look: every color, size and font of the interface, and the few shared
//! building blocks (pills, cards, tags, tiles) the screens compose.
//!
//! Screens never spell a hex value themselves; they pick a token or a family here, so the
//! palette can change in one place.
use super::*;
use gpui_kit::component::{
    Size, ThemeConfig,
    button::{ButtonCustomVariant, ButtonRounded},
};
use std::{borrow::Cow, rc::Rc, sync::LazyLock};

/// Rounded display face for titles (SIL Open Font License, embedded).
pub const HEADING_FONT: &str = "Fredoka";
/// Body face (SIL Open Font License, embedded).
pub const BODY_FONT: &str = "Nunito";

const FONTS: &[&[u8]] = &[
    include_bytes!("../../../../assets/fonts/Fredoka-SemiBold.ttf"),
    include_bytes!("../../../../assets/fonts/Nunito-Regular.ttf"),
    include_bytes!("../../../../assets/fonts/Nunito-SemiBold.ttf"),
    include_bytes!("../../../../assets/fonts/Nunito-Bold.ttf"),
    include_bytes!("../../../../assets/fonts/Nunito-ExtraBold.ttf"),
];

/// Window background.
pub const CREAM: u32 = 0xFFF8EF;
/// Sidebar, segmented tracks and other recessed surfaces.
pub const SAND: u32 = 0xFFF1E3;
/// Cards, rows, inputs.
pub const SURFACE: u32 = 0xFFFFFF;
/// Card edges and outlined buttons.
pub const BORDER: u32 = 0xF0E2D0;
/// Separators between panes.
pub const DIVIDER: u32 = 0xF5EADD;
/// Empty checkboxes and dashed drop zones.
pub const BOX_BORDER: u32 = 0xE3D3BF;
/// Main text.
pub const INK: u32 = 0x23323A;
/// Body copy on light surfaces.
pub const INK_SOFT: u32 = 0x45535A;
/// Secondary text.
pub const MUTED: u32 = 0x5E6B72;
/// Captions and tertiary labels.
pub const MUTED_WARM: u32 = 0x6B6158;
/// Text of neutral tags and of quiet round buttons.
pub const BROWN: u32 = 0x6B4A2B;
/// Text on the peach primary button.
pub const ON_PRIMARY: u32 = 0x2A1A12;
/// Track numbers in the playlist editor.
pub const NUMBER: u32 = 0x9A8B7A;
/// Offline dot.
pub const OFFLINE: u32 = 0xB9AC9C;

/// One pastel family: a soft tile, a saturated accent, a readable ink and the darker
/// « step » drawn under raised buttons.
pub struct Family {
    pub soft: u32,
    pub accent: u32,
    pub ink: u32,
    /// Icon color on the soft tile (slightly lighter than `ink` for some families).
    pub icon: u32,
    pub step: u32,
}
/// Main actions, the library.
pub const PEACH: Family = Family {
    soft: 0xFFE3D3,
    accent: 0xFF8A5B,
    ink: 0x8A3A17,
    icon: 0xB4481D,
    step: 0xE06A3A,
};
/// Success, listening, the account.
pub const MINT: Family = Family {
    soft: 0xD7F2E3,
    accent: 0x3FB57A,
    ink: 0x1E6B45,
    icon: 0x1E6B45,
    step: 0x2E9562,
};
/// Information, trimming, players.
pub const SKY: Family = Family {
    soft: 0xDCEBFF,
    accent: 0x4C8DF0,
    ink: 0x1D4E99,
    icon: 0x1D4E99,
    step: 0x3A74D6,
};
/// Work in progress, imports.
pub const BUTTER: Family = Family {
    soft: 0xFFF1C2,
    accent: 0xF7B731,
    ink: 0x7A5500,
    icon: 0x7A5500,
    step: 0xD99A1A,
};
/// Playlists, stories.
pub const LILAC: Family = Family {
    soft: 0xECE3FF,
    accent: 0x8B6CF0,
    ink: 0x4B3399,
    icon: 0x6A4FD6,
    step: 0xD9CBFA,
};
/// Destructive actions and failures.
pub const ROSE: Family = Family {
    soft: 0xFFD9DA,
    accent: 0xE5484D,
    ink: 0xA1262B,
    icon: 0xA1262B,
    step: 0xC93A3F,
};
/// Quiet sand buttons on the cream background (refresh, close).
pub const LIGHT_SAND: Family = Family {
    soft: SAND,
    accent: BOX_BORDER,
    ink: BROWN,
    icon: BROWN,
    step: BORDER,
};
/// Neutral family for idle or offline things.
pub const STONE: Family = Family {
    soft: 0xF3ECE3,
    accent: 0xC9B8A3,
    ink: MUTED_WARM,
    icon: MUTED_WARM,
    step: BOX_BORDER,
};
/// Families a thumbnail color is drawn from.
const THUMBNAILS: [&Family; 5] = [&PEACH, &MINT, &SKY, &BUTTER, &LILAC];

/// A token as a GPUI color.
pub fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}
fn hex(value: u32) -> String {
    format!("#{value:06X}")
}

/// A stable pastel family for anything with an identifier, so a media or playlist keeps
/// its color between launches.
pub fn family_for(seed: &str) -> &'static Family {
    // FNV-1a: stable across Rust releases, unlike the standard hasher.
    let hash = seed.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    });
    THUMBNAILS[(hash % THUMBNAILS.len() as u64) as usize]
}

/// Register the embedded fonts and install the Pastel theme. Without the fonts (a broken
/// font file) the theme keeps the system font rather than failing to start.
pub fn install(cx: &mut App) {
    let fonts_loaded = cx
        .text_system()
        .add_fonts(FONTS.iter().map(|font| Cow::Borrowed(*font)).collect())
        .inspect_err(|error| eprintln!("Embedded fonts unavailable: {error}"))
        .is_ok();
    let colors: Vec<(&str, String)> = vec![
        ("background", hex(CREAM)),
        ("foreground", hex(INK)),
        ("caret", hex(INK)),
        ("border", hex(BORDER)),
        ("input.border", hex(BOX_BORDER)),
        ("ring", hex(PEACH.accent)),
        ("selection.background", hex(PEACH.soft)),
        ("link", hex(PEACH.icon)),
        ("link.hover", hex(PEACH.ink)),
        ("link.active", hex(PEACH.ink)),
        ("primary.background", hex(PEACH.accent)),
        ("primary.hover.background", "#F57E4E".into()),
        ("primary.active.background", hex(PEACH.step)),
        ("primary.foreground", hex(ON_PRIMARY)),
        ("secondary.background", hex(PEACH.soft)),
        ("secondary.hover.background", "#FFD6C0".into()),
        ("secondary.active.background", "#FFC9AD".into()),
        ("secondary.foreground", hex(PEACH.ink)),
        ("muted.background", hex(SAND)),
        ("muted.foreground", hex(MUTED)),
        ("accent.background", hex(SAND)),
        ("accent.foreground", hex(INK)),
        ("popover.background", hex(SURFACE)),
        ("popover.foreground", hex(INK)),
        ("list.background", hex(SURFACE)),
        ("list.hover.background", hex(SAND)),
        ("list.active.background", hex(PEACH.soft)),
        ("list.active.border", hex(PEACH.accent)),
        ("sidebar.background", hex(SAND)),
        ("sidebar.foreground", hex(INK)),
        ("sidebar.border", hex(DIVIDER)),
        ("title_bar.background", hex(SAND)),
        ("title_bar.border", hex(SAND)),
        ("tab_bar.segmented.background", hex(SAND)),
        ("tab.active.background", hex(SURFACE)),
        ("tab.active.foreground", hex(INK)),
        ("tab.foreground", hex(MUTED)),
        ("button.background", hex(SURFACE)),
        ("button.hover.background", hex(CREAM)),
        ("button.active.background", hex(SAND)),
        ("button.foreground", hex(INK)),
        ("button.primary.background", hex(PEACH.accent)),
        ("button.primary.hover.background", "#F57E4E".into()),
        ("button.primary.active.background", hex(PEACH.step)),
        ("button.primary.foreground", hex(ON_PRIMARY)),
        ("button.secondary.background", hex(PEACH.soft)),
        ("button.secondary.hover.background", "#FFD6C0".into()),
        ("button.secondary.active.background", "#FFC9AD".into()),
        ("button.secondary.foreground", hex(PEACH.ink)),
        ("button.danger.background", hex(ROSE.soft)),
        ("button.danger.hover.background", "#FFC9CB".into()),
        ("button.danger.active.background", "#FFBABD".into()),
        ("button.danger.foreground", hex(ROSE.ink)),
        ("button.success.background", hex(MINT.accent)),
        ("button.success.hover.background", "#38A96F".into()),
        ("button.success.active.background", hex(MINT.step)),
        ("button.success.foreground", hex(SURFACE)),
        ("button.info.background", hex(SKY.soft)),
        ("button.info.hover.background", "#CFE2FF".into()),
        ("button.info.active.background", "#BFD7FF".into()),
        ("button.info.foreground", hex(SKY.ink)),
        ("success.background", hex(MINT.accent)),
        ("success.foreground", hex(SURFACE)),
        ("info.background", hex(SKY.accent)),
        ("info.foreground", hex(SURFACE)),
        ("warning.background", hex(BUTTER.accent)),
        ("warning.foreground", hex(ON_PRIMARY)),
        ("danger.background", hex(ROSE.accent)),
        ("danger.foreground", hex(SURFACE)),
        ("progress.bar.background", hex(MINT.accent)),
        ("switch.background", hex(MINT.accent)),
        ("switch.thumb.background", hex(SURFACE)),
        ("slider.background", hex(PEACH.accent)),
        ("slider.thumb.background", hex(SURFACE)),
        ("skeleton.background", hex(SAND)),
        ("overlay", "#2E221861".into()),
        ("window.border", hex(BORDER)),
    ];
    let mut config = serde_json::json!({
        "name": "Pastel",
        "mode": "light",
        "font.size": 15.0,
        "radius": 12,
        "radius.lg": 22,
        "shadow": true,
        "colors": colors
            .into_iter()
            .map(|(key, value)| (key.to_owned(), serde_json::Value::from(value)))
            .collect::<serde_json::Map<_, _>>(),
    });
    if fonts_loaded {
        config["font.family"] = BODY_FONT.into();
    }
    let config: ThemeConfig = serde_json::from_value(config).expect("Pastel theme is valid");
    gpui_kit::component::Theme::update(cx, |theme| theme.apply_config(&Rc::new(config)));
}

/// The application icon, decoded once.
pub fn logo(size: f32) -> Img {
    static LOGO: LazyLock<Arc<Image>> = LazyLock::new(|| {
        Arc::new(Image::from_bytes(
            ImageFormat::Png,
            include_bytes!("../../../../assets/branding/portal-logo.png").to_vec(),
        ))
    });
    img(LOGO.clone()).size(px(size)).flex_shrink_0()
}

/// Flat « step » under raised surfaces and buttons: `0 <y>px 0 <color>`.
pub fn step(color: u32, y: f32) -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: c(color),
        offset: point(px(0.), px(y)),
        blur_radius: px(0.),
        spread_radius: px(0.),
        inset: false,
    }]
}
/// Soft lift for the active sidebar item and selected segments.
pub fn lift() -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: hsla(30. / 360., 0.5, 0.31, 0.10),
        offset: point(px(0.), px(2.)),
        blur_radius: px(8.),
        spread_radius: px(0.),
        inset: false,
    }]
}
/// Floating bars and popovers.
pub fn float(alpha: f32) -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: hsla(200. / 360., 0.25, 0.18, alpha),
        offset: point(px(0.), px(12.)),
        blur_radius: px(32.),
        spread_radius: px(0.),
        inset: false,
    }]
}

/// Display text in the heading face.
pub fn heading(text: impl Into<SharedString>, size: f32) -> Div {
    div()
        .font_family(HEADING_FONT)
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(size))
        .line_height(relative(1.15))
        .child(text.into())
}
/// Screen title with its one-line explanation.
pub fn screen_header(title: impl Into<SharedString>, subtitle: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(heading(title, 32.).whitespace_nowrap())
        .child(meta(subtitle).text_size(px(14.)).whitespace_nowrap())
}
/// Secondary information under a title: bold, muted, small.
pub fn meta(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(13.))
        .font_weight(FontWeight::BOLD)
        .text_color(c(MUTED))
        .child(text.into())
}
/// Explanatory paragraph.
pub fn note(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(13.))
        .line_height(relative(1.45))
        .text_color(c(MUTED))
        .child(text.into())
}
/// White card on the cream background.
pub fn card() -> Div {
    div()
        .bg(c(SURFACE))
        .rounded(px(22.))
        .p(px(20.))
        .shadow(step(BORDER, 2.))
        .flex()
        .flex_col()
        .gap(px(12.))
}
/// Cream inset inside a card.
pub fn inset() -> Div {
    div()
        .bg(c(CREAM))
        .rounded(px(16.))
        .p(px(14.))
        .flex()
        .flex_col()
        .gap(px(8.))
}
/// Small pill label: a pastel family is one meaning (content type or sending state).
pub fn tag(text: impl Into<SharedString>, family: &Family) -> Div {
    tag_colors(text, family.soft, family.ink)
}
pub fn tag_colors(text: impl Into<SharedString>, background: u32, ink: u32) -> Div {
    div()
        .flex_shrink_0()
        .px(px(11.))
        .py(px(4.))
        .rounded_full()
        .bg(c(background))
        .text_color(c(ink))
        .text_size(px(12.))
        .font_weight(FontWeight::EXTRA_BOLD)
        .child(text.into())
}
/// Counter badge, as in the sidebar.
pub fn badge(count: usize, family: &Family) -> Div {
    div()
        .flex_shrink_0()
        .px(px(8.))
        .py(px(2.))
        .rounded_full()
        .bg(c(family.soft))
        .text_color(c(family.ink))
        .text_size(px(12.))
        .font_weight(FontWeight::EXTRA_BOLD)
        .child(count.to_string())
}
/// Rounded square with a centered icon on a soft pastel background.
pub fn icon_tile(icon: IconName, family: &Family, size: f32, radius: f32) -> Div {
    div()
        .size(px(size))
        .flex_shrink_0()
        .rounded(px(radius))
        .bg(c(family.soft))
        .text_color(c(family.icon))
        .flex()
        .items_center()
        .justify_center()
        .child(
            gpui_kit::component::Icon::new(icon)
                .size(px((size * 0.5).round()))
                .text_color(c(family.icon)),
        )
}
/// Small round status dot.
pub fn dot(color: u32) -> Div {
    div()
        .size(px(8.))
        .flex_shrink_0()
        .rounded_full()
        .bg(c(color))
}
/// Round check mark on a solid mint disc, for finished things.
pub fn done_mark(size: f32) -> Div {
    div()
        .size(px(size))
        .flex_shrink_0()
        .rounded_full()
        .bg(c(MINT.accent))
        .flex()
        .items_center()
        .justify_center()
        .child(
            gpui_kit::component::Icon::new(IconName::Check)
                .size(px((size * 0.55).round()))
                .text_color(c(SURFACE)),
        )
}

/// Pixel glyphs drawn in the thumbnails, from the design's 8×7 grid.
#[derive(Clone, Copy)]
pub enum Glyph {
    Heart,
    Star,
    Moon,
    Note,
}
impl Glyph {
    const ALL: [Self; 4] = [Self::Note, Self::Heart, Self::Star, Self::Moon];
    pub fn for_seed(seed: &str) -> Self {
        let sum = seed
            .bytes()
            .fold(0usize, |sum, b| sum.wrapping_add(b.into()));
        Self::ALL[sum % Self::ALL.len()]
    }
    fn cells(self) -> &'static [(u8, u8, u8, u8)] {
        match self {
            Self::Heart => &[
                (1, 0, 2, 1),
                (5, 0, 2, 1),
                (0, 1, 8, 3),
                (1, 4, 6, 1),
                (2, 5, 4, 1),
                (3, 6, 2, 1),
            ],
            Self::Star => &[
                (3, 0, 1, 2),
                (0, 2, 7, 1),
                (1, 3, 5, 1),
                (2, 4, 3, 1),
                (1, 5, 2, 1),
                (4, 5, 2, 1),
                (0, 6, 2, 1),
                (5, 6, 2, 1),
            ],
            Self::Moon => &[
                (2, 0, 3, 1),
                (1, 1, 2, 1),
                (0, 2, 2, 3),
                (1, 5, 3, 1),
                (2, 6, 4, 1),
            ],
            Self::Note => &[
                (2, 0, 5, 1),
                (2, 1, 1, 4),
                (6, 1, 1, 3),
                (0, 4, 3, 2),
                (4, 3, 3, 2),
            ],
        }
    }
}
/// A pixel glyph `cell` pixels per grid square, in `color`.
pub fn pixel_glyph(glyph: Glyph, color: u32, cell: f32) -> Div {
    glyph.cells().iter().fold(
        div()
            .relative()
            .flex_shrink_0()
            .w(px(8. * cell))
            .h(px(7. * cell)),
        |glyph, &(x, y, w, h)| {
            glyph.child(
                div()
                    .absolute()
                    .left(px(f32::from(x) * cell))
                    .top(px(f32::from(y) * cell))
                    .w(px(f32::from(w) * cell))
                    .h(px(f32::from(h) * cell))
                    .bg(c(color)),
            )
        },
    )
}
/// Thumbnail for something without artwork: a stable pastel tile and pixel glyph.
pub fn thumbnail(seed: &str, size: f32, radius: f32) -> Div {
    let family = family_for(seed);
    div()
        .size(px(size))
        .flex_shrink_0()
        .rounded(px(radius))
        .bg(c(family.soft))
        .flex()
        .items_center()
        .justify_center()
        .child(pixel_glyph(
            Glyph::for_seed(seed),
            family.accent,
            (size / 16.).round().max(2.),
        ))
}

/// How a pill button is painted.
#[derive(Clone, Copy)]
pub enum Look {
    /// Peach, with its « step »: the one main action of a screen or dialog.
    Primary,
    /// Solid accent with white text and a step (Listen, Play).
    Solid(&'static Family),
    /// Soft pastel background with the family ink.
    Soft(&'static Family),
    /// White with a 2 px border.
    Outline,
    /// Text only.
    Ghost,
    /// Text only, in a family ink (« Tout sélectionner »).
    Link(&'static Family),
    /// Dark selected chip.
    Dark,
    /// On the dark selection bar.
    OnDark,
    /// Translucent on the dark selection bar, rose text.
    OnDarkDanger,
}
/// Pill heights from the design.
#[derive(Clone, Copy)]
pub enum Height {
    /// 48 px: the main action of a panel.
    Hero,
    /// 44 px: standard buttons.
    Large,
    /// 40 px: dialogs and secondary rows.
    Medium,
    /// 36 px: compact chips and filters.
    Small,
    /// 34 px: actions inside list cards.
    Tiny,
}
impl Height {
    fn px(self) -> f32 {
        match self {
            Self::Hero => 48.,
            Self::Large => 44.,
            Self::Medium => 40.,
            Self::Small => 36.,
            Self::Tiny => 34.,
        }
    }
    fn text(self) -> f32 {
        match self {
            Self::Hero | Self::Large => 15.,
            Self::Medium | Self::Small => 14.,
            Self::Tiny => 13.,
        }
    }
}
struct Paint {
    bg: Hsla,
    fg: Hsla,
    hover: Hsla,
    active: Hsla,
    border: bool,
    step: Option<u32>,
    weight: FontWeight,
}
fn paint(look: Look) -> Paint {
    let shade = |hex: u32, amount: f32| {
        let color = c(hex);
        hsla(color.h, color.s, (color.l - amount).max(0.), color.a)
    };
    match look {
        Look::Primary => Paint {
            bg: c(PEACH.accent),
            fg: c(ON_PRIMARY),
            hover: shade(PEACH.accent, 0.04),
            active: c(PEACH.step),
            border: false,
            step: Some(PEACH.step),
            weight: FontWeight::EXTRA_BOLD,
        },
        Look::Solid(family) => Paint {
            bg: c(family.accent),
            fg: c(SURFACE),
            hover: shade(family.accent, 0.04),
            active: c(family.step),
            border: false,
            step: Some(family.step),
            weight: FontWeight::EXTRA_BOLD,
        },
        Look::Soft(family) => Paint {
            bg: c(family.soft),
            fg: c(family.ink),
            hover: shade(family.soft, 0.03),
            active: shade(family.soft, 0.06),
            border: false,
            step: None,
            weight: FontWeight::EXTRA_BOLD,
        },
        Look::Outline => Paint {
            bg: c(SURFACE),
            fg: c(INK),
            hover: c(CREAM),
            active: c(SAND),
            border: true,
            step: None,
            weight: FontWeight::BOLD,
        },
        Look::Ghost => Paint {
            bg: c(SURFACE).opacity(0.),
            fg: c(MUTED),
            hover: c(SAND),
            active: shade(SAND, 0.03),
            border: false,
            step: None,
            weight: FontWeight::BOLD,
        },
        Look::Link(family) => Paint {
            bg: c(SURFACE).opacity(0.),
            fg: c(family.icon),
            hover: c(family.soft),
            active: shade(family.soft, 0.03),
            border: false,
            step: None,
            weight: FontWeight::EXTRA_BOLD,
        },
        Look::Dark => Paint {
            bg: c(INK),
            fg: c(SURFACE),
            hover: c(INK_SOFT),
            active: c(INK),
            border: false,
            step: None,
            weight: FontWeight::EXTRA_BOLD,
        },
        Look::OnDark => Paint {
            bg: c(SURFACE).opacity(0.),
            fg: c(SURFACE),
            hover: c(SURFACE).opacity(0.12),
            active: c(SURFACE).opacity(0.2),
            border: false,
            step: None,
            weight: FontWeight::BOLD,
        },
        Look::OnDarkDanger => Paint {
            bg: c(SURFACE).opacity(0.12),
            fg: c(ROSE.soft),
            hover: c(SURFACE).opacity(0.2),
            active: c(SURFACE).opacity(0.28),
            border: false,
            step: None,
            weight: FontWeight::EXTRA_BOLD,
        },
    }
}
fn painted(button: Button, look: Look, disabled: bool, cx: &App) -> Button {
    let paint = paint(look);
    let variant = ButtonCustomVariant::new(cx)
        .color(paint.bg)
        .foreground(paint.fg)
        .hover(paint.hover)
        .active(paint.active);
    let mut button = button
        .custom(variant)
        .disabled(disabled)
        .bg(paint.bg)
        .text_color(paint.fg)
        .font_weight(paint.weight)
        .rounded(ButtonRounded::Size(px(999.)));
    if paint.border {
        button = button.border_2().border_color(c(BORDER));
    }
    if let Some(color) = paint.step
        && !disabled
    {
        button = button.shadow(step(color, 3.));
    }
    if disabled {
        button = button.opacity(0.5);
    }
    button
}
/// A labelled pill button.
pub fn pill(button: Button, look: Look, height: Height, disabled: bool, cx: &App) -> Button {
    let pad = match height {
        Height::Hero | Height::Large => 20.,
        Height::Medium => 16.,
        Height::Small | Height::Tiny => 12.,
    };
    painted(button, look, disabled, cx)
        .with_size(Size::Large)
        .h(px(height.px()))
        .px(px(pad))
        .text_size(px(height.text()))
}
/// A round icon-only button; `button` must carry its icon and accessibility label.
pub fn round(button: Button, look: Look, diameter: f32, disabled: bool, cx: &App) -> Button {
    painted(button, look, disabled, cx)
        .with_size(Size::Size(px(diameter)))
        .size(px(diameter))
}
/// One option of a segmented control; pair with [`segmented`].
pub fn segment(button: Button, selected: bool, height: Height, cx: &App) -> Button {
    let paint = Paint {
        bg: if selected {
            c(SURFACE)
        } else {
            c(SURFACE).opacity(0.)
        },
        fg: c(if selected { INK } else { MUTED }),
        hover: if selected { c(SURFACE) } else { c(CREAM) },
        active: c(SURFACE),
        border: false,
        step: None,
        weight: if selected {
            FontWeight::EXTRA_BOLD
        } else {
            FontWeight::BOLD
        },
    };
    let variant = ButtonCustomVariant::new(cx)
        .color(paint.bg)
        .foreground(paint.fg)
        .hover(paint.hover)
        .active(paint.active);
    let button = button
        .custom(variant)
        .toggled(selected)
        .bg(paint.bg)
        .text_color(paint.fg)
        .font_weight(paint.weight)
        .rounded(ButtonRounded::Size(px(999.)))
        .with_size(Size::Large)
        .h(px(height.px()))
        .px(px(14.))
        .text_size(px(height.text()));
    if selected {
        button.shadow(lift())
    } else {
        button
    }
}
/// The sand track holding [`segment`]s.
pub fn segmented() -> Div {
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(2.))
        .p(px(4.))
        .rounded_full()
        .bg(c(SAND))
}
/// A large rounded text field, as in the design: 46 px, radius 14, 2 px border.
pub fn field(input: Input) -> Input {
    input
        .large()
        .h(px(46.))
        .rounded(px(14.))
        .border_2()
        .border_color(c(BORDER))
        .bg(c(SURFACE))
}
/// A compact field for forms inside cards: 40 px, radius 12.
pub fn small_field(input: Input) -> Input {
    input
        .h(px(40.))
        .rounded(px(12.))
        .border_2()
        .border_color(c(BORDER))
        .bg(c(SURFACE))
}
/// Collapsible section header row: title, summary and chevron, as one button.
pub fn disclosure(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    summary: impl Into<SharedString>,
    open: bool,
    cx: &App,
) -> Button {
    let title = title.into();
    let variant = ButtonCustomVariant::new(cx)
        .color(c(SURFACE))
        .foreground(c(INK))
        .hover(c(CREAM))
        .active(c(SAND));
    Button::new(id)
        .custom(variant)
        .toggled(open)
        .accessibility_label(title.clone())
        .bg(c(SURFACE))
        .rounded(ButtonRounded::Size(px(16.)))
        .h(px(52.))
        .px(px(16.))
        .w_full()
        .shadow(step(BORDER, 2.))
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(
                    div()
                        .flex_1()
                        .font_weight(FontWeight::EXTRA_BOLD)
                        .text_size(px(15.))
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(c(MUTED))
                        .truncate()
                        .child(summary.into()),
                )
                .child(
                    gpui_kit::component::Icon::new(if open {
                        IconName::ChevronUp
                    } else {
                        IconName::ChevronDown
                    })
                    .size(px(18.))
                    .text_color(c(MUTED)),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[core::prelude::v1::test]
    fn thumbnail_colors_are_stable_per_identifier() {
        let first = family_for("media-1").soft;
        assert_eq!(first, family_for("media-1").soft);
        assert!(THUMBNAILS.iter().any(|family| family.soft == first));
    }
    #[core::prelude::v1::test]
    fn glyphs_stay_inside_their_grid() {
        for glyph in Glyph::ALL {
            for &(x, y, w, h) in glyph.cells() {
                assert!(x + w <= 8 && y + h <= 7);
            }
        }
    }
}
