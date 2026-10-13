use super::*;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
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
                    let versions = media.recipes.len().saturating_sub(1);
                    let details = if versions > 0 {
                        format!(
                            "{} · {:.1} Mo · {versions} version(s) préparée(s)",
                            duration_text(media.info.duration.seconds()),
                            media.info.bytes as f64 / 1_000_000.
                        )
                    } else {
                        format!(
                            "{} · {:.1} Mo",
                            duration_text(media.info.duration.seconds()),
                            media.info.bytes as f64 / 1_000_000.
                        )
                    };
                    rows.push(
                        div()
                            .p_4()
                            .border_1()
                            .border_color(if open {
                                cx.theme().primary
                            } else {
                                cx.theme().border
                            })
                            .rounded_lg()
                            .flex()
                            .gap_3()
                            .items_center()
                            .child(
                                gpui_kit::component::checkbox::Checkbox::new(element_id(
                                    "select-media",
                                    id,
                                ))
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
                            .child(
                                div()
                                    .flex_1()
                                    .child(
                                        div()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(media.title.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(details),
                                    ),
                            )
                            .child(
                                Button::new(element_id("open-media", id.to_string()))
                                    .label(if open { "Ouvert" } else { "Écouter" })
                                    .selected(open)
                                    .on_click(cx.listener(move |v, _, window, cx| {
                                        v.select_media(id, window, cx)
                                    })),
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
                div()
                    .p_8()
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().text_xl().child("Commencez votre médiathèque"))
                    .child(
                        "Ajoutez un fichier audio ou un lien YouTube. Vous pourrez ensuite écouter, découper, puis ajouter l’audio à une playlist Yoto.",
                    )
                    .child(
                        Button::new("first-file")
                            .label("Ajouter un fichier audio")
                            .primary()
                            .on_click(cx.listener(|v, _, _, cx| v.add_files(cx))),
                    )
                    .child(
                        Button::new("first-youtube")
                            .label("Importer depuis un lien YouTube")
                            .on_click(cx.listener(|v, _, _, cx| {
                                v.navigate(Route::Imports, cx);
                            })),
                    ),
            )
            .into_any_element();
    }
    let panel = view
        .selected_media
        .and_then(|id| view.trims.get(&id))
        .map(|panel| {
            div()
                .w(px(390.))
                .h_full()
                .border_l_1()
                .border_color(cx.theme().border)
                .child(panel.clone())
                .into_any_element()
        });
    div()
        .size_full()
        .flex()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .p_7()
                .flex()
                .flex_col()
                .gap_4()
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .items_center()
                        .child(
                            div()
                                .text_2xl()
                                .font_weight(FontWeight::BOLD)
                                .child("Médiathèque"),
                        )
                        .child(
                            Button::new("add-file")
                                .label("+ Ajouter")
                                .primary()
                                .on_click(cx.listener(|v, _, _, cx| v.add_files(cx))),
                        ),
                )
                .child(Input::new(&view.search))
                .child(library_controls(view, cx))
                .child(list),
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
    let sorting = cx.entity().downgrade();
    let more = sorting.clone();
    let current = view.sort;
    let mut controls = div()
        .flex()
        .gap_2()
        .items_center()
        .child(
            Button::new("sort-library")
                .label(format!("Trier : {}", sort_label(current)))
                .dropdown_menu(move |mut menu, _, _| {
                    for sort in [
                        CatalogueSort::Title,
                        CatalogueSort::Author,
                        CatalogueSort::Duration,
                        CatalogueSort::RecentlyImported,
                    ] {
                        let parent = sorting.clone();
                        menu = menu.item(
                            PopupMenuItem::new(sort_label(sort))
                                .checked(current == sort)
                                .on_click(move |_, _, cx| {
                                    let _ = parent.update(cx, |v, cx| {
                                        v.sort = sort;
                                        v.filter_library(cx);
                                        cx.notify();
                                    });
                                }),
                        );
                    }
                    menu
                }),
        )
        .child(div().flex_1())
        .child(
            Button::new("library-more")
                .label("⋯")
                .accessibility_label("Plus d’options")
                .tooltip("Plus d’options")
                .ghost()
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
                        PopupMenuItem::new("Libérer l’espace inutilisé").on_click(
                            move |_, _, cx| {
                                let _ =
                                    clean.update(cx, |v, cx| v.send(Command::CollectGarbage, cx));
                            },
                        ),
                    )
                }),
        );
    if let Some(playlist) = view
        .selected_playlist
        .and_then(|id| view.snapshot.playlists.iter().find(|p| p.id == id))
        && let Some(media) = view
            .selected_media
            .and_then(|id| view.snapshot.media.iter().find(|m| m.id == id))
        && let Some(recipe) = view
            .trims
            .get(&media.id)
            .and_then(|p| p.read(cx).chosen_recipe())
            .or_else(|| media.recipes.first().copied())
    {
        let id = playlist.id;
        let media = media.clone();
        controls = controls.child(
            Button::new("add-selected-audio-to-playlist")
                .label("Ajouter à la playlist ouverte")
                .primary()
                .on_click(cx.listener(move |v, _, window, cx| {
                    v.select_playlist(id, window, cx);
                    if let Some(editor) = v.editors.get(&id).cloned() {
                        let handle = window.window_handle();
                        let media = media.clone();
                        cx.defer(move |cx| {
                            let _ = cx.update_window(handle, move |_, window, cx| {
                                editor.update(cx, |e, cx| e.add_media(&media, recipe, window, cx))
                            });
                        });
                        v.navigate(Route::Playlists, cx);
                    }
                })),
        );
    }
    if !view.selected_media_set.is_empty() {
        controls = controls
            .child(
                div()
                    .text_sm()
                    .child(format!("{} sélectionné(s)", view.selected_media_set.len())),
            )
            .child(
                Button::new("delete-selected-media")
                    .label("Supprimer")
                    .on_click(cx.listener(|v, _, window, cx| {
                        let count = v.selected_media_set.len();
                        let owner = cx.entity().downgrade();
                        confirm_destructive(
                            window,
                            cx,
                            format!("Supprimer {count} média(s) de la médiathèque ?"),
                            "Les fichiers audio et leurs versions préparées seront effacés de cet ordinateur. Les médias déjà utilisés dans une playlist seront conservés. Cette action est irréversible.",
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
            )
            .child(
                Button::new("clear-media-selection")
                    .label("Désélectionner")
                    .on_click(cx.listener(|v, _, _, cx| {
                        v.selected_media_set.clear();
                        cx.notify();
                    })),
            );
    }
    controls.into_any_element()
}
