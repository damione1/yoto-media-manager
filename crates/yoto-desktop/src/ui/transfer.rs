//! « Exporter la médiathèque… » and « Importer une médiathèque… »: the encrypted backup
//! and restore of the whole library, as two short dialogs.
use super::*;
use gpui_kit::component::{WindowExt, dialog::DialogClose, spinner::Spinner};
use theme::{Height, Look};
use wording::library_transfer as text;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Export,
    Import,
}

enum Outcome {
    Exported(std::path::PathBuf),
    Imported(std::path::PathBuf),
    Failed(String),
}

pub(super) struct LibraryTransfer {
    direction: Direction,
    owner: WeakEntity<AppView>,
    service: Arc<Service>,
    runtime: Arc<tokio::runtime::Runtime>,
    /// Export: the folder the export goes in. Import: the export's own folder.
    folder: Option<std::path::PathBuf>,
    /// Export: the recovery key made for this export, never shown elsewhere.
    key: String,
    /// Import: the key the parent pastes.
    key_input: Entity<InputState>,
    copied: bool,
    busy: bool,
    hint: Option<&'static str>,
    outcome: Option<Outcome>,
}

impl AppView {
    /// Open the export or import dialog. A transfer still running is shown again rather
    /// than replaced, so its result is not lost.
    pub fn open_transfer(
        &mut self,
        direction: Direction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let transfer = match self.transfer.clone().filter(|t| t.read(cx).busy) {
            Some(running) => running,
            None => {
                let owner = cx.entity().downgrade();
                let created = cx.new(|cx| LibraryTransfer::new(direction, self, owner, window, cx));
                self.transfer = Some(created.clone());
                created
            }
        };
        let title = match transfer.read(cx).direction {
            Direction::Export => text::export_title(),
            Direction::Import => text::import_title(),
        };
        window.open_dialog(cx, move |dialog, _, _| {
            dialog.title(title).w(px(560.)).child(transfer.clone())
        });
    }
}

