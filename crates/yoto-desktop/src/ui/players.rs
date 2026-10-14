use super::*;
use gpui_kit::component::button::{ButtonCustomVariant, ButtonRounded};
use theme::{Height, Look};

impl AppView {
    fn select_device(
        &mut self,
        account: AccountId,
        id: DeviceId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = (account.clone(), id.clone());
        if !self.device_panels.contains_key(&key)
            && let Some(d) = self.snapshot.devices.iter().find(|d| d.id == id)
        {
            let panel = cx.new(|cx| {
                device::DevicePanel::new(
                    account,
                    d.clone(),
                    self.service.clone(),
                    self.runtime.clone(),
                    window,
                    cx,
                )
            });
            // The screen draws the panel's cards itself, so it redraws when the panel changes.
            self._subscriptions
                .push(cx.observe(&panel, |_, _, cx| cx.notify()));
            // Settings are read as soon as a player is chosen, so the card shows real values.
            panel.update(cx, |p, cx| p.load(cx));
            self.device_panels.insert(key, panel);
        }
        self.selected_device = Some(id);
        cx.notify();
    }
    fn edit_group(
        &mut self,
        account: AccountId,
        group: Option<GroupId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = (account.clone(), group.clone());
        if !self.group_editors.contains_key(&key) {
            let existing = group
                .as_ref()
                .and_then(|id| self.snapshot.groups.iter().find(|g| &g.id == id))
                .cloned();
            if group.is_some() && existing.is_none() {
                return;
            }
            let panel = cx.new(|cx| {
                group::GroupEditor::new(
                    account,
                    existing,
                    self.snapshot.remote.clone(),
                    self.service.clone(),
                    self.runtime.clone(),
                    window,
                    cx,
                )
            });
            panel.update(cx, |p, cx| p.load(cx));
            let owner = key.0.clone();
            self._subscriptions
                .push(cx.subscribe(&panel, move |v, _, event, cx| {
                    match event {
                        group::GroupEditorEvent::Created(id) => rekey_created_group(
                            &mut v.group_editors,
                            &mut v.selected_group,
                            &owner,
                            id.clone(),
                        ),
                        group::GroupEditorEvent::Close => v.selected_group = None,
                    }
                    cx.notify();
                }));
            self.group_editors.insert(key, panel);
        }
        self.selected_group = Some(group);
        cx.notify();
    }
}

/// Once a new group is saved, its editor edits that group: it moves to the group's own key,
/// so « Créer un groupe » opens a blank editor next time instead of overwriting this group.
fn rekey_created_group<T>(
    editors: &mut BTreeMap<(AccountId, Option<GroupId>), T>,
    selected: &mut Option<Option<GroupId>>,
    account: &AccountId,
    id: GroupId,
) {
    if let Some(editor) = editors.remove(&(account.clone(), None)) {
        editors.insert((account.clone(), Some(id.clone())), editor);
        if *selected == Some(None) {
            *selected = Some(Some(id));
        }
    }
}

