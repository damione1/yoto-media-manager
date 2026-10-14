use super::*;
use gpui_kit::component::{
    Icon,
    menu::{DropdownMenu, PopupMenuItem},
};
use gpui_kit::prelude::FluentBuilder;
use theme::{Height, Look};
impl AppView {
    fn preview_playing(&self, media: MediaId, cx: &App) -> bool {
        self.trims
            .get(&media)
            .is_some_and(|panel| panel.read(cx).is_playing())
    }
}

/// Where an audio came from, as a parent names it.
pub fn source_label(media: &MediaItem) -> &'static str {
    match media.provenance {
        Provenance::Youtube { .. } => "YouTube",
        Provenance::LocalFile { .. } => "Fichier audio",
    }
}
/// Duration, then the author when known, otherwise the source.
fn media_meta(media: &MediaItem) -> String {
    let duration = duration_text(media.info.duration.seconds());
    if media.author.trim().is_empty() {
        format!("{duration} · {}", source_label(media))
    } else {
        format!("{duration} · {}", media.author)
    }
}
/// The version a playlist receives by default: the latest finished cut, else the original.
fn default_recipe(snapshot: &Snapshot, media: &MediaItem) -> Option<RecipeId> {
    media
        .recipes
        .iter()
        .rev()
        .find(|id| {
            snapshot
                .recipes
                .iter()
                .any(|recipe| recipe.id == **id && recipe.result.is_some())
        })
        .or_else(|| media.recipes.first())
        .copied()
}

pub fn render(view: &mut AppView, _window: &mut Window, cx: &mut Context<AppView>) -> AnyElement {
    let mut list = gpui_kit::uniform_list(
        "library-list",
        view.library_rows.len(),
        cx.processor(
            |view: &mut AppView,
             range: std::ops::Range<usize>,
             _window,
             cx: &mut Context<AppView>| {
                let mut rows = Vec::new();
                for index in range {
                    let media = &view.snapshot.media[view.library_rows[index]];
                    let id = media.id;
                    let selected = view.selected_media_set.contains(&id);
                    let open = view.selected_media == Some(id);
                    let trimmed = media.recipes.len() > 1;
                    rows.push(
                        div().h(px(76.)).pb(px(8.)).child(
                            div()
                                .h_full()
                                .px(px(12.))
                                .rounded(px(18.))
                                .bg(theme::c(theme::SURFACE))
                                .border_2()
                                .border_color(theme::c(if open {
                                    theme::PEACH.accent
                                } else {
                                    theme::SURFACE
                                }))
                                .when(!open, |row| row.shadow(theme::step(theme::BORDER, 2.)))
                                .flex()
                                .gap(px(14.))
                                .items_center()
                                .child(
                                    gpui_kit::component::checkbox::Checkbox::new(element_id(
                                        "select-media",
                                        id,
                                    ))
                                    .large()
                                    .accessibility_label(format!("Sélectionner {}", media.title))
                                    .checked(selected)
                                    .on_click(cx.listener(
                                        move |v, checked, _, cx| {
                                            if *checked {
                                                v.selected_media_set.insert(id);
                                            } else {
                                                v.selected_media_set.remove(&id);
                                            }
                                            cx.notify();
                                        },
                                    )),
                                )
                                .child(theme::thumbnail(&id.to_string(), 48., 14.))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .child(theme::heading(media.title.clone(), 17.).truncate())
                                        .child(theme::meta(media_meta(media)).truncate()),
                                )
                                .child(if trimmed {
                                    theme::tag("Découpé", &theme::SKY)
                                } else {
                                    theme::tag_colors("Original", theme::SAND, theme::BROWN)
                                })
                                .child(theme::round(
                                    Button::new(element_id("open-media", id.to_string()))
                                        .icon(if open && view.preview_playing(id, cx) {
                                            IconName::Pause
                                        } else {
                                            IconName::Play
                                        })
                                        .accessibility_label(format!("Écouter {}", media.title))
                                        .tooltip(if open {
                                            "Écouter"
                                        } else {
                                            "Ouvrir et écouter"
                                        })
                                        .on_click(cx.listener(move |v, _, window, cx| {
                                            if v.selected_media == Some(id)
                                                && let Some(panel) = v.trims.get(&id).cloned()
                                            {
                                                panel.update(cx, |p, cx| p.toggle_play(cx));
                                            } else {
                                                v.select_media(id, window, cx);
                                            }
                                        })),
                                    Look::Soft(&theme::PEACH),
                                    40.,
                                    false,
                                    cx,
                                )),
                        ),
                    );
                }
                rows
            },
        ),
    )
    .flex_1()
    .min_h_0()
    .into_any_element();
    if view.snapshot.media.is_empty() {
        list = div()
            .flex_1()
            .child(
                theme::card()
                    .p(px(28.))
                    .gap(px(14.))
                    .items_start()
                    .child(theme::thumbnail("bienvenue", 64., 18.))
                    .child(theme::heading("Commencez votre médiathèque", 24.))
                    .child(
                        div().text_color(theme::c(theme::INK_SOFT)).child(
                            "Ajoutez un fichier audio ou un lien YouTube. Vous pourrez ensuite écouter, découper, puis ajouter l’audio à une playlist Yoto.",
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(10.))
                            .child(theme::pill(
                                Button::new("first-file")
                                    .icon(IconName::Plus)
                                    .label("Ajouter un fichier audio")
                                    .on_click(cx.listener(|v, _, _, cx| v.add_files(cx))),
                                Look::Primary,
                                Height::Large,
                                false,
                                cx,
                            ))
                            .child(theme::pill(
                                Button::new("first-youtube")
                                    .icon(IconName::Link)
                                    .label("Importer depuis un lien YouTube")
                                    .on_click(cx.listener(|v, _, _, cx| {
                                        v.navigate(Route::Imports, cx);
                                    })),
                                Look::Outline,
                                Height::Large,
                                false,
                                cx,
                            )),
                    ),
            )
            .into_any_element();
    } else if view.library_rows.is_empty() {
        list = div()
            .flex_1()
            .pt(px(12.))
            .child(theme::note(
                "Aucun audio ne correspond à votre recherche. Essayez un autre mot.",
            ))
            .into_any_element();
    }
    let panel = view
        .selected_media
        .and_then(|id| view.trims.get(&id))
        .map(|panel| {
            div()
                .w(px(340.))
                .flex_shrink_0()
                .h_full()
                .border_l_2()
                .border_color(theme::c(theme::DIVIDER))
                .child(panel.clone())
                .into_any_element()
        });
    let count = view.snapshot.media.len();
    let header = div()
        .flex()
        .items_center()
        .gap(px(12.))
        .child(div().flex_1().min_w_0().child(theme::screen_header(
            "Médiathèque",
            if count == 1 {
                "1 audio · sur cet ordinateur".to_owned()
            } else {
                format!("{count} audios · sur cet ordinateur")
            },
        )))
        .child(theme::pill(
            Button::new("add-youtube")
                .icon(IconName::Link)
                .label("Lien YouTube")
                .on_click(cx.listener(|v, _, _, cx| v.navigate(Route::Imports, cx))),
            Look::Outline,
            Height::Large,
            false,
            cx,
        ))
        .child(theme::pill(
            Button::new("add-file")
                .icon(IconName::Plus)
                .label("Ajouter un fichier audio")
                .on_click(cx.listener(|v, _, _, cx| v.add_files(cx))),
            Look::Primary,
            Height::Large,
            false,
            cx,
        ));
    div()
        .size_full()
        .flex()
        .child(
            div()
                .relative()
                .flex_1()
                .min_w_0()
                .h_full()
                .pt(px(24.))
                .px(px(28.))
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(header)
                .children((count > 0).then(|| library_controls(view, cx)))
                .child(list)
                .children(selection_bar(view, cx)),
        )
        .children(panel)
        .into_any_element()
}

