use super::*;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use theme::{Height, Look};

pub fn render(view: &mut AppView, cx: &mut Context<AppView>) -> AnyElement {
    div()
        .id("accounts-scroll")
        .size_full()
        .overflow_y_scroll()
        .py(px(24.))
        .px(px(28.))
        .flex()
        .flex_col()
        .gap(px(18.))
        .child(theme::screen_header(
            tr!("Compte et synchronisation"),
            tr!("Votre collection personnelle, sur cet ordinateur et sur Yoto."),
        ))
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_start()
                .gap(px(16.))
                .child(
                    div()
                        .flex_grow(3.)
                        .flex_basis(px(0.))
                        .min_w(px(420.))
                        .flex()
                        .flex_col()
                        .gap(px(14.))
                        .children(accounts(view, cx)),
                )
                .child(
                    div()
                        .flex_grow(2.)
                        .flex_basis(px(0.))
                        .min_w(px(320.))
                        .flex()
                        .flex_col()
                        .gap(px(14.))
                        .child(language(cx))
                        .child(advanced_mode(view, cx)),
                ),
        )
        .into_any_element()
}

/// First letter of an account label, for its round avatar.
fn initial(label: &str) -> String {
    label
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "Y".into())
}

/// « actualisé il y a 2 min » from the age of the oldest refreshed collection.
fn refreshed_text(age_seconds: u64) -> String {
    match age_seconds {
        0..60 => tr!("actualisé à l’instant").into(),
        60..3600 => tr!("actualisé il y a {minutes} min", minutes = age_seconds / 60),
        3600..86_400 => tr!("actualisé il y a {hours} h", hours = age_seconds / 3600),
        _ => tr!("actualisé il y a {days} j", days = age_seconds / 86_400),
    }
}

/// « Options du compte » round button with « Déconnecter ce compte », confirmed first.
fn account_menu(
    id: &AccountId,
    background: Hsla,
    cx: &mut Context<AppView>,
) -> impl IntoElement + use<> {
    let owner = cx.entity().downgrade();
    let id = id.clone();
    theme::round(
        Button::new(element_id("account-menu", &id))
            .icon(IconName::Ellipsis)
            .accessibility_label(tr!("Options du compte"))
            .tooltip(tr!("Options du compte")),
        Look::Soft(&theme::MINT),
        40.,
        false,
        cx,
    )
    .bg(background)
    .dropdown_menu(move |menu, _, _| {
        let owner = owner.clone();
        let id = id.clone();
        menu.item(
            PopupMenuItem::new(tr!("Déconnecter ce compte")).on_click(move |_, window, cx| {
                let owner = owner.clone();
                let id = id.clone();
                confirm_destructive(
                    window,
                    cx,
                    tr!("Déconnecter ce compte Yoto ?"),
                    tr!("Les envois en cours vers Yoto seront interrompus. Vos playlists restent sur votre compte Yoto et votre médiathèque reste sur cet ordinateur. Vous pourrez vous reconnecter à tout moment."),
                    tr!("Déconnecter"),
                    move |_, cx| {
                        let _ = owner
                            .update(cx, |v, cx| v.send(Command::Disconnect(id.clone()), cx));
                    },
                );
            }),
        )
    })
}

/// Round avatar with the account initial.
fn avatar(label: &str, size: f32, background: u32) -> Div {
    div()
        .size(px(size))
        .flex_shrink_0()
        .rounded_full()
        .bg(theme::c(background))
        .text_color(theme::c(theme::MINT.ink))
        .flex()
        .items_center()
        .justify_center()
        .child(theme::heading(initial(label), (size * 0.43).round()))
}

