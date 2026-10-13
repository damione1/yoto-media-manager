use super::*;
pub fn render(view: &mut AppView, _window: &mut Window, cx: &mut Context<AppView>) -> AnyElement {
    let mut body = div()
        .id("players-scroll")
        .size_full()
        .p_7()
        .flex()
        .flex_col()
        .gap_4()
        .overflow_y_scroll()
        .child(div().text_2xl().child("Lecteurs"))
        .child(account_summary(&view.snapshot));
    let Some(account) = view.snapshot.active_account.clone() else {
        return body
            .child(
                "Connectez votre compte Yoto pour voir vos lecteurs et lancer une playlist dessus.",
            )
            .child(
                Button::new("players-login")
                    .label(if view.connecting {
                        "Connexion dans votre navigateur…"
                    } else {
                        "Connecter Yoto"
                    })
                    .disabled(view.connecting)
                    .primary()
                    .on_click(cx.listener(|v, _, _, cx| v.send(Command::Connect, cx))),
            )
            .into_any_element();
    };
    let sync_account = account.clone();
    body = body.child(
        Button::new("sync-yoto")
            .label("Actualiser depuis Yoto")
            .disabled(remote_syncing(&view.snapshot))
            .primary()
            .on_click(
                cx.listener(move |v, _, _, cx| v.send(Command::Sync(sync_account.clone()), cx)),
            ),
    );
    body = body
        .children(RemoteCollection::ALL.iter().map(|collection| {
            div()
                .text_sm()
                .child(remote_sync_message(&view.snapshot, *collection))
        }))
        .child(
            Button::new("open-online-playlists")
                .label("Ouvrir mes playlists")
                .on_click(cx.listener(|v, _, _, cx| v.navigate(Route::Playlists, cx))),
        );
    for device in &view.snapshot.devices {
        let id = device.id.clone();
        let selected = id.clone();
        let account = account.clone();
        body = body.child(
            Button::new(element_id("select-device", id.to_string()))
                .label(format!(
                    "{} · {}",
                    device.name,
                    if device.online {
                        "En ligne"
                    } else {
                        "Hors ligne"
                    }
                ))
                .selected(view.selected_device.as_ref() == Some(&id))
                .on_click(cx.listener(move |v, _, window, cx| {
                    let key = (account.clone(), selected.clone());
                    if !v.device_panels.contains_key(&key)
                        && let Some(d) = v.snapshot.devices.iter().find(|d| d.id == selected)
                    {
                        let panel = cx.new(|cx| {
                            device::DevicePanel::new(
                                account.clone(),
                                d.clone(),
                                v.service.clone(),
                                v.runtime.clone(),
                                window,
                                cx,
                            )
                        });
                        v.device_panels.insert(key, panel);
                    }
                    v.selected_device = Some(selected.clone());
                    cx.notify();
                })),
        );
    }
    if let Some(id) = &view.selected_device
        && let Some(panel) = view.device_panels.get(&(account.clone(), id.clone()))
    {
        body = body.child(panel.clone());
    }
    if view.snapshot.devices.is_empty() {
        body = body.child(div().text_sm().text_color(cx.theme().muted_foreground).child("Aucun lecteur trouvé. Actualisez depuis Yoto, puis vérifiez que votre lecteur est allumé et connecté au Wi-Fi."));
    }
    body = body
        .child(div().text_lg().child("Mes playlists Yoto"))
        .child(div().text_sm().text_color(cx.theme().muted_foreground).child("Choisissez un lecteur ci-dessus, puis lancez une playlist. Les cartes achetées dans la boutique Yoto n’apparaissent pas ici."));
    for card in &view.snapshot.remote {
        let id = card.card.clone();
        let account = account.clone();
        let play_card = id.clone();
        let delete_card = id.clone();
        let delete_account = account.clone();
        let title = card.title.clone();
        let panel = view
            .selected_device
            .as_ref()
            .and_then(|d| view.device_panels.get(&(account.clone(), d.clone())))
            .cloned();
        body = body.child(
            div()
                .flex()
                .gap_3()
                .items_center()
                .child(card.title.clone())
                .child(
                    Button::new(element_id("play-card", play_card.to_string()))
                        .label("Lire sur le lecteur sélectionné")
                        .tooltip(if panel.is_none() { "Choisissez d’abord un lecteur" } else { "Lancer cette playlist sur le lecteur" })
                        .disabled(panel.is_none())
                        .on_click(cx.listener(move |_, _, _, cx| {
                            if let Some(panel) = &panel {
                                panel.update(cx, |p, cx| p.start(play_card.clone(), cx));
                            }
                        })),
                )
                .child(
                    Button::new(element_id("delete-card", delete_card.to_string()))
                        .label("Supprimer de mon compte")
                        .on_click(cx.listener(move |_, _, window, cx| {
                            let owner = cx.entity().downgrade();
                            let account = delete_account.clone();
                            let card = delete_card.clone();
                            confirm_destructive(
                                window,
                                cx,
                                format!("Supprimer « {title} » de votre compte Yoto ?"),
                                "La playlist disparaîtra de votre compte Yoto et de vos lecteurs. Vos fichiers audio restent dans la médiathèque. Cette action est irréversible.",
                                "Supprimer",
                                move |_, cx| {
                                    let _ = owner.update(cx, |v, cx| {
                                        v.send(
                                            Command::DeleteRemote {
                                                account: account.clone(),
                                                card: card.clone(),
                                            },
                                            cx,
                                        )
                                    });
                                },
                            );
                        })),
                )
                .child(
                    Button::new(element_id("edit-remote", id.as_str().to_owned()))
                        .label("Modifier dans Playlists")
                        .on_click(cx.listener(move |v, _, _, cx| {
                            v.send(
                                Command::LoadRemote {
                                    account: account.clone(),
                                    card: id.clone(),
                                },
                                cx,
                            )
                        })),
                ),
        );
    }
    body = body.child(div().text_lg().child("Groupes familiaux"));
    let create_account = account.clone();
    body = body.child(
        Button::new("new-group")
            .label("Créer un groupe")
            .on_click(cx.listener(move |v, _, window, cx| {
                let key = (create_account.clone(), None);
                if !v.group_editors.contains_key(&key) {
                    let panel = cx.new(|cx| {
                        group::GroupEditor::new(
                            create_account.clone(),
                            None,
                            v.snapshot.remote.clone(),
                            v.service.clone(),
                            v.runtime.clone(),
                            window,
                            cx,
                        )
                    });
                    v.group_editors.insert(key, panel);
                }
                v.selected_group = Some(None);
                cx.notify();
            })),
    );
    for group in &view.snapshot.groups {
        let account = account.clone();
        let id = group.id.clone();
        let edit_id = id.clone();
        let edit_account = account.clone();
        let name = group.name.clone();
        body = body.child(
            div()
                .flex()
                .gap_3()
                .child(group.name.clone())
                .child(
                    Button::new(element_id("edit-group", edit_id.to_string()))
                        .label("Modifier")
                        .on_click(cx.listener(move |v, _, window, cx| {
                            let key = (edit_account.clone(), Some(edit_id.clone()));
                            if !v.group_editors.contains_key(&key)
                                && let Some(group) =
                                    v.snapshot.groups.iter().find(|g| g.id == edit_id)
                            {
                                let panel = cx.new(|cx| {
                                    group::GroupEditor::new(
                                        edit_account.clone(),
                                        Some(group.clone()),
                                        v.snapshot.remote.clone(),
                                        v.service.clone(),
                                        v.runtime.clone(),
                                        window,
                                        cx,
                                    )
                                });
                                v.group_editors.insert(key, panel);
                            }
                            v.selected_group = Some(Some(edit_id.clone()));
                            cx.notify();
                        })),
                )
                .child(
                    Button::new(element_id("delete-group", id.as_str().to_owned()))
                        .label("Supprimer ce groupe")
                        .on_click(cx.listener(move |_, _, window, cx| {
                            let owner = cx.entity().downgrade();
                            let account = account.clone();
                            let id = id.clone();
                            confirm_destructive(
                                window,
                                cx,
                                format!("Supprimer le groupe « {name} » ?"),
                                "Le groupe disparaîtra de votre compte Yoto. Les playlists qu’il contient ne sont pas supprimées.",
                                "Supprimer",
                                move |_, cx| {
                                    let _ = owner.update(cx, |v, cx| {
                                        v.send(
                                            Command::DeleteGroup {
                                                account: account.clone(),
                                                id: id.clone(),
                                            },
                                            cx,
                                        )
                                    });
                                },
                            );
                        })),
                ),
        );
    }
    if let Some(selected) = &view.selected_group
        && let Some(panel) = view.group_editors.get(&(account, selected.clone()))
    {
        body = body.child(panel.clone());
    }
    body.into_any_element()
}
