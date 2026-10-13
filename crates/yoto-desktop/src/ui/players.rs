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
        .child(div().text_2xl().child("Lecteurs et bibliothèque Yoto"))
        .child(account_summary(&view.snapshot));
    let Some(account) = view.snapshot.active_account.clone() else {
        return body
            .child("Connectez un compte Yoto pour consulter vos playlists MYO et vos lecteurs.")
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
            .label("Synchroniser MYO, lecteurs et groupes")
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
                .label("Voir mes playlists Yoto")
                .on_click(cx.listener(|v, _, _, cx| {
                    v.route = Route::Playlists;
                    cx.notify();
                })),
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
    body=body.child(div().text_lg().child("Bibliothèque MYO")).child("Le catalogue public confirme les créations MYO ; les cartes commerciales ne sont pas énumérées ici.");
    for card in &view.snapshot.remote {
        let id = card.card.clone();
        let account = account.clone();
        let play_card = id.clone();
        let delete_card = id.clone();
        let delete_account = account.clone();
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
                        .disabled(panel.is_none())
                        .on_click(cx.listener(move |_, _, _, cx| {
                            if let Some(panel) = &panel {
                                panel.update(cx, |p, cx| p.start(play_card.clone(), cx));
                            }
                        })),
                )
                .child(
                    Button::new(element_id("delete-card", delete_card.to_string()))
                        .label("Supprimer de Yoto")
                        .on_click(cx.listener(move |v, _, _, cx| {
                            v.send(
                                Command::DeleteRemote {
                                    account: delete_account.clone(),
                                    card: delete_card.clone(),
                                },
                                cx,
                            )
                        })),
                )
                .child(
                    Button::new(element_id("edit-remote", id.as_str().to_owned()))
                        .label("Importer le brouillon")
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
                        .on_click(cx.listener(move |v, _, _, cx| {
                            v.send(
                                Command::DeleteGroup {
                                    account: account.clone(),
                                    id: id.clone(),
                                },
                                cx,
                            )
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