pub fn render(view: &mut AppView, _window: &mut Window, cx: &mut Context<AppView>) -> AnyElement {
    let mut header = div()
        .flex()
        .items_center()
        .gap(px(12.))
        .child(div().flex_1().child(theme::screen_header(
            "Lecteurs",
            "Choisissez un lecteur, lancez une playlist, réglez le volume et la lumière.",
        )));
    let mut body = div()
        .id("players-scroll")
        .size_full()
        .overflow_y_scroll()
        .py(px(24.))
        .px(px(28.))
        .flex()
        .flex_col()
        .gap(px(16.));
    let Some(account) = view.snapshot.active_account.clone() else {
        return body
            .child(header)
            .child(
                theme::card()
                    .items_start()
                    .child(theme::icon_tile(IconName::Speaker, &theme::SKY, 48., 14.))
                    .child(theme::heading("Connectez votre compte Yoto", 19.))
                    .child(theme::note(
                        "Vous verrez alors vos lecteurs et pourrez lancer une playlist dessus.",
                    ))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .items_center()
                            .child(theme::pill(
                                Button::new("players-login")
                                    .label(if view.connecting {
                                        wording::connect::waiting()
                                    } else {
                                        "Connecter Yoto"
                                    })
                                    .on_click(
                                        cx.listener(|v, _, _, cx| v.send(Command::Connect, cx)),
                                    ),
                                Look::Primary,
                                Height::Large,
                                view.connecting,
                                cx,
                            ))
                            .children(view.connecting.then(|| {
                                cancel_connect("players-login-cancel", Height::Large, cx)
                            })),
                    ),
            )
            .into_any_element();
    };
    let syncing = remote_syncing(&view.snapshot);
    let sync_account = account.clone();
    header = header.child(theme::pill(
        Button::new("sync-yoto")
            .icon(IconName::RefreshCw)
            .label(if syncing {
                "Actualisation…"
            } else {
                "Actualiser depuis Yoto"
            })
            .on_click(
                cx.listener(move |v, _, _, cx| v.send(Command::Sync(sync_account.clone()), cx)),
            ),
        Look::Outline,
        Height::Large,
        syncing,
        cx,
    ));
    body = body.child(header);
    if let Some(RemoteSyncState::Failed { .. }) =
        view.snapshot.remote_sync.get(&RemoteCollection::Devices)
    {
        body = body.child(theme::note(remote_sync_message(
            &view.snapshot,
            RemoteCollection::Devices,
        )));
    }
    let mut players = div().flex().flex_wrap().gap(px(12.));
    for device in &view.snapshot.devices {
        let id = device.id.clone();
        let selected = view.selected_device.as_ref() == Some(&id);
        let family = if !device.online {
            &theme::STONE
        } else if selected {
            &theme::SKY
        } else {
            theme::family_for(&device.id.to_string())
        };
        let account = account.clone();
        let mut card = Button::new(element_id("select-device", id.to_string()))
            .custom(
                ButtonCustomVariant::new(cx)
                    .color(theme::c(theme::SURFACE))
                    .foreground(theme::c(theme::INK))
                    .hover(theme::c(theme::SURFACE))
                    .active(theme::c(theme::CREAM)),
            )
            .accessibility_label(format!(
                "{} · {}",
                device.name,
                if device.online {
                    "En ligne"
                } else {
                    "Hors ligne"
                }
            ))
            .toggled(selected)
            .bg(theme::c(theme::SURFACE))
            .rounded(ButtonRounded::Size(px(20.)))
            .h(px(80.))
            .px(px(14.))
            .flex_1()
            .min_w(px(220.))
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(theme::icon_tile(IconName::Speaker, family, 48., 14.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .font_weight(FontWeight::EXTRA_BOLD)
                                    .text_size(px(16.))
                                    .truncate()
                                    .child(device.name.clone()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(5.))
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(theme::c(if device.online {
                                        theme::MINT.ink
                                    } else {
                                        theme::MUTED_WARM
                                    }))
                                    .child(theme::dot(if device.online {
                                        theme::MINT.accent
                                    } else {
                                        theme::OFFLINE
                                    }))
                                    .child(if device.online {
                                        "En ligne"
                                    } else {
                                        "Hors ligne"
                                    }),
                            ),
                    ),
            )
            .on_click(cx.listener(move |v, _, window, cx| {
                v.select_device(account.clone(), id.clone(), window, cx)
            }));
        card = if selected {
            card.border_2().border_color(theme::c(theme::SKY.accent))
        } else {
            card.shadow(theme::step(theme::BORDER, 2.))
        };
        if !device.online {
            card = card.opacity(0.75);
        }
        players = players.child(card);
    }
    body = body.child(players);
    if view.snapshot.devices.is_empty() {
        body = body.child(theme::note(
            "Aucun lecteur trouvé. Actualisez depuis Yoto, puis vérifiez que votre lecteur est allumé et connecté au Wi-Fi.",
        ));
    }
    let selected = view.selected_device.as_ref().and_then(|id| {
        let panel = view.device_panels.get(&(account.clone(), id.clone()))?;
        let device = view.snapshot.devices.iter().find(|d| &d.id == id)?;
        Some((panel.clone(), device.online))
    });
    let column = || {
        div()
            .flex_1()
            .min_w(px(360.))
            .flex()
            .flex_col()
            .gap(px(12.))
    };
    let advanced = view.advanced;
    let (play, settings) = match selected {
        Some((panel, online)) => {
            let snapshot = view.snapshot.clone();
            panel.update(cx, |p, cx| {
                (
                    p.play_card(&snapshot.remote, online, cx),
                    Some(p.settings(advanced, cx)),
                )
            })
        }
        None if !view.snapshot.devices.is_empty() => (
            theme::inset()
                .child(theme::note(
                    "Choisissez un lecteur ci-dessus pour lancer une playlist, régler le volume, la veilleuse ou la minuterie.",
                ))
                .into_any_element(),
            None,
        ),
        None => (div().into_any_element(), None),
    };
    body.child(
        div()
            .flex()
            .flex_wrap()
            .items_start()
            .gap(px(14.))
            .child(column().child(play))
            .child(column().children(settings).child(groups(view, account, cx))),
    )
    .into_any_element()
}