/// The left column: the active account and its sync state, the other accounts and
/// « Un autre compte Yoto ? ».
fn accounts(view: &AppView, cx: &mut Context<AppView>) -> Vec<AnyElement> {
    let mut column = Vec::new();
    let active = view
        .snapshot
        .active_account
        .as_ref()
        .and_then(|id| view.snapshot.accounts.iter().find(|a| &a.id == id));
    match active {
        Some(account) => column.push(active_card(view, account, cx)),
        None => column.push(
            div()
                .bg(theme::c(theme::MINT.soft))
                .rounded(px(26.))
                .p(px(24.))
                .flex()
                .flex_col()
                .items_start()
                .gap(px(14.))
                .child(theme::icon_tile(
                    IconName::User,
                    &theme::MINT,
                    60.,
                    999.,
                ).bg(theme::c(theme::SURFACE)))
                .child(theme::heading(
                    if view.snapshot.accounts.is_empty() {
                        tr!("Connectez votre compte Yoto")
                    } else {
                        tr!("Choisissez le compte à utiliser")
                    },
                    22.,
                ))
                .child(theme::note(tr!(
                    "Connectez Yoto pour retrouver vos playlists et les envoyer à vos lecteurs. Votre médiathèque reste disponible hors ligne."
                )))
                .children(view.snapshot.accounts.is_empty().then(|| {
                    div()
                        .flex()
                        .gap(px(8.))
                        .items_center()
                        .child(theme::pill(
                            Button::new("login")
                                .label(if view.connecting {
                                    wording::connect::waiting()
                                } else {
                                    tr!("Connecter mon compte Yoto")
                                })
                                .loading(view.connecting)
                                .on_click(cx.listener(|v, _, _, cx| v.send(Command::Connect, cx))),
                            Look::Primary,
                            Height::Large,
                            view.connecting,
                            cx,
                        ))
                        .children(
                            view.connecting
                                .then(|| cancel_connect("login-cancel", Height::Large, cx)),
                        )
                }))
                .into_any_element(),
        ),
    }
    for account in &view.snapshot.accounts {
        if Some(&account.id) == view.snapshot.active_account.as_ref() {
            continue;
        }
        let select = account.id.clone();
        column.push(
            theme::card()
                .flex_row()
                .items_center()
                .py(px(16.))
                .child(avatar(&account.label, 44., theme::MINT.soft))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .font_weight(FontWeight::EXTRA_BOLD)
                                .truncate()
                                .child(account.label.clone()),
                        )
                        .child(theme::meta(tr!("Connecté"))),
                )
                .child(theme::pill(
                    Button::new(element_id("choose-account", &account.id))
                        .label(tr!("Utiliser ce compte"))
                        .on_click(cx.listener(move |v, _, _, cx| {
                            v.send(Command::SelectAccount(select.clone()), cx)
                        })),
                    Look::Soft(&theme::MINT),
                    Height::Medium,
                    false,
                    cx,
                ))
                .child(account_menu(&account.id, theme::c(theme::SAND), cx))
                .into_any_element(),
        );
    }
    if !view.snapshot.accounts.is_empty() {
        column.push(
            theme::card()
                .flex_row()
                .items_center()
                .px(px(20.))
                .py(px(18.))
                .child(
                    div()
                        .size(px(44.))
                        .flex_shrink_0()
                        .rounded_full()
                        .border_2()
                        .border_dashed()
                        .border_color(theme::c(theme::BOX_BORDER))
                        .text_color(theme::c(theme::MUTED_WARM))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            gpui_kit::component::Icon::new(IconName::Plus)
                                .size(px(18.))
                                .text_color(theme::c(theme::MUTED_WARM)),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .font_weight(FontWeight::EXTRA_BOLD)
                                .child(tr!("Un autre compte Yoto ?")),
                        )
                        .child(theme::meta(tr!("Par exemple celui des grands-parents."))),
                )
                .child(theme::pill(
                    Button::new("login")
                        .label(if view.connecting {
                            wording::connect::waiting()
                        } else {
                            tr!("Ajouter un compte")
                        })
                        .loading(view.connecting)
                        .on_click(cx.listener(|v, _, _, cx| v.send(Command::Connect, cx))),
                    Look::Soft(&theme::PEACH),
                    Height::Medium,
                    view.connecting,
                    cx,
                ))
                .children(
                    view.connecting
                        .then(|| cancel_connect("login-cancel", Height::Medium, cx)),
                )
                .into_any_element(),
        );
    }
    column
}

