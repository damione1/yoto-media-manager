//! Interface language: gettext catalogues for French and English, embedded in the binary.
//!
//! Wrap every user-visible string in [`tr!`] (or [`trn!`] when it depends on a count).
//! The French wording is the message id, so the French catalogue mirrors the source and
//! the English one translates it. Values go in named placeholders, never by concatenation:
//! `tr!("« {title} » est prêt.", title = name)` lets a translator move `{title}`.
//! Placeholders take no format spec; format numbers before passing them.
//!
//! The catalogues live in `crates/yoto-desktop/lang`, extracted and checked by
//! `scripts/check_translations.sh`.
use std::{
    collections::HashMap,
    fmt::Display,
    sync::{
        LazyLock,
        atomic::{AtomicU8, Ordering},
    },
};

/// The interface languages. French is the source language of the message ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    French,
    English,
}

impl Language {
    pub const ALL: [Self; 2] = [Self::French, Self::English];

    /// The code stored in the settings.
    pub fn code(self) -> &'static str {
        match self {
            Self::French => "fr",
            Self::English => "en",
        }
    }

    /// The language a stored code names, if it is one the app offers.
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "fr" => Some(Self::French),
            "en" => Some(Self::English),
            _ => None,
        }
    }

    /// The interface language for an operating system locale such as `fr-CA` or `en_US`:
    /// any French variant is French, everything else English.
    pub fn for_locale(locale: &str) -> Self {
        if locale
            .get(..2)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("fr"))
        {
            Self::French
        } else {
            Self::English
        }
    }

    /// The language's own name, as shown in the language selector whatever the current
    /// language is, so a person can always find theirs.
    pub fn native_name(self) -> &'static str {
        match self {
            Self::French => "Français",
            Self::English => "English",
        }
    }

    fn catalogue(self) -> &'static Catalogue {
        match self {
            Self::French => &FRENCH,
            Self::English => &ENGLISH,
        }
    }

    /// Index of the plural form for `n`, as in the catalogue's `Plural-Forms` header.
    fn plural_index(self, n: u64) -> usize {
        match self {
            // nplurals=2; plural=(n > 1);
            Self::French => usize::from(n > 1),
            // nplurals=2; plural=(n != 1);
            Self::English => usize::from(n != 1),
        }
    }
}

/// The interface language: the one saved in the settings, or else the operating system's.
pub fn resolve(saved: Option<&str>, system_locale: Option<&str>) -> Language {
    saved
        .and_then(Language::from_code)
        .unwrap_or_else(|| system_locale.map_or(Language::English, Language::for_locale))
}

/// The operating system's preferred locale, if it reports one.
pub fn system_locale() -> Option<String> {
    sys_locale::get_locale()
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// Switch the interface language. Views pick it up on their next render.
pub fn set_language(language: Language) {
    CURRENT.store(
        match language {
            Language::French => 0,
            Language::English => 1,
        },
        Ordering::Relaxed,
    );
}

/// The interface language used by [`tr!`] and [`trn!`].
pub fn language() -> Language {
    #[cfg(test)]
    if let Some(language) = tests::OVERRIDE.with(std::cell::Cell::get) {
        return language;
    }
    match CURRENT.load(Ordering::Relaxed) {
        1 => Language::English,
        _ => Language::French,
    }
}

/// The translation of `msgid` in the interface language; the id itself when the
/// catalogue has none.
pub fn gettext(context: Option<&str>, msgid: &'static str) -> &'static str {
    language()
        .catalogue()
        .get(context, msgid)
        .and_then(|forms| forms.first())
        .map_or(msgid, String::as_str)
}

/// The translation of `singular`/`plural` for the count `n` in the interface language.
pub fn ngettext(
    context: Option<&str>,
    singular: &'static str,
    plural: &'static str,
    n: u64,
) -> &'static str {
    let language = language();
    let index = language.plural_index(n);
    language
        .catalogue()
        .get(context, singular)
        .and_then(|forms| forms.get(index))
        .map(String::as_str)
        .unwrap_or_else(|| {
            // Without a translation, the French ids follow the French plural rule.
            if Language::French.plural_index(n) == 0 {
                singular
            } else {
                plural
            }
        })
}

/// Replace each `{name}` of a translated template with its value; `{{` and `}}` stand for
/// literal braces.
pub fn fill(template: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(at) = rest.find(['{', '}']) {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            out.push_str(&tail[..1]);
            rest = &tail[2..];
            continue;
        }
        if let Some(name) = tail
            .strip_prefix('{')
            .and_then(|t| t.find('}').map(|end| &t[..end]))
            && let Some((_, value)) = args.iter().find(|(arg, _)| *arg == name)
        {
            use std::fmt::Write;
            let _ = write!(out, "{value}");
            rest = &tail[name.len() + 2..];
            continue;
        }
        debug_assert!(false, "Unfilled placeholder in « {template} »");
        out.push_str(&tail[..1]);
        rest = &tail[1..];
    }
    out.push_str(rest);
    out
}

