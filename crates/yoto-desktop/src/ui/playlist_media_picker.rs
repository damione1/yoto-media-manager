use super::*;
use gpui_kit::component::{
    checkbox::Checkbox,
    menu::{DropdownMenu, PopupMenuItem},
};
use std::collections::{BTreeMap, BTreeSet};
struct VersionChoice {
    id: RecipeId,
    /// The whole audio, or a trimmed part of it.
    original: bool,
    trim: TrimRange,
    normalization: Normalization,
}
impl VersionChoice {
    /// Worded when shown, so it follows the interface language.
    fn label(&self) -> String {
        let duration = duration_text(self.trim.duration().seconds());
        if self.original {
            return tr!("Original · {duration}", duration = duration);
        }
        let start = wording::position_text(self.trim.start().seconds());
        let end = wording::position_text(self.trim.end().seconds());
        match self.normalization {
            Normalization::Preserve => tr!(
                "Découpe {start} → {end} ({duration})",
                start = start,
                end = end,
                duration = duration
            ),
            Normalization::Loudness => tr!(
                "Découpe {start} → {end} ({duration}) · volume égalisé",
                start = start,
                end = end,
                duration = duration
            ),
        }
    }
}
struct MediaChoice {
    id: MediaId,
    title: String,
    /// The icon its chapter and tracks will get.
    icon: Option<IconReference>,
    versions: Vec<VersionChoice>,
    default: RecipeId,
}
/// Immutable worker-built catalogue shared by every picker; only typed IDs and presentation values.
pub(crate) struct MediaChoices {
    rows: Vec<MediaChoice>,
    positions: BTreeMap<MediaId, usize>,
}
pub(super) struct ChapterChoice {
    pub(super) id: ChapterId,
    pub(super) title: String,
    pub(super) tracks: usize,
}
pub(in crate::ui) fn prepare_media_choices(snapshot: &Snapshot) -> Arc<MediaChoices> {
    let rows = choices(snapshot);
    let positions = rows
        .iter()
        .enumerate()
        .map(|(index, row)| (row.id, index))
        .collect();
    Arc::new(MediaChoices { rows, positions })
}
fn choices(snapshot: &Snapshot) -> Vec<MediaChoice> {
    let recipes: BTreeMap<_, _> = snapshot.recipes.iter().map(|r| (r.id, r)).collect();
    snapshot
        .media
        .iter()
        .filter_map(|media| {
            let original = media.recipes.first().copied()?;
            let usable: Vec<_> = media
                .recipes
                .iter()
                .filter_map(|id| recipes.get(id).copied())
                .filter(|recipe| recipe.id == original || recipe.result.is_some())
                .collect();
            let default = usable
                .iter()
                .rev()
                .find(|recipe| recipe.result.is_some())
                .map(|r| r.id)
                .unwrap_or(original);
            let versions = usable
                .into_iter()
                .map(|recipe| VersionChoice {
                    id: recipe.id,
                    original: recipe.id == original,
                    trim: recipe.trim,
                    normalization: recipe.normalization,
                })
                .collect::<Vec<_>>();
            if versions.is_empty() {
                return None;
            }
            Some(MediaChoice {
                id: media.id,
                title: media.title.clone(),
                icon: media.icon.clone(),
                versions,
                default,
            })
        })
        .collect()
}
pub(super) struct MediaPicker {
    editor: WeakEntity<PlaylistEditor>,
    rows: Arc<MediaChoices>,
    selected: BTreeSet<MediaId>,
    versions: BTreeMap<MediaId, RecipeId>,
    _observation: Subscription,
}
impl MediaPicker {
    pub(super) fn new(
        editor: Entity<PlaylistEditor>,
        rows: Arc<MediaChoices>,
        cx: &mut Context<Self>,
    ) -> Self {
        let versions = BTreeMap::new();
        Self {
            editor: editor.downgrade(),
            rows,
            selected: BTreeSet::new(),
            versions,
            _observation: cx.observe(&editor, |_, _, cx| cx.notify()),
        }
    }
}
impl Render for MediaPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (busy, blocked, replacing, status) = self
            .editor
            .upgrade()
            .map(|editor| {
                let e = editor.read(cx);
                (
                    e.busy(),
                    e.blocked(),
                    e.replacement.is_some(),
                    e.status.clone(),
                )
            })
            .unwrap_or((
                false,
                true,
                false,
                tr!("Cette playlist n’est plus ouverte.").into(),
            ));
        let picker = cx.entity().downgrade();
        let rows = gpui_kit::uniform_list(
            "playlist-media-choices",
            self.rows.rows.len(),
            cx.processor(
                move |view: &mut MediaPicker,
                      range: std::ops::Range<usize>,
                      _,
                      cx: &mut Context<MediaPicker>| {
                    let parent = view
                        .editor
                        .upgrade()
                        .and_then(|editor| editor.read(cx).parent.upgrade());
                    range
                        .map(|index| {
                            let row = &view.rows.rows[index];
                            let icon = match &parent {
                                Some(parent) => parent.read(cx).media_icon_tile(
                                    row.id,
                                    row.icon.as_ref(),
                                    48.,
                                    14.,
                                    cx,
                                ),
                                None => theme::empty_icon_slot(48., 14., false),
                            };
                            let media = row.id;
                            let selected = view.selected.contains(&media);
                            let default = row.default;
                            let chosen = view.versions.get(&media).copied().unwrap_or(row.default);
                            let label = row
                                .versions
                                .iter()
                                .find(|version| version.id == chosen)
                                .map(VersionChoice::label)
                                .unwrap_or_else(|| tr!("Choisir une version").into());
                            let weak = picker.clone();
                            let versions = row
                                .versions
                                .iter()
                                .map(|v| (v.id, v.label()))
                                .collect::<Vec<_>>();
                            div()
                                .h(px(96.))
                                .w_full()
                                .flex()
                                .gap_3()
                                .items_center()
                                .border_b_2()
                                .border_color(theme::c(theme::DIVIDER))
                                .child(
                                    Checkbox::new(element_id("choose-media", media))
                                        .large()
                                        .accessibility_label(row.title.clone())
                                        .checked(selected)
                                        .disabled(blocked)
                                        .on_click(cx.listener(move |v, checked, _, cx| {
                                            if *checked {
                                                if replacing {
                                                    v.selected.clear();
                                                }
                                                v.selected.insert(media);
                                                v.versions.entry(media).or_insert(default);
                                            } else {
                                                v.selected.remove(&media);
                                            }
                                            cx.notify();
                                        })),
                                )
                                .child(icon)
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .flex_col()
                                        .items_start()
                                        .gap_2()
                                        .child(theme::heading(row.title.clone(), 17.).truncate())
                                        .child(
                                            Button::new(element_id("choose-version", media))
                                                .label(label)
                                                .small()
                                                .outline()
                                                .rounded(px(999.))
                                                .dropdown_caret(true)
                                                .disabled(blocked)
                                                .dropdown_menu(move |mut menu, _, _| {
                                                    for (id, label) in &versions {
                                                        let weak = weak.clone();
                                                        let id = *id;
                                                        menu = menu.item(
                                                            PopupMenuItem::new(label.clone())
                                                                .checked(chosen == id)
                                                                .on_click(move |_, _, cx| {
                                                                    let _ =
                                                                        weak.update(cx, |v, cx| {
                                                                            v.versions
                                                                                .insert(media, id);
                                                                            cx.notify();
                                                                        });
                                                                }),
                                                        );
                                                    }
                                                    menu.scrollable(true)
                                                }),
                                        ),
                                )
                        })
                        .collect()
                },
            ),
        )
        .h(px(320.))
        .min_h(px(120.));
        let selected = self.selected.len();
        let list = if self.rows.rows.is_empty() {
            div()
                .p_6()
                .child(theme::note(tr!(
                    "Votre médiathèque est vide. Importez un fichier ou une vidéo pour commencer."
                )))
                .into_any_element()
        } else {
            rows.into_any_element()
        };
        let actions = theme::dialog_actions()
            .child(theme::pill(
                Button::new("picker-import-media")
                    .label(tr!("Importer de l’audio"))
                    .on_click(cx.listener(|v, _, window, cx| v.go_to_imports(window, cx))),
                theme::Look::Outline,
                theme::Height::Medium,
                busy,
                cx,
            ))
            .child(div().flex_1())
            .child(theme::dialog_close(
                "picker-cancel-footer",
                theme::pill(
                    Button::new("picker-cancel").label(tr!("Annuler")),
                    theme::Look::Ghost,
                    theme::Height::Medium,
                    busy,
                    cx,
                ),
            ))
            .child(theme::pill(
                Button::new("picker-add-selected")
                    .label(if busy {
                        tr!("Envoi en cours…").into()
                    } else {
                        tr!("Ajouter ({selected})", selected = selected)
                    })
                    .on_click(cx.listener(|v, _, _, cx| v.add_selected(cx))),
                theme::Look::Primary,
                theme::Height::Medium,
                selected == 0 || blocked,
                cx,
            ));
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(theme::note(if replacing {
                tr!("Choisissez le média qui remplacera cette piste.")
            } else {
                tr!("Sélectionnez vos médias et la version à envoyer.")
            }))
            .child(list)
            .child(theme::note(tr!(
                "L’audio est envoyé à Yoto avant d’apparaître dans votre playlist. Pas d’icône ? Choisissez-la dans la Médiathèque, ou plus tard dans la playlist."
            )))
            .child(actions)
            .children(busy.then(|| {
                div()
                    .flex()
                    .gap_2()
                    .items_center()
                    .child(Spinner::new().small())
                    .child(status.clone())
            }))
            .children((!busy && !status.is_empty()).then(|| theme::note(status)))
    }
}
impl MediaPicker {
    /// Close the picker and open « Imports » to bring in new audio.
    fn go_to_imports(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let editor = self.editor.clone();
        window.close_dialog(cx);
        let _ = editor.update(cx, |e, cx| {
            e.picker_window = None;
            let _ = e.parent.update(cx, |p, cx| p.navigate(Route::Imports, cx));
        });
    }
    /// Send the ticked media, in library order, with the version chosen for each.
    fn add_selected(&mut self, cx: &mut Context<Self>) {
        let mut batch = self
            .selected
            .iter()
            .filter_map(|media| self.versions.get(media).map(|recipe| (*media, *recipe)))
            .collect::<Vec<_>>();
        batch.sort_by_key(|(media, _)| self.rows.positions.get(media).copied());
        let _ = self.editor.update(cx, |e, cx| e.add_media_batch(batch, cx));
    }
}
#[cfg(test)]
mod tests {
    use super::choices;
    use yoto_domain::*;
    use yoto_infra::Snapshot;
    #[test]
    fn picker_prefers_latest_completed_recipe_and_excludes_unfinished_cuts() {
        let duration = MediaTime::from_seconds(12.).unwrap();
        let original = RecipeId::new();
        let ready = RecipeId::new();
        let pending = RecipeId::new();
        let media = MediaItem {
            id: MediaId::new(),
            title: "Conte".into(),
            author: String::new(),
            tags: vec![],
            original: AssetId::new(),
            info: AudioInfo {
                duration,
                bytes: 100,
                codec: "mp3".into(),
                sample_rate: 44100,
                channels: 2,
            },
            provenance: Provenance::LocalFile {
                name: "conte.mp3".into(),
            },
            recipes: vec![original, ready, pending],
            icon: None,
            chapters: vec![],
        };
        let media_id = media.id;
        let original_asset = media.original;
        let recipe = |id, result| MediaRecipe {
            id,
            media: media_id,
            original: original_asset,
            trim: TrimRange::new(MediaTime::ZERO, duration, duration).unwrap(),
            normalization: Normalization::Preserve,
            result,
            info: None,
            tool_version: String::new(),
            part: None,
        };
        let snapshot = Snapshot {
            media: vec![media],
            recipes: vec![
                recipe(pending, None),
                recipe(ready, Some(AssetId::new())),
                recipe(original, None),
            ],
            ..Default::default()
        };
        let rows = choices(&snapshot);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].default, ready);
        assert_eq!(
            rows[0].versions.iter().map(|v| v.id).collect::<Vec<_>>(),
            vec![original, ready]
        );
    }
}