/// Mint card of the account used for synchronisation, with the Yoto data state.
fn active_card(view: &AppView, account: &Account, cx: &mut Context<AppView>) -> AnyElement {
    let snapshot = &view.snapshot;
    let syncing = remote_syncing(snapshot);
    let failures = snapshot
        .remote_sync
        .values()
        .filter(|state| matches!(state, RemoteSyncState::Failed { .. }))
        .count();
    let complete = snapshot.remote_sync.len() == RemoteCollection::ALL.len();
    let summary: String = if syncing {
        tr!("Actualisation en cours…").into()
    } else if failures > 0 {
        trn!(
            "{count} élément à actualiser. Les données déjà récupérées restent disponibles.",
            "{count} éléments à actualiser. Les données déjà récupérées restent disponibles.",
            failures
        )
    } else if complete {
        tr!("Vos playlists et lecteurs sont à jour.").into()
    } else {
        tr!("Vos playlists se chargent automatiquement. Actualisez pour retrouver aussi vos lecteurs.")
            .into()
    };
    let mut detail = vec![
        trn!(
            "{count} playlist",
            "{count} playlists",
            snapshot.remote.len()
        ),
        trn!(
            "{count} lecteur",
            "{count} lecteurs",
            snapshot.devices.len()
        ),
    ];
    let oldest = snapshot
        .remote_sync
        .values()
        .filter_map(|state| match state {
            RemoteSyncState::Ready { synced } => Some(*synced),
            RemoteSyncState::Syncing | RemoteSyncState::Failed { .. } => None,
        })
        .min();
    if let Some(synced) = oldest
        && !syncing
    {
        detail.push(refreshed_text(yoto_infra::now().saturating_sub(synced)));
    }
    let mark = if syncing {
        theme::icon_tile(IconName::RefreshCw, &theme::SKY, 28., 999.)
    } else if failures > 0 {
        theme::icon_tile(IconName::TriangleAlert, &theme::BUTTER, 28., 999.)
    } else if complete {
        theme::done_mark(28.)
    } else {
        theme::icon_tile(IconName::RefreshCw, &theme::MINT, 28., 999.)
    };
    let sync_id = account.id.clone();
    let mut card = div()
        .bg(theme::c(theme::MINT.soft))
        .rounded(px(26.))
        .p(px(24.))
        .flex()
        .flex_col()
        .gap(px(16.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(14.))
                .child(avatar(&account.label, 60., theme::SURFACE))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(theme::heading(account.label.clone(), 22.).truncate())
                        .child(
                            div()
                                .text_size(px(14.))
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme::c(theme::MINT.ink))
                                .child(tr!("Connecté · utilisé pour la synchronisation")),
                        ),
                )
                .child(account_menu(
                    &account.id,
                    theme::c(theme::SURFACE).opacity(0.7),
                    cx,
                )),
        )
        .child(
            div()
                .bg(theme::c(theme::SURFACE))
                .rounded(px(18.))
                .px(px(16.))
                .py(px(14.))
                .flex()
                .items_center()
                .gap(px(12.))
                .child(mark)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(div().font_weight(FontWeight::EXTRA_BOLD).child(summary))
                        .child(theme::meta(detail.join(" · "))),
                )
                .child(theme::pill(
                    Button::new("account-sync")
                        .label(if syncing {
                            tr!("Actualisation…")
                        } else {
                            tr!("Actualiser")
                        })
                        .loading(syncing)
                        .on_click(cx.listener(move |v, _, _, cx| {
                            v.send(Command::Sync(sync_id.clone()), cx)
                        })),
                    Look::Outline,
                    Height::Small,
                    syncing,
                    cx,
                )),
        )
        .child(
            theme::pill(
                Button::new("account-sync-details")
                    .label(if view.account_sync_details_open {
                        tr!("Masquer le détail")
                    } else {
                        tr!("Voir le détail")
                    })
                    .on_click(cx.listener(|v, _, _, cx| {
                        v.account_sync_details_open = !v.account_sync_details_open;
                        cx.notify();
                    })),
                Look::Link(&theme::MINT),
                Height::Tiny,
                false,
                cx,
            )
            .px(px(4.)),
        );
    if view.account_sync_details_open {
        card = card.child(
            div()
                .bg(theme::c(theme::SURFACE).opacity(0.7))
                .rounded(px(16.))
                .p(px(14.))
                .flex()
                .flex_col()
                .gap(px(6.))
                .children(RemoteCollection::ALL.iter().map(|collection| {
                    theme::note(remote_sync_message(snapshot, *collection))
                        .text_color(theme::c(theme::INK_SOFT))
                })),
        );
    }
    card.into_any_element()
}