/// A user-visible string in the interface language.
///
/// `tr!("Texte")` gives a `&'static str`; with named values,
/// `tr!("« {title} » est prêt.", title = name)` gives a `String`.
macro_rules! tr {
    ($msgid:literal) => {
        $crate::i18n::gettext(None, $msgid)
    };
    ($msgid:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::fill(
            $crate::i18n::gettext(None, $msgid),
            &[$((stringify!($name), &$value as &dyn ::std::fmt::Display)),+],
        )
    };
}

/// A user-visible string that depends on a count, with the count as `{count}`:
/// `trn!("{count} piste", "{count} pistes", tracks.len())`. Further named values follow
/// the count.
macro_rules! trn {
    ($singular:literal, $plural:literal, $n:expr $(, $name:ident = $value:expr)* $(,)?) => {{
        let count = $n;
        $crate::i18n::fill(
            $crate::i18n::ngettext(None, $singular, $plural, count as u64),
            &[
                ("count", &count as &dyn ::std::fmt::Display),
                $((stringify!($name), &$value as &dyn ::std::fmt::Display)),*
            ],
        )
    }};
}

/// [`tr!`] with a disambiguating context, for the same French words that need different
/// translations: `trc!("verbe", "Lecture")`.
#[allow(unused_macros)]
macro_rules! trc {
    ($context:literal, $msgid:literal) => {
        $crate::i18n::gettext(Some($context), $msgid)
    };
    ($context:literal, $msgid:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::fill(
            $crate::i18n::gettext(Some($context), $msgid),
            &[$((stringify!($name), &$value as &dyn ::std::fmt::Display)),+],
        )
    };
}

static FRENCH: LazyLock<Catalogue> =
    LazyLock::new(|| Catalogue::parse(include_str!("../lang/fr/LC_MESSAGES/yoto-desktop.po")));
static ENGLISH: LazyLock<Catalogue> =
    LazyLock::new(|| Catalogue::parse(include_str!("../lang/en/LC_MESSAGES/yoto-desktop.po")));

/// Translated forms by message, keyed like gettext: `context\u{4}msgid`.
#[derive(Default)]
struct Catalogue {
    messages: HashMap<String, Vec<String>>,
    header: String,
}

impl Catalogue {
    fn get(&self, context: Option<&str>, msgid: &str) -> Option<&Vec<String>> {
        match context {
            None => self.messages.get(msgid),
            Some(context) => self.messages.get(&format!("{context}\u{4}{msgid}")),
        }
        .filter(|forms| forms.iter().all(|form| !form.is_empty()))
    }

    /// Read a PO file as written by the gettext tools. Fuzzy and obsolete entries are not
    /// translations and are skipped.
    fn parse(source: &str) -> Self {
        let mut catalogue = Self::default();
        let mut entry = Entry::default();
        for line in source.lines().chain(std::iter::once("")) {
            let line = line.trim();
            if line.is_empty() {
                catalogue.add(std::mem::take(&mut entry));
            } else if let Some(flags) = line.strip_prefix("#,") {
                entry.fuzzy |= flags.split(',').any(|flag| flag.trim() == "fuzzy");
            } else if line.starts_with('#') {
                continue;
            } else if let Some(text) = line.strip_prefix('"') {
                if let Some(field) = entry.current() {
                    field.push_str(&unescape(text));
                }
            } else if let Some((keyword, text)) = line.split_once(' ') {
                let text = text
                    .trim()
                    .strip_prefix('"')
                    .map(unescape)
                    .unwrap_or_default();
                entry.start(keyword, text);
            }
        }
        catalogue
    }

    fn add(&mut self, entry: Entry) {
        let Some(msgid) = entry.msgid else {
            return;
        };
        if msgid.is_empty() {
            self.header = entry.msgstr.into_iter().next().unwrap_or_default();
            return;
        }
        if entry.fuzzy {
            return;
        }
        let key = match entry.msgctxt {
            Some(context) => format!("{context}\u{4}{msgid}"),
            None => msgid,
        };
        self.messages.insert(key, entry.msgstr);
    }
}