impl LibraryTransfer {
    fn new(
        direction: Direction,
        view: &AppView,
        owner: WeakEntity<AppView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let key = match direction {
            Direction::Export => new_recovery_key(),
            Direction::Import => String::new(),
        };
        Self {
            direction,
            owner,
            service: view.service.clone(),
            runtime: view.runtime.clone(),
            folder: None,
            key,
            key_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(text::key_placeholder())),
            copied: false,
            busy: false,
            hint: None,
            outcome: None,
        }
    }

    fn fail(&mut self, hint: &'static str, cx: &mut Context<Self>) {
        self.hint = Some(hint);
        cx.notify();
    }

    /// The key as the service needs it, or why it cannot be used yet.
    fn import_key(&self, cx: &App) -> std::result::Result<String, &'static str> {
        let key = wording::normalize_recovery_key(self.key_input.read(cx).value().as_ref());
        wording::check_recovery_key(&key).map(|()| key)
    }

    fn choose_folder(&mut self, cx: &mut Context<Self>) {
        // The key comes first: no folder is chosen for an import that could not run.
        if self.direction == Direction::Import
            && let Err(hint) = self.import_key(cx)
        {
            return self.fail(hint, cx);
        }
        self.hint = None;
        let direction = self.direction;
        cx.spawn(async move |weak, cx| {
            let picked = rfd::AsyncFileDialog::new()
                .set_title(match direction {
                    Direction::Export => text::folder_picker(),
                    Direction::Import => text::export_picker(),
                })
                .pick_folder()
                .await;
            let Some(picked) = picked else {
                return;
            };
            let _ = weak.update(cx, |t, cx| {
                let folder = picked.path().to_owned();
                match direction {
                    Direction::Export => t.folder = Some(folder),
                    Direction::Import => match find_export(&folder) {
                        Some(export) => t.folder = Some(export),
                        None => {
                            t.folder = None;
                            t.hint = Some(text::not_an_export());
                        }
                    },
                }
                t.outcome = None;
                cx.notify();
            });
        })
        .detach();
    }

    fn run(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let (command, target) = match self.direction {
            Direction::Export => {
                let Some(folder) = self.folder.clone() else {
                    return self.fail(text::folder_missing(), cx);
                };
                let destination = free_path(&folder, &text::export_folder_name(&today()));
                (
                    Command::Backup {
                        destination: destination.clone(),
                        recovery_key: self.key.clone(),
                    },
                    destination,
                )
            }
            Direction::Import => {
                let key = match self.import_key(cx) {
                    Ok(key) => key,
                    Err(hint) => return self.fail(hint, cx),
                };
                let Some(source) = self.folder.clone() else {
                    return self.fail(text::export_missing(), cx);
                };
                let Some(destination) = imported_destination() else {
                    return self.fail(text::no_library_folder(), cx);
                };
                (
                    Command::Restore {
                        source,
                        destination: destination.clone(),
                        recovery_key: key,
                    },
                    destination,
                )
            }
        };
        self.busy = true;
        self.hint = None;
        self.outcome = None;
        cx.notify();
        let service = self.service.clone();
        let rt = self.runtime.clone();
        let direction = self.direction;
        cx.spawn(async move |weak, cx| {
            let result = rt
                .spawn(async move { service.execute(command).await })
                .await;
            let outcome = match joined(result) {
                Ok(_) => match direction {
                    Direction::Export => Outcome::Exported(target),
                    Direction::Import => Outcome::Imported(target),
                },
                Err(failure) => {
                    let reason = failure_text(&failure);
                    Outcome::Failed(match direction {
                        Direction::Export => text::export_failed(&reason),
                        Direction::Import => text::import_failed(&reason),
                    })
                }
            };
            let _ = weak.update(cx, |t, cx| {
                let status = match &outcome {
                    Outcome::Exported(path) => text::exported(&path.display().to_string()),
                    Outcome::Imported(path) => text::imported(&path.display().to_string()),
                    Outcome::Failed(message) => message.clone(),
                };
                let _ = t.owner.update(cx, |v, cx| {
                    v.status = status;
                    cx.notify();
                });
                t.busy = false;
                t.outcome = Some(outcome);
                cx.notify();
            });
        })
        .detach();
    }

    fn step(&self, title: &'static str) -> Div {
        theme::inset()
            .gap(px(10.))
            .child(div().font_weight(FontWeight::EXTRA_BOLD).child(title))
    }

    fn folder_row(&self, cx: &mut Context<Self>) -> Div {
        let (choose, again) = match self.direction {
            Direction::Export => (text::choose_folder(), text::choose_other_folder()),
            Direction::Import => (text::choose_export(), text::choose_other_export()),
        };
        div()
            .flex()
            .flex_col()
            .items_start()
            .gap(px(8.))
            .child(
                theme::meta(
                    self.folder
                        .as_ref()
                        .map_or_else(|| text::no_folder().into(), |f| f.display().to_string()),
                )
                .max_w_full()
                .truncate(),
            )
            .child(theme::pill(
                Button::new("transfer-choose-folder")
                    .icon(IconName::FolderOpen)
                    .label(if self.folder.is_some() { again } else { choose })
                    .on_click(cx.listener(|t, _, _, cx| t.choose_folder(cx))),
                Look::Outline,
                Height::Small,
                self.busy,
                cx,
            ))
    }

    fn key_block(&self, cx: &mut Context<Self>) -> Div {
        match self.direction {
            Direction::Export => {
                let shown = wording::grouped_recovery_key(&self.key);
                let copy = shown.clone();
                self.step(text::export_step_key())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .bg(theme::c(theme::SURFACE))
                                    .rounded(px(12.))
                                    .px(px(12.))
                                    .py(px(10.))
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(theme::c(theme::INK))
                                    .child(shown),
                            )
                            .child(theme::pill(
                                Button::new("transfer-copy-key")
                                    .icon(if self.copied {
                                        IconName::Check
                                    } else {
                                        IconName::Copy
                                    })
                                    .label(if self.copied {
                                        text::copied()
                                    } else {
                                        text::copy()
                                    })
                                    .on_click(cx.listener(move |t, _, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            copy.clone(),
                                        ));
                                        t.copied = true;
                                        cx.notify();
                                    })),
                                Look::Soft(&theme::LILAC),
                                Height::Small,
                                false,
                                cx,
                            )),
                    )
                    .child(theme::note(text::key_reminder()))
            }
            Direction::Import => self
                .step(text::import_step_key())
                .child(theme::small_field(Input::new(&self.key_input))),
        }
    }
}

