use super::*;
use gpui_kit::component::{WindowExt, spinner::Spinner};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Choosing,
    Loading,
    Completed,
}

struct PlaylistPicker {
    parent: WeakEntity<AppView>,
    account: AccountId,
    batch: Vec<(MediaId, RecipeId)>,
    selected: Option<CardId>,
    window: AnyWindowHandle,
    phase: Phase,
    invalidated: bool,
    error: Option<String>,
    _parent_observation: Subscription,
}

pub(super) fn open(view: &mut AppView, window: &mut Window, cx: &mut Context<AppView>) {
    let Some(account) = view.snapshot.active_account.clone() else {
        return;
    };
    let batch = selected_batch(
        &view.snapshot,
        &view.selected_media_set,
        view.selected_media,
        |media| {
            view.trims
                .get(&media)
                .and_then(|panel| panel.read(cx).chosen_recipe())
        },
    );
    if batch.is_empty() {
        return;
    }
    view.refresh_playlists(cx);
    let parent = cx.entity();
    let handle = window.window_handle();
    let picker = cx.new(|cx| PlaylistPicker {
        parent: parent.downgrade(),
        account,
        batch,
        selected: None,
        window: handle,
        phase: Phase::Choosing,
        invalidated: false,
        error: None,
        _parent_observation: cx.observe(&parent, |picker: &mut PlaylistPicker, _, cx| {
            if !picker.active(cx) && picker.phase != Phase::Completed {
                // Even switching back must not revive an old asynchronous request.
                picker.invalidated = true;
                picker.fail(
                    tr!("Le compte Yoto a changé. Fermez cette fenêtre et recommencez.").into(),
                    cx,
                );
            }
            cx.notify();
        }),
    });
    window.open_dialog(cx, move |dialog, _, _| {
        let cancel = picker.downgrade();
        dialog
            .title(tr!("Ajouter à une playlist"))
            .w(px(500.))
            .child(picker.clone())
            .on_cancel(move |_, _, cx| {
                cancel
                    .upgrade()
                    .is_none_or(|picker| !picker.read(cx).busy())
            })
    });
}

fn account_matches(captured: &AccountId, current: Option<&AccountId>) -> bool {
    current == Some(captured)
}

impl PlaylistPicker {
    fn active(&self, cx: &App) -> bool {
        !self.invalidated
            && self.parent.upgrade().is_some_and(|parent| {
                account_matches(
                    &self.account,
                    parent.read(cx).snapshot.active_account.as_ref(),
                )
            })
    }

    fn busy(&self) -> bool {
        self.phase == Phase::Loading
    }

    fn fail(&mut self, reason: String, cx: &mut Context<Self>) {
        self.phase = Phase::Choosing;
        self.error = Some(reason);
        cx.notify();
    }

    fn append(&mut self, id: PlaylistId, window: &mut Window, cx: &mut Context<Self>) {
        if !self.active(cx) {
            self.fail(
                tr!("Le compte Yoto a changé. Fermez cette fenêtre et recommencez.").into(),
                cx,
            );
            return;
        }
        let Some(parent) = self.parent.upgrade() else {
            return;
        };
        let editor = parent.update(cx, |view, cx| {
            view.select_playlist(id, window, cx);
            view.editors.get(&id).cloned()
        });
        let Some(editor) = editor else {
            self.fail(
                tr!("Cette playlist n’est plus disponible. Actualisez la liste et réessayez.")
                    .into(),
                cx,
            );
            return;
        };
        let batch = self.batch.clone();
        match editor.update(cx, |editor, cx| editor.append_library_media(batch, cx)) {
            Err(error) => self.fail(failure_text(&error), cx),
            Ok(()) => {
                // Admission is the hand-off: the editor owns retries, errors and
                // reconciliation from here, so this chooser cannot submit it twice.
                self.phase = Phase::Completed;
                parent.update(cx, |view, cx| {
                    let sent: std::collections::BTreeSet<_> =
                        self.batch.iter().map(|(media, _)| *media).collect();
                    view.selected_media_set.retain(|id| !sent.contains(id));
                    view.navigate(Route::Playlists, cx);
                });
                window.close_dialog(cx);
            }
        }
    }