#[derive(Default)]
struct Entry {
    msgctxt: Option<String>,
    msgid: Option<String>,
    msgid_plural: Option<String>,
    msgstr: Vec<String>,
    fuzzy: bool,
    field: Field,
}

#[derive(Default, Clone, Copy)]
enum Field {
    #[default]
    None,
    Context,
    Id,
    Plural,
    Str(usize),
}

impl Entry {
    fn start(&mut self, keyword: &str, text: String) {
        self.field = match keyword {
            "msgctxt" => {
                self.msgctxt = Some(text);
                Field::Context
            }
            "msgid" => {
                self.msgid = Some(text);
                Field::Id
            }
            "msgid_plural" => {
                self.msgid_plural = Some(text);
                Field::Plural
            }
            "msgstr" => {
                self.msgstr = vec![text];
                Field::Str(0)
            }
            other => match other
                .strip_prefix("msgstr[")
                .and_then(|i| i.strip_suffix(']'))
                .and_then(|i| i.parse::<usize>().ok())
            {
                Some(index) => {
                    if self.msgstr.len() <= index {
                        self.msgstr.resize(index + 1, String::new());
                    }
                    self.msgstr[index] = text;
                    Field::Str(index)
                }
                None => Field::None,
            },
        };
    }

    fn current(&mut self) -> Option<&mut String> {
        match self.field {
            Field::None => None,
            Field::Context => self.msgctxt.as_mut(),
            Field::Id => self.msgid.as_mut(),
            Field::Plural => self.msgid_plural.as_mut(),
            Field::Str(index) => self.msgstr.get_mut(index),
        }
    }
}