impl Render for LibraryTransfer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (icon, family, intro) = match self.direction {
            Direction::Export => (IconName::Upload, &theme::LILAC, text::export_intro()),
            Direction::Import => (IconName::Download, &theme::MINT, text::import_intro()),
        };
        let mut body = div().flex().flex_col().gap(px(14.)).child(
            div()
                .flex()
                .items_start()
                .gap(px(12.))
                .child(theme::icon_tile(icon, family, 44., 13.))
                .child(div().flex_1().child(theme::note(intro))),
        );
        body = match self.direction {
            Direction::Export => body
                .child(
                    self.step(text::export_step_folder())
                        .child(self.folder_row(cx)),
                )
                .child(self.key_block(cx)),
            Direction::Import => body.child(self.key_block(cx)).child(
                self.step(text::import_step_folder())
                    .child(self.folder_row(cx)),
            ),
        };
        if let Some(hint) = self.hint {
            body = body.child(problem(hint.to_owned()));
        }
        if self.busy {
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme::c(theme::INK_SOFT))
                    .child(Spinner::new().color(theme::c(family.accent)))
                    .child(match self.direction {
                        Direction::Export => text::exporting(),
                        Direction::Import => text::importing(),
                    }),
            );
        }
        let mut open_imported = None;
        match &self.outcome {
            Some(Outcome::Exported(path)) => {
                body = body.child(done(text::exported(&path.display().to_string())));
            }
            Some(Outcome::Imported(path)) => {
                body = body.child(done(text::imported(&path.display().to_string())));
                let open = path.clone();
                let owner = self.owner.clone();
                open_imported = Some(theme::pill(
                    Button::new("transfer-open-imported")
                        .label(text::open_imported())
                        .on_click(move |_, window, cx| {
                            let _ = owner.update(cx, |v, cx| v.open_profile(open.clone(), cx));
                            window.close_dialog(cx);
                        }),
                    Look::Primary,
                    Height::Medium,
                    false,
                    cx,
                ));
            }
            Some(Outcome::Failed(message)) => body = body.child(problem(message.clone())),
            None => {}
        }
        let finished = matches!(
            self.outcome,
            Some(Outcome::Exported(_) | Outcome::Imported(_))
        );
        let action = (!finished).then(|| {
            theme::pill(
                Button::new("transfer-run")
                    .label(match self.direction {
                        Direction::Export => text::export(),
                        Direction::Import => text::import(),
                    })
                    .loading(self.busy)
                    .on_click(cx.listener(|t, _, _, cx| t.run(cx))),
                Look::Primary,
                Height::Medium,
                self.busy || self.folder.is_none(),
                cx,
            )
        });
        body.child(
            div()
                .flex()
                .justify_end()
                .gap(px(8.))
                .child(DialogClose::new().child(theme::pill(
                    Button::new("transfer-close").label(text::close()),
                    Look::Outline,
                    Height::Medium,
                    false,
                    cx,
                )))
                .children(action)
                .children(open_imported),
        )
    }
}

fn problem(message: String) -> Div {
    theme::inset().bg(theme::c(theme::ROSE.soft)).child(
        div()
            .text_color(theme::c(theme::ROSE.ink))
            .font_weight(FontWeight::BOLD)
            .child(message),
    )
}