fn sort_label(sort: CatalogueSort) -> &'static str {
    match sort {
        CatalogueSort::Title => "Titre",
        CatalogueSort::Author => "Auteur",
        CatalogueSort::Duration => "Durée",
        CatalogueSort::RecentlyImported => "Plus récents",
    }
}
fn library_controls(view: &AppView, cx: &mut Context<AppView>) -> AnyElement {
    let more = cx.entity().downgrade();
    let current = view.sort;
    let sorts = [
        CatalogueSort::Title,
        CatalogueSort::RecentlyImported,
        CatalogueSort::Duration,
        CatalogueSort::Author,
    ]
    .map(|sort| {
        theme::segment(
            Button::new(element_id("sort-library", sort_label(sort)))
                .label(sort_label(sort))
                .accessibility_label(format!("Trier par : {}", sort_label(sort)))
                .on_click(cx.listener(move |v, _, _, cx| {
                    v.sort = sort;
                    v.filter_library(cx);
                    cx.notify();
                })),
            current == sort,
            Height::Small,
            cx,
        )
    });
    div()
        .flex()
        .flex_wrap()
        .gap(px(12.))
        .items_center()
        .child(
            theme::field(Input::new(&view.search))
                .flex_1()
                .min_w(px(240.))
                .prefix(
                    Icon::new(IconName::Search)
                        .size(px(18.))
                        .text_color(theme::c(theme::MUTED_WARM)),
                ),
        )
        .child(theme::segmented().children(sorts))
        .child(
            theme::round(
                Button::new("library-more")
                    .icon(IconName::Ellipsis)
                    .accessibility_label("Plus d’options")
                    .tooltip("Plus d’options"),
                Look::Ghost,
                44.,
                false,
                cx,
            )
            .dropdown_menu(move |menu, _, _| {
                let select = more.clone();
                let clean = more.clone();
                menu.item(
                    PopupMenuItem::new("Tout sélectionner").on_click(move |_, _, cx| {
                        let _ = select.update(cx, |v, cx| {
                            v.selected_media_set = v
                                .library_rows
                                .iter()
                                .map(|i| v.snapshot.media[*i].id)
                                .collect();
                            cx.notify();
                        });
                    }),
                )
                .separator()
                .item(
                    PopupMenuItem::new("Libérer l’espace inutilisé").on_click(move |_, _, cx| {
                        let _ = clean.update(cx, |v, cx| v.send(Command::CollectGarbage, cx));
                    }),
                )
            }),
        )
        .into_any_element()
}