/// « Langue / Language »: the interface language, applied at once. The title and the
/// language names stay the same in every language, so anyone can find theirs.
fn language(cx: &mut Context<AppView>) -> AnyElement {
    use crate::i18n::Language;
    let current = crate::i18n::language();
    let choices = Language::ALL.map(|language| {
        theme::segment(
            Button::new(element_id("language", language.code()))
                .label(language.native_name())
                .on_click(cx.listener(move |v, _, window, cx| {
                    if crate::i18n::language() != language {
                        v.set_language(language, window, cx);
                    }
                })),
            current == language,
            Height::Small,
            cx,
        )
    });
    theme::card()
        .p(px(22.))
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(10.))
                .child(theme::icon_tile(IconName::Globe, &theme::SKY, 40., 12.))
                .child(
                    div()
                        .flex_1()
                        .child(theme::heading("Langue / Language", 20.)),
                )
                .child(theme::segmented().children(choices)),
        )
        .child(theme::note(tr!(
            "Le changement s’applique tout de suite à toute l’application."
        )))
        .into_any_element()
}

/// The « Mode avancé » switch: technical settings stay out of sight until a parent asks.
fn advanced_mode(view: &AppView, cx: &mut Context<AppView>) -> AnyElement {
    use gpui_kit::component::switch::Switch;
    let card = theme::card()
        .p(px(22.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(theme::icon_tile(IconName::Settings2, &theme::STONE, 40., 12.))
                .child(div().flex_1().child(theme::heading(tr!("Mode avancé"), 20.)))
                .child(
                    Switch::new("advanced-mode")
                        .accessibility_label(tr!("Mode avancé"))
                        .checked(view.advanced)
                        .on_click(cx.listener(|v, checked, _, cx| v.set_advanced(*checked, cx))),
                ),
        )
        .child(theme::note(tr!(
            "Affiche les réglages techniques : lecture interactive, événements des pistes, plages aléatoires et flux audio dans les playlists ; valeurs Yoto, raccourcis et commandes techniques des lecteurs ; ouverture d’une autre médiathèque. Inutile au quotidien."
        )));
    if view.advanced {
        card.child(youtube_tool(view, cx))
            .child(other_library(cx))
            .into_any_element()
    } else {
        card.into_any_element()
    }
}

/// « Autre médiathèque »: open another library of this computer, for example one
/// imported earlier, in a new window.
fn other_library(cx: &mut Context<AppView>) -> AnyElement {
    use wording::other_library as text;
    theme::inset()
        .gap(px(8.))
        .child(
            div()
                .font_weight(FontWeight::EXTRA_BOLD)
                .child(text::title()),
        )
        .child(theme::note(text::note()))
        .child(theme::pill(
            Button::new("choose-profile")
                .label(text::open())
                .on_click(cx.listener(|v, _, _, cx| v.choose_profile(cx))),
            Look::Outline,
            Height::Medium,
            false,
            cx,
        ))
        .into_any_element()
}

/// The YouTube tool imports use, with a button to update it without waiting.
fn youtube_tool(view: &AppView, cx: &mut Context<AppView>) -> AnyElement {
    use wording::youtube_tool as text;
    use yoto_infra::youtube_tool::{ToolActivity, YoutubeToolAction};
    let checking = view
        .youtube_tool
        .as_ref()
        .is_some_and(|s| s.activity == ToolActivity::Checking);
    let mut section = theme::inset()
        .gap(px(8.))
        .child(
            div()
                .font_weight(FontWeight::EXTRA_BOLD)
                .child(text::title()),
        )
        .child(theme::note(text::note()));
    if let Some(status) = &view.youtube_tool {
        section = section.child(div().text_size(px(14.)).child(text::version(status)));
        if let Some(activity) = text::activity(&status.activity) {
            section = section.child(theme::note(activity));
        }
    }
    section
        .child(theme::pill(
            Button::new("update-youtube-tool")
                .label(text::update())
                .disabled(checking)
                .on_click(cx.listener(|v, _, _, cx| v.youtube_tool(YoutubeToolAction::Update, cx))),
            Look::Outline,
            Height::Medium,
            checking,
            cx,
        ))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[core::prelude::v1::test]
    fn refresh_age_reads_naturally() {
        assert_eq!(refreshed_text(12), "actualisé à l’instant");
        crate::i18n::tests::with_language(crate::i18n::Language::English, || {
            assert_eq!(refreshed_text(150), "refreshed 2 min ago");
        });
        assert_eq!(refreshed_text(150), "actualisé il y a 2 min");
        assert_eq!(refreshed_text(7_300), "actualisé il y a 2 h");
        assert_eq!(refreshed_text(200_000), "actualisé il y a 2 j");
    }
    #[core::prelude::v1::test]
    fn avatar_initial_skips_symbols() {
        assert_eq!(initial("« famille Collin »"), "F");
        assert_eq!(initial("—"), "Y");
    }
}