fn done(message: String) -> Div {
    theme::inset()
        .bg(theme::c(theme::MINT.soft))
        .flex_row()
        .items_start()
        .gap(px(10.))
        .child(theme::done_mark(22.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .font_weight(FontWeight::BOLD)
                .text_color(theme::c(theme::MINT.ink))
                .child(message),
        )
}

/// A fresh random recovery key: 32 random bytes, as the backup format expects.
fn new_recovery_key() -> String {
    use rand::RngCore;
    let mut bytes = [0; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The export inside `folder`: the folder itself, or the only export directly in it when
/// the parent picked the folder that holds it.
fn find_export(folder: &std::path::Path) -> Option<std::path::PathBuf> {
    const MANIFEST: &str = "manifest.ybk";
    if folder.join(MANIFEST).is_file() {
        return Some(folder.to_owned());
    }
    let mut found = std::fs::read_dir(folder)
        .ok()?
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.join(MANIFEST).is_file());
    let only = found.next()?;
    found.next().is_none().then_some(only)
}

/// `name` in `folder`, numbered « (2) », « (3) »… when that name is taken.
fn free_path(folder: &std::path::Path, name: &str) -> std::path::PathBuf {
    let mut path = folder.join(name);
    let mut number = 2;
    while path.exists() {
        path = folder.join(format!("{name} ({number})"));
        number += 1;
    }
    path
}

/// Where an imported library goes: next to the library this window uses.
fn imported_destination() -> Option<std::path::PathBuf> {
    let current = yoto_infra::default_profile().ok()?;
    let parent = current.parent()?;
    let library = current.file_name()?.to_string_lossy().into_owned();
    Some(free_path(
        parent,
        &text::imported_folder_name(&library, &today()),
    ))
}

/// Today's date as « 2026-10-13 », in UTC, for folder names.
fn today() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    date_text(seconds)
}

/// « AAAA-MM-JJ » for a time in seconds since 1970 (civil calendar, UTC).
fn date_text(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64 + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[core::prelude::v1::test]
    fn dates_read_as_calendar_days() {
        assert_eq!(date_text(0), "1970-01-01");
        assert_eq!(date_text(951_782_400), "2000-02-29");
        assert_eq!(date_text(1_791_849_600), "2026-10-13");
        assert_eq!(date_text(1_798_761_599), "2026-12-31");
    }
    #[core::prelude::v1::test]
    fn a_new_recovery_key_is_accepted_back() {
        let key = new_recovery_key();
        assert_eq!(key.len(), 64);
        assert!(wording::check_recovery_key(&key).is_ok());
        let typed = wording::grouped_recovery_key(&key);
        assert_eq!(wording::normalize_recovery_key(&typed), key);
    }
    /// An empty folder of its own, removed when dropped.
    struct Scratch(std::path::PathBuf);
    impl Scratch {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "yoto-transfer-{name}-{}-{}",
                std::process::id(),
                new_recovery_key()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[core::prelude::v1::test]
    fn an_export_is_found_in_its_folder_or_the_one_above() {
        let root = Scratch::new("find");
        assert_eq!(find_export(&root.0), None);
        let export = root.0.join("Médiathèque Yoto 2026-10-13");
        std::fs::create_dir(&export).unwrap();
        std::fs::write(export.join("manifest.ybk"), b"x").unwrap();
        assert_eq!(find_export(&export), Some(export.clone()));
        assert_eq!(find_export(&root.0), Some(export.clone()));
        let second = root.0.join("Médiathèque Yoto 2026-10-14");
        std::fs::create_dir(&second).unwrap();
        std::fs::write(second.join("manifest.ybk"), b"x").unwrap();
        assert_eq!(find_export(&root.0), None);
    }
    #[core::prelude::v1::test]
    fn taken_names_are_numbered() {
        let root = Scratch::new("names");
        assert_eq!(free_path(&root.0, "A"), root.0.join("A"));
        std::fs::create_dir(root.0.join("A")).unwrap();
        assert_eq!(free_path(&root.0, "A"), root.0.join("A (2)"));
    }
}