/// The text of a PO string literal whose opening quote was already removed.
fn unescape(quoted: &str) -> String {
    let body = quoted.strip_suffix('"').unwrap_or(quoted);
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    thread_local! {
        pub(super) static OVERRIDE: std::cell::Cell<Option<Language>> =
            const { std::cell::Cell::new(None) };
    }

    /// Run `f` with the interface in `language`, for this test thread only.
    pub(crate) fn with_language<R>(language: Language, f: impl FnOnce() -> R) -> R {
        let previous = OVERRIDE.with(|cell| cell.replace(Some(language)));
        let result = f();
        OVERRIDE.with(|cell| cell.set(previous));
        result
    }

    #[test]
    fn the_language_follows_the_setting_then_the_system() {
        assert_eq!(resolve(Some("en"), Some("fr-CA")), Language::English);
        assert_eq!(resolve(Some("fr"), Some("en-US")), Language::French);
        assert_eq!(resolve(None, Some("fr-CA")), Language::French);
        assert_eq!(resolve(None, Some("fr")), Language::French);
        assert_eq!(resolve(None, Some("FR_be")), Language::French);
        assert_eq!(resolve(None, Some("en-GB")), Language::English);
        assert_eq!(resolve(None, Some("de-DE")), Language::English);
        assert_eq!(resolve(None, Some("")), Language::English);
        assert_eq!(resolve(None, None), Language::English);
        // An unknown stored code is ignored rather than trusted.
        assert_eq!(resolve(Some("de"), Some("fr-FR")), Language::French);
        for language in Language::ALL {
            assert_eq!(Language::from_code(language.code()), Some(language));
        }
    }

    #[test]
    fn placeholders_are_filled_by_name_in_any_order() {
        let template = "« {title} » : {count} pistes";
        assert_eq!(
            fill(template, &[("count", &3), ("title", &"Berceuses")]),
            "« Berceuses » : 3 pistes"
        );
        assert_eq!(fill("{b} then {a}", &[("a", &1), ("b", &2)]), "2 then 1");
        assert_eq!(fill("{{literal}} {x}", &[("x", &"ok")]), "{literal} ok");
        assert_eq!(fill("no placeholder", &[]), "no placeholder");
    }

    #[test]
    fn strings_follow_the_interface_language() {
        let french = with_language(Language::French, || tr!("Annuler"));
        let english = with_language(Language::English, || tr!("Annuler"));
        assert_eq!(french, "Annuler");
        assert_eq!(english, "Cancel");
    }

    #[test]
    fn plurals_follow_each_language_rule() {
        let tracks = |n: usize| trn!("{count} piste", "{count} pistes", n);
        with_language(Language::French, || {
            assert_eq!(tracks(0), "0 piste");
            assert_eq!(tracks(1), "1 piste");
            assert_eq!(tracks(2), "2 pistes");
        });
        with_language(Language::English, || {
            assert_eq!(tracks(0), "0 tracks");
            assert_eq!(tracks(1), "1 track");
            assert_eq!(tracks(2), "2 tracks");
        });
    }

    #[test]
    fn po_entries_are_read_with_contexts_plurals_and_escapes() {
        let catalogue = Catalogue::parse(concat!(
            "msgid \"\"\n",
            "msgstr \"\"\n",
            "\"Plural-Forms: nplurals=2; plural=(n != 1);\\n\"\n",
            "\n",
            "msgctxt \"verbe\"\n",
            "msgid \"Lecture\"\n",
            "msgstr \"Play\"\n",
            "\n",
            "msgid \"\"\n",
            "\"Deux\\n\"\n",
            "\"lignes « {x} »\"\n",
            "msgstr \"Two\\nlines \\\"{x}\\\"\"\n",
            "\n",
            "#, fuzzy\n",
            "msgid \"Douteux\"\n",
            "msgstr \"Doubtful\"\n",
            "\n",
            "msgid \"{count} piste\"\n",
            "msgid_plural \"{count} pistes\"\n",
            "msgstr[0] \"{count} track\"\n",
            "msgstr[1] \"{count} tracks\"\n",
            "\n",
            "#~ msgid \"Ancien\"\n",
            "#~ msgstr \"Old\"\n",
        ));
        assert!(catalogue.header.contains("plural=(n != 1)"));
        assert_eq!(
            catalogue.get(Some("verbe"), "Lecture"),
            Some(&vec!["Play".to_string()])
        );
        assert_eq!(catalogue.get(None, "Lecture"), None);
        assert_eq!(
            catalogue.get(None, "Deux\nlignes « {x} »"),
            Some(&vec!["Two\nlines \"{x}\"".to_string()])
        );
        assert_eq!(catalogue.get(None, "Douteux"), None);
        assert_eq!(catalogue.get(None, "Ancien"), None);
        assert_eq!(catalogue.get(None, "{count} piste").map(Vec::len), Some(2));
    }

    /// Names inside `{…}`, in order.
    fn placeholders(text: &str) -> Vec<&str> {
        let mut names: Vec<&str> = text
            .split('{')
            .skip(1)
            .filter_map(|part| part.split_once('}').map(|(name, _)| name))
            .filter(|name| !name.is_empty())
            .collect();
        names.sort_unstable();
        names
    }

    #[test]
    fn catalogues_are_complete_and_keep_every_placeholder() {
        for language in Language::ALL {
            let catalogue = language.catalogue();
            assert!(
                !catalogue.messages.is_empty(),
                "{language:?} catalogue is empty"
            );
            let header = match language {
                Language::French => "Plural-Forms: nplurals=2; plural=(n > 1);",
                Language::English => "Plural-Forms: nplurals=2; plural=(n != 1);",
            };
            assert!(
                catalogue.header.contains(header),
                "{language:?} plural rule differs from the code"
            );
            assert!(
                catalogue
                    .header
                    .contains(&format!("Language: {}", language.code())),
                "{language:?} header names another language"
            );
            for (key, forms) in &catalogue.messages {
                let msgid = key.rsplit('\u{4}').next().unwrap_or(key);
                assert!(
                    forms.iter().all(|form| !form.is_empty()),
                    "{language:?}: untranslated « {msgid} »"
                );
                for form in forms {
                    // A singular form may name its count in words instead of `{count}`.
                    let mut expected = placeholders(msgid);
                    let mut found = placeholders(form);
                    if forms.len() > 1 {
                        expected.retain(|name| *name != "count");
                        found.retain(|name| *name != "count");
                    }
                    assert_eq!(
                        found, expected,
                        "{language:?}: placeholders differ in « {form} »"
                    );
                }
            }
        }
        let french: std::collections::BTreeSet<_> = FRENCH.messages.keys().collect();
        let english: std::collections::BTreeSet<_> = ENGLISH.messages.keys().collect();
        assert_eq!(french, english, "the catalogues hold different messages");
    }

    #[test]
    fn the_french_catalogue_is_the_source_wording() {
        for (key, forms) in &FRENCH.messages {
            let msgid = key.rsplit('\u{4}').next().unwrap_or(key);
            assert_eq!(forms[0], msgid, "French wording changed for « {msgid} »");
        }
    }
}