fn groups(view: &AppView, account: AccountId, cx: &mut Context<AppView>) -> AnyElement {
    let create_account = account.clone();
    let mut card = theme::card().child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .child(theme::heading("Groupes familiaux", 19.))
            .child(theme::pill(
                Button::new("new-group")
                    .label("Créer un groupe")
                    .on_click(cx.listener(move |v, _, window, cx| {
                        v.edit_group(create_account.clone(), None, window, cx)
                    })),
                Look::Soft(&theme::PEACH),
                Height::Tiny,
                false,
                cx,
            )),
    );
    if view.snapshot.groups.is_empty() {
        card = card.child(theme::note(
            "Un groupe rassemble des playlists à partager avec la famille, dans l’application Yoto.",
        ));
    }
    for group in &view.snapshot.groups {
        let id = group.id.clone();
        let edit_id = id.clone();
        let edit_account = account.clone();
        let delete_account = account.clone();
        let name = group.name.clone();
        let count = group.cards.len();
        card = card.child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(
                    div()
                        .size(px(40.))
                        .flex_shrink_0()
                        .rounded(px(12.))
                        .bg(theme::c(theme::family_for(id.as_str()).soft)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .font_weight(FontWeight::EXTRA_BOLD)
                                .truncate()
                                .child(group.name.clone()),
                        )
                        .child(theme::meta(match count {
                            0 => "Aucune playlist".to_owned(),
                            1 => "1 playlist".to_owned(),
                            n => format!("{n} playlists"),
                        })),
                )
                .child(theme::pill(
                    Button::new(element_id("edit-group", edit_id.to_string()))
                        .label("Modifier")
                        .on_click(cx.listener(move |v, _, window, cx| {
                            v.edit_group(edit_account.clone(), Some(edit_id.clone()), window, cx)
                        })),
                    Look::Link(&theme::PEACH),
                    Height::Tiny,
                    false,
                    cx,
                ))
                .child(theme::pill(
                    Button::new(element_id("delete-group", id.as_str().to_owned()))
                        .label("Supprimer")
                        .on_click(cx.listener(move |_, _, window, cx| {
                            let owner = cx.entity().downgrade();
                            let account = delete_account.clone();
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
                    Look::Link(&theme::ROSE),
                    Height::Tiny,
                    false,
                    cx,
                )),
        );
    }
    if let Some(selected) = &view.selected_group
        && let Some(panel) = view.group_editors.get(&(account, selected.clone()))
    {
        card = card.child(panel.clone());
    }
    card.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[core::prelude::v1::test]
    fn a_created_group_frees_the_new_group_slot() {
        let account = AccountId::new("account").unwrap();
        let first = GroupId::new("group-1").unwrap();
        let mut editors = BTreeMap::new();
        editors.insert((account.clone(), None), "draft");
        let mut selected = Some(None);
        rekey_created_group(&mut editors, &mut selected, &account, first.clone());
        assert!(!editors.contains_key(&(account.clone(), None)));
        assert_eq!(
            editors.get(&(account.clone(), Some(first.clone()))),
            Some(&"draft")
        );
        assert_eq!(selected, Some(Some(first.clone())));
        // Another group being edited stays selected when an older draft is saved.
        editors.insert((account.clone(), None), "second");
        let mut selected = Some(Some(first));
        rekey_created_group(
            &mut editors,
            &mut selected,
            &account,
            GroupId::new("group-2").unwrap(),
        );
        assert_eq!(selected.flatten().unwrap().as_str(), "group-1");
        assert_eq!(editors.len(), 2);
    }
}