    fn add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.phase != Phase::Choosing || !self.active(cx) {
            return;
        }
        let Some(card) = self.selected.clone() else {
            return;
        };
        let Some(parent) = self.parent.upgrade() else {
            return;
        };
        let view = parent.read(cx);
        if !view
            .playlist_rows
            .iter()
            .any(|row| row.target.account == self.account && row.target.card == card)
        {
            self.fail(
                tr!("Cette playlist n’est plus disponible. Actualisez la liste et réessayez.")
                    .into(),
                cx,
            );
            return;
        }
        let existing = view
            .snapshot
            .bindings
            .iter()
            .find(|binding| binding.account == self.account && binding.card == card)
            .map(|binding| binding.playlist)
            .filter(|id| view.editors.contains_key(id));
        if let Some(id) = existing {
            self.append(id, window, cx);
            return;
        }
        // Loading an unopened destination has no navigation side effect. Reuse an
        // existing editor above so a remote reload never replaces its unsaved edits.
        let service = view.service.clone();
        let runtime = view.runtime.clone();
        let base = view.snapshot.clone();
        let previous = view.revisions.clone();
        let worker_base = base.clone();
        let account = self.account.clone();
        let handle = self.window;
        self.phase = Phase::Loading;
        self.error = None;
        cx.notify();
        cx.spawn(async move |weak, cx| {
            let result = runtime
                .spawn(async move {
                    let Reply::OpenedPlaylist(id) = service
                        .execute(Command::LoadRemote { account, card })
                        .await?
                    else {
                        return Err(Failure::Compatibility(
                            tr!("Résultat de playlist inattendu.").into(),
                        ));
                    };
                    let delta = service.interface_changes(previous).await?;
                    let prepared = tokio::task::spawn_blocking(move || {
                        let mut snapshot = (*worker_base).clone();
                        let revisions = delta.apply(&mut snapshot);
                        PreparedSnapshot::new(snapshot, revisions)
                    })
                    .await
                    .map_err(|error| Failure::Storage(error.to_string()))?;
                    Ok::<_, Failure>((id, prepared))
                })
                .await;
            let _ = cx.update_window(handle, |_, window, cx| {
                let _ = weak.update(cx, |picker, cx| {
                    if picker.phase != Phase::Loading {
                        return;
                    }
                    if !picker.active(cx) {
                        picker.fail(
                            tr!("Le compte Yoto a changé. Fermez cette fenêtre et recommencez.")
                                .into(),
                            cx,
                        );
                        return;
                    }
                    let result = result
                        .map_err(|error| Failure::Storage(error.to_string()))
                        .and_then(|result| result);
                    match result {
                        Err(error) => picker.fail(failure_text(&error), cx),
                        Ok((id, prepared)) => {
                            if let Some(parent) = picker.parent.upgrade() {
                                parent.update(cx, |view, cx| {
                                    if Arc::ptr_eq(&view.snapshot, &base)
                                        || (view.revisions.len() == prepared.revisions.len()
                                            && view
                                                .revisions
                                                .iter()
                                                .zip(&prepared.revisions)
                                                .all(|(current, next)| next >= current))
                                    {
                                        view.adopt(prepared, cx);
                                    }
                                });
                            }
                            picker.append(id, window, cx);
                        }
                    }
                });
            });
        })
        .detach();
    }
}