/// The dark bar floating over the list: what is selected and what can be done with it.
/// Without a ticked audio it offers to send the open audio to the open playlist.
fn selection_bar(view: &AppView, cx: &mut Context<AppView>) -> Option<AnyElement> {
    let playlist = view
        .selected_playlist
        .and_then(|id| view.snapshot.playlists.iter().find(|p| p.id == id))
        .filter(|playlist| view.editors.contains_key(&playlist.id));
    let ticked = view.selected_media_set.len();
    let open = view
        .selected_media
        .and_then(|id| view.snapshot.media.iter().find(|m| m.id == id));
    if ticked == 0 && (playlist.is_none() || open.is_none()) {
        return None;
    }
    let label = match (ticked, open) {
        (0, Some(media)) => format!("« {} »", media.title),
        (1, _) => "1 audio sélectionné".to_owned(),
        (count, _) => format!("{count} audios sélectionnés"),
    };
    let mut bar = div()
        .absolute()
        .left(px(28.))
        .right(px(28.))
        .bottom(px(20.))
        .h(px(60.))
        .rounded(px(20.))
        .bg(theme::c(theme::INK))
        .text_color(theme::c(theme::SURFACE))
        .shadow(theme::float(0.25))
        .flex()
        .items_center()
        .gap(px(10.))
        .pl(px(20.))
        .pr(px(10.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_weight(FontWeight::EXTRA_BOLD)
                .child(label),
        );
    if ticked > 0 {
        bar = bar
            .child(theme::pill(
                Button::new("clear-media-selection")
                    .label("Désélectionner")
                    .on_click(cx.listener(|v, _, _, cx| {
                        v.selected_media_set.clear();
                        cx.notify();
                    })),
                Look::OnDark,
                Height::Medium,
                false,
                cx,
            ))
            .child(theme::pill(
                Button::new("delete-selected-media")
                    .label("Supprimer")
                    .on_click(cx.listener(|v, _, window, cx| {
                        let count = v.selected_media_set.len();
                        let owner = cx.entity().downgrade();
                        confirm_destructive(
                            window,
                            cx,
                            format!("Supprimer {count} média(s) de la médiathèque ?"),
                            "Les fichiers audio et leurs versions préparées seront effacés de cet ordinateur. Les médias encore utilisés dans une playlist ou par une tâche en cours seront conservés. Cette action est irréversible.",
                            "Supprimer",
                            move |_, cx| {
                                let _ = owner.update(cx, |v, cx| {
                                    v.send(
                                        Command::DeleteMediaBatch {
                                            media: v.selected_media_set.iter().copied().collect(),
                                            policy: ReferencePolicy::RejectReferenced,
                                        },
                                        cx,
                                    );
                                });
                            },
                        );
                    })),
                Look::OnDarkDanger,
                Height::Medium,
                false,
                cx,
            ));
    }
    if let Some(playlist) = playlist {
        let id = playlist.id;
        let blocked = view.editors.get(&id).is_none_or(|e| e.read(cx).blocked());
        bar = bar.child(theme::pill(
            Button::new("add-selected-audio-to-playlist")
                .label(format!("Ajouter à « {} »", playlist.title))
                .on_click(cx.listener(move |v, _, window, cx| {
                    let chosen: Vec<_> = if v.selected_media_set.is_empty() {
                        v.selected_media.into_iter().collect()
                    } else {
                        v.library_rows
                            .iter()
                            .map(|i| v.snapshot.media[*i].id)
                            .filter(|id| v.selected_media_set.contains(id))
                            .collect()
                    };
                    let batch: Vec<_> = chosen
                        .into_iter()
                        .filter_map(|media| {
                            let item = v.snapshot.media.iter().find(|m| m.id == media)?;
                            // The open audio keeps the version chosen in its panel.
                            let recipe = v
                                .trims
                                .get(&media)
                                .and_then(|p| p.read(cx).chosen_recipe())
                                .or_else(|| default_recipe(&v.snapshot, item))?;
                            Some((media, recipe))
                        })
                        .collect();
                    v.select_playlist(id, window, cx);
                    if let Some(editor) = v.editors.get(&id).cloned() {
                        cx.defer(move |cx| {
                            editor.update(cx, |e, cx| e.add_media_batch(batch, cx));
                        });
                        v.selected_media_set.clear();
                        v.navigate(Route::Playlists, cx);
                    }
                })),
            Look::Primary,
            Height::Medium,
            blocked,
            cx,
        ));
    }
    Some(bar.into_any_element())
}
