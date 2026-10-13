use super::*;
use gpui_kit::component::{
    accordion::Accordion,
    menu::{DropdownMenu, PopupMenuItem},
};

pub fn render(view: &mut AppView, cx: &mut Context<AppView>) -> AnyElement {
    let mut accounts = div().flex().flex_col().gap_3();
    if view.snapshot.accounts.is_empty() {
        accounts = accounts.child(div().text_color(cx.theme().muted_foreground).child("Connectez Yoto pour retrouver vos playlists et les envoyer à vos lecteurs. Votre médiathèque reste disponible hors ligne."));
    }
    for account in &view.snapshot.accounts {
        let active = view.snapshot.active_account.as_ref() == Some(&account.id);
        let id = account.id.clone();
        let select = id.clone();
        let owner = cx.entity().downgrade();
        let mut card = div()
            .p_4()
            .border_1()
            .border_color(cx.theme().border)
            .rounded_lg()
            .flex()
            .gap_3()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(account.label.clone()),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(if active {
                                "Connecté · Compte utilisé pour la synchronisation"
                            } else {
                                "Connecté"
                            }),
                    ),
            );
        if !active {
            card = card.child(
                Button::new(element_id("choose-account", &id))
                    .label("Utiliser ce compte")
                    .on_click(cx.listener(move |v, _, _, cx| {
                        v.send(Command::SelectAccount(select.clone()), cx);
                    })),
            );
        }
        accounts = accounts.child(
            card.child(
                Button::new(element_id("account-menu", &id))
                    .label("Options")
                    .ghost()
                    .dropdown_menu(move |menu, _, _| {
                        let owner = owner.clone();
                        let id = id.clone();
                        menu.item(PopupMenuItem::new("Déconnecter ce compte").on_click(
                            move |_, _, cx| {
                                let _ = owner.update(cx, |v, cx| {
                                    v.send(Command::Disconnect(id.clone()), cx)
                                });
                            },
                        ))
                    }),
            ),
        );
    }
    accounts = accounts.child(
        Button::new("login")
            .label(if view.connecting {
                "Connexion dans votre navigateur…"
            } else if view.snapshot.accounts.is_empty() {
                "Connecter mon compte Yoto"
            } else {
                "Ajouter un compte"
            })
            .disabled(view.connecting)
            .loading(view.connecting)
            .on_click(cx.listener(|v, _, _, cx| v.send(Command::Connect, cx))),
    );
    let mut body = div()
        .id("accounts-scroll")
        .size_full()
        .overflow_y_scroll()
        .p_8()
        .flex()
        .flex_col()
        .gap_6()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_2xl().child("Compte et synchronisation"))
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child("Votre collection personnelle, sur cet ordinateur et sur Yoto."),
                ),
        )
        .child(
            div()
                .max_w(px(760.))
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .text_lg()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Mes comptes Yoto"),
                )
                .child(accounts),
        );
    if let Some(id) = view.snapshot.active_account.clone() {
        let syncing = remote_syncing(&view.snapshot);
        let failures = view
            .snapshot
            .remote_sync
            .values()
            .filter(|state| matches!(state, RemoteSyncState::Failed { .. }))
            .count();
        let summary = if syncing {
            "Actualisation en cours…".into()
        } else if failures > 0 {
            format!(
                "{failures} élément(s) à actualiser. Les données déjà récupérées restent disponibles."
            )
        } else if view.snapshot.remote_sync.len() == RemoteCollection::ALL.len() {
            "Vos playlists et lecteurs sont à jour.".into()
        } else {
            "Vos playlists se chargent automatiquement. Actualisez pour retrouver aussi vos lecteurs.".into()
        };
        body = body.child(
            div()
                .max_w(px(760.))
                .p_4()
                .border_1()
                .border_color(cx.theme().border)
                .rounded_lg()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(
                                    div()
                                        .text_lg()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child("Données Yoto"),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(summary),
                                ),
                        )
                        .child(
                            Button::new("account-sync")
                                .label(if syncing {
                                    "Actualisation…"
                                } else {
                                    "Actualiser"
                                })
                                .primary()
                                .disabled(syncing)
                                .loading(syncing)
                                .on_click(cx.listener(move |v, _, _, cx| {
                                    v.send(Command::Sync(id.clone()), cx)
                                })),
                        ),
                )
                .child(
                    Accordion::new("account-sync-details")
                        .h_auto()
                        .bordered(false)
                        .item(|item| {
                            item.title("Voir le détail")
                                .open(view.account_sync_details_open)
                                .child(div().flex().flex_col().gap_2().children(
                                    RemoteCollection::ALL.iter().map(|collection| {
                                        div()
                                            .text_sm()
                                            .child(remote_sync_message(&view.snapshot, *collection))
                                    }),
                                ))
                        })
                        .on_toggle_click(cx.listener(|v, indices: &[usize], _, cx| {
                            v.account_sync_details_open = indices.contains(&0);
                            cx.notify();
                        })),
                ),
        );
    }
    let mut backup = div().flex().flex_col().gap_3()
        .child("Une sauvegarde chiffrée conserve vos originaux, vos versions audio et vos playlists. Les connexions Yoto restent dans le coffre sécurisé de ce profil.")
        .child(div().text_sm().text_color(cx.theme().muted_foreground).child("Conservez la clé dans votre gestionnaire de mots de passe : elle est nécessaire pour restaurer la sauvegarde."))
        .child(Input::new(&view.recovery_key))
        .child(div().flex().flex_wrap().gap_2()
            .child(Button::new("generate-recovery-key").label("Générer une clé").on_click(cx.listener(|v, _, window, cx| {
                use rand::RngCore;
                let mut bytes = [0;32]; rand::thread_rng().fill_bytes(&mut bytes);
                let text = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
                v.recovery_key.update(cx, |s, cx| s.set_value(text, window, cx));
            })))
            .child(Button::new("reveal-recovery-key").label(if view.recovery_visible { "Masquer" } else { "Afficher" }).on_click(cx.listener(|v, _, window, cx| {
                v.recovery_visible = !v.recovery_visible;
                v.recovery_key.update(cx, |s, cx| s.set_masked(!v.recovery_visible, window, cx)); cx.notify();
            })))
            .child(Button::new("copy-recovery-key").label("Copier la clé").on_click(cx.listener(|v, _, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(v.recovery_key.read(cx).value().to_string()))))))
        .child(div().flex().flex_wrap().gap_2()
            .child(Button::new("backup-catalogue").label("Créer une sauvegarde").on_click(cx.listener(|v, _, _, cx| v.recovery(false, cx))))
            .child(Button::new("restore-catalogue").label("Restaurer une sauvegarde").on_click(cx.listener(|v, _, _, cx| v.recovery(true, cx)))));
    if let Some(path) = &view.restored_profile {
        let open = path.clone();
        backup = backup
            .child(format!("Médiathèque restaurée : {}", path.display()))
            .child(
                Button::new("open-restored")
                    .label("Ouvrir la médiathèque restaurée")
                    .on_click(cx.listener(move |v, _, _, cx| v.open_profile(open.clone(), cx))),
            );
    }
    backup = backup.child(
        Button::new("choose-profile")
            .label("Ouvrir une autre médiathèque")
            .ghost()
            .on_click(cx.listener(|v, _, _, cx| v.choose_profile(cx))),
    );
    body.child(
        Accordion::new("account-backup")
            .h_auto()
            .max_w(px(760.))
            .item(|item| {
                item.title("Sauvegarde et médiathèques · Options avancées")
                    .open(view.account_backup_open)
                    .child(backup)
            })
            .on_toggle_click(cx.listener(|v, indices: &[usize], _, cx| {
                v.account_backup_open = indices.contains(&0);
                cx.notify();
            })),
    )
    .into_any_element()
}