impl Render for PlaylistPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.active(cx);
        let busy = self.busy();
        let parent = self.parent.upgrade();
        let rows = parent
            .as_ref()
            .filter(|_| active)
            .map(|parent| parent.read(cx).playlist_rows.clone());
        let syncing = parent.as_ref().filter(|_| active).is_some_and(|parent| {
            matches!(
                parent
                    .read(cx)
                    .snapshot
                    .remote_sync
                    .get(&RemoteCollection::Playlists),
                Some(RemoteSyncState::Syncing)
            )
        });
        let sync_error = parent.as_ref().filter(|_| active).and_then(|parent| {
            match parent
                .read(cx)
                .snapshot
                .remote_sync
                .get(&RemoteCollection::Playlists)
            {
                Some(RemoteSyncState::Failed { failure }) => Some(failure_text(failure)),
                _ => None,
            }
        });
        let mut list = div()
            .id("library-playlist-choices")
            .max_h(px(320.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(8.));
        if let Some(rows) = rows {
            for row in rows.iter().filter(|row| row.target.account == self.account) {
                let card = row.target.card.clone();
                let selected = self.selected.as_ref() == Some(&card);
                list = list.child(theme::pill(
                    Button::new(element_id("library-destination", card.as_str()))
                        .label(row.title.clone())
                        .w_full()
                        .on_click(cx.listener(move |picker, _, _, cx| {
                            picker.selected = Some(card.clone());
                            picker.error = None;
                            cx.notify();
                        })),
                    if selected {
                        Look::Primary
                    } else {
                        Look::Outline
                    },
                    Height::Medium,
                    busy,
                    cx,
                ));
            }
            if rows.is_empty() && !syncing {
                list = list.child(theme::note(tr!("Aucune playlist Yoto. Créez une playlist dans l’onglet Playlists, puis revenez ici.")));
            }
        }
        let count = self.batch.len();
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(theme::note(trn!(
                "Choisissez la playlist qui recevra cet audio.",
                "Choisissez la playlist qui recevra ces {count} audios.",
                count
            )))
            .child(list);
        if syncing || busy {
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(Spinner::new().small())
                    .child(tr!("Chargement des playlists…")),
            );
        }
        let error = if active {
            self.error.clone().or(sync_error)
        } else {
            Some(tr!("Le compte Yoto a changé. Fermez cette fenêtre et recommencez.").into())
        };
        if let Some(error) = error {
            body = body.child(theme::note(error));
        }
        body.child(
            theme::dialog_actions()
                .child(theme::pill(
                    Button::new("library-destination-refresh")
                        .label(tr!("Actualiser"))
                        .on_click(cx.listener(|picker, _, _, cx| {
                            let _ = picker
                                .parent
                                .update(cx, |view, cx| view.refresh_playlists(cx));
                        })),
                    Look::Outline,
                    Height::Medium,
                    busy || !active || syncing,
                    cx,
                ))
                .child(div().flex_1())
                .child(theme::dialog_close(
                    "library-destination-close",
                    theme::pill(
                        Button::new("library-destination-cancel").label(tr!("Annuler")),
                        Look::Ghost,
                        Height::Medium,
                        busy,
                        cx,
                    ),
                ))
                .child(theme::pill(
                    Button::new("library-destination-add")
                        .label(tr!("Ajouter"))
                        .on_click(cx.listener(|picker, _, window, cx| picker.add(window, cx))),
                    Look::Primary,
                    Height::Medium,
                    busy || !active || self.selected.is_none(),
                    cx,
                )),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{account_matches, editor};
    use yoto_domain::{AccountId, CardId};
    use yoto_infra::Snapshot;
    #[test]
    fn destination_is_invalid_after_switching_or_disconnecting_accounts() {
        let account = AccountId::new("one").unwrap();
        let other = AccountId::new("two").unwrap();
        assert!(account_matches(&account, Some(&account)));
        assert!(!account_matches(&account, Some(&other)));
        assert!(!account_matches(&account, None));
    }

    #[test]
    fn remote_destinations_do_not_require_a_previously_opened_playlist() {
        let account = AccountId::new("family").unwrap();
        let snapshot = Snapshot {
            active_account: Some(account.clone()),
            remote: vec![yoto_infra::api::RemoteCard {
                card: CardId::new("unopened-card").unwrap(),
                title: "Never opened".into(),
                document: serde_json::Value::Null,
                synced: 1,
            }],
            ..Default::default()
        };
        assert!(snapshot.playlists.is_empty());
        assert!(snapshot.bindings.is_empty());
        let rows = editor::playlist_rows(&snapshot);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].target.account, account);
        assert_eq!(rows[0].target.card.as_str(), "unopened-card");
    }
}
