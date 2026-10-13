use super::*;
use gpui_kit::component::WindowExt;
use gpui_kit::component::checkbox::Checkbox;
use yoto_infra::service::ConflictChoice;
impl AppView {
    pub fn begin_youtube(&mut self, cx: &mut Context<Self>) {
        if self.youtube_import_pending {
            return;
        }
        let url = self.youtube.read(cx).value().trim().to_owned();
        if url.is_empty() {
            self.report_error(
                Failure::Validation("Collez un lien YouTube pour importer son audio.".into()),
                cx,
            );
            return;
        }
        self.youtube_import_pending = true;
        self.youtube_inspection_generation = self.youtube_inspection_generation.wrapping_add(1);
        let generation = self.youtube_inspection_generation;
        self.youtube_entries.clear();
        self.youtube_selection_url = None;
        self.selected_entries.clear();
        self.error = None;
        self.status = "Recherche du titre et des informations YouTube…".into();
        cx.notify();
        let service = self.service.clone();
        let rt = self.runtime.clone();
        cx.spawn(async move |weak, cx| {
            let request_url = url.clone();
            let result = rt
                .spawn(async move { service.execute(Command::InspectYoutube(request_url)).await })
                .await;
            let _ = weak.update(cx, |view, cx| {
                if generation != view.youtube_inspection_generation {
                    return;
                }
                view.youtube_import_pending = false;
                if view.youtube.read(cx).value().trim() != url {
                    cx.notify();
                    return;
                }
                match result {
                    Ok(Ok(Reply::Entries(inspection))) => match inspection.kind {
                        SourceShape::Video => {
                            if let [entry] = inspection.entries.as_slice() {
                                view.youtube_titles
                                    .insert(entry.url.clone(), entry.title.clone());
                                view.send(
                                    Command::Import(vec![ImportSelection::Youtube {
                                        url: entry.url.clone(),
                                        entry: Some(entry.id.clone()),
                                    }]),
                                    cx,
                                );
                                view.status = format!(
                                    "Import de « {} » lancé. Suivez-le ci-dessous.",
                                    entry.title
                                );
                                view.clear_youtube_link(cx);
                            } else {
                                view.report_error(
                                    Failure::Validation(
                                        "Cette vidéo n’est pas disponible. Vérifiez le lien et réessayez.".into(),
                                    ),
                                    cx,
                                );
                            }
                        }
                        SourceShape::Playlist => {
                            view.youtube_selection_url = Some(url.clone());
                            view.youtube_entries = inspection.entries;
                            view.status = if view.youtube_entries.is_empty() {
                                "Cette playlist ne contient aucune vidéo disponible.".into()
                            } else {
                                "Cochez les vidéos de cette playlist à importer.".into()
                            };
                        }
                    },
                    Ok(Ok(
                        Reply::AppUpdate(_)
                        | Reply::Updated
                        | Reply::Saved(_)
                        | Reply::OpenedPlaylist(_)
                        | Reply::PlaylistOperation { .. }
                        | Reply::Waveform(_)
                        | Reply::Prepared(_)
                        | Reply::Asset(_)
                        | Reply::PixelArt(_)
                        | Reply::Image(_)
                        | Reply::Images(_)
                        | Reply::Player(_)
                        | Reply::Data(_),
                    )) => view.report_error(
                        Failure::Validation(
                            "Impossible de lire les informations de cette vidéo. Réessayez.".into(),
                        ),
                        cx,
                    ),
                    Ok(Err(error)) => view.report_error(error, cx),
                    Err(error) => view.report_error(Failure::Storage(error.to_string()), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }
    /// Empty the link field once its import is queued, so a second click cannot import
    /// the same video twice.
    fn clear_youtube_link(&mut self, cx: &mut Context<Self>) {
        let input = self.youtube.clone();
        if let Some(window) = cx.windows().first().copied() {
            cx.defer(move |cx| {
                let _ = cx.update_window(window, |_, window, cx| {
                    input.update(cx, |input, cx| input.set_value("", window, cx));
                });
            });
        }
    }
}
pub fn render(view: &mut AppView, _: &mut Window, cx: &mut Context<AppView>) -> AnyElement {
    let mut body = div()
        .id("imports-content")
        .size_full()
        .p_7()
        .flex()
        .flex_col()
        .gap_4()
        .min_h_0()
        .child(div().text_2xl().child("Importer de l’audio"))
        .child(
            div()
                .flex()
                .gap_3()
                .child(Input::new(&view.youtube).disabled(view.youtube_import_pending).flex_1())
                .child(
                    Button::new("inspect-youtube")
                        .label(if view.youtube_import_pending { "Recherche…" } else { "Importer" })
                        .primary()
                        .disabled(view.youtube_import_pending)
                        .loading(view.youtube_import_pending)
                        .on_click(cx.listener(|v, _, _, cx| v.begin_youtube(cx))),
                ),
        )
        .child(div().text_sm().text_color(cx.theme().muted_foreground).child("Collez le lien d’une vidéo YouTube : son audio rejoint votre médiathèque. Pour une playlist YouTube, vous choisirez les vidéos à garder. Vous pouvez aussi glisser des fichiers audio dans cette fenêtre."));
    let choosing = view.youtube_selection_url.as_deref()
        == Some(view.youtube.read(cx).value().trim())
        && !view.youtube_entries.is_empty();
    if choosing {
        let choices = gpui_kit::uniform_list(
            "youtube-playlist-choices",
            view.youtube_entries.len(),
            cx.processor(
                |view: &mut AppView,
                 range: std::ops::Range<usize>,
                 _,
                 cx: &mut Context<AppView>| {
                    range
                        .map(|index| {
                            let entry = &view.youtube_entries[index];
                            let id = entry.id.clone();
                            div()
                                .h(px(36.))
                                .flex()
                                .items_center()
                                .min_w_0()
                                .child(
                                    Checkbox::new(element_id("youtube-entry", id.clone()))
                                        .accessibility_label(entry.title.clone())
                                        .child(div().truncate().child(entry.title.clone()))
                                        .checked(view.selected_entries.contains(&id))
                                        .on_click(cx.listener(move |v, checked, _, cx| {
                                            if *checked {
                                                v.selected_entries.insert(id.clone());
                                            } else {
                                                v.selected_entries.remove(&id);
                                            }
                                            cx.notify();
                                        })),
                                )
                                .into_any_element()
                        })
                        .collect()
                },
            ),
        )
        .h(px(180.))
        .flex_shrink_0();
        body = body
            .child(div().text_lg().child("Vidéos de la playlist"))
            .child(choices)
            .child(
                Button::new("import-youtube")
                    .label(format!(
                        "Importer la sélection ({})",
                        view.selected_entries.len()
                    ))
                    .primary()
                    .disabled(view.selected_entries.is_empty())
                    .on_click(cx.listener(|v, _, _, cx| {
                        let chosen: Vec<_> = v
                            .youtube_entries
                            .iter()
                            .filter(|e| v.selected_entries.contains(&e.id))
                            .cloned()
                            .collect();
                        let count = chosen.len();
                        let selections = chosen
                            .into_iter()
                            .map(|e| {
                                v.youtube_titles.insert(e.url.clone(), e.title);
                                ImportSelection::Youtube {
                                    url: e.url,
                                    entry: Some(e.id),
                                }
                            })
                            .collect();
                        v.send(Command::Import(selections), cx);
                        v.selected_entries.clear();
                        v.youtube_entries.clear();
                        v.youtube_selection_url = None;
                        v.status =
                            format!("{count} vidéo(s) en cours d’import. Suivez-les ci-dessous.");
                        v.clear_youtube_link(cx);
                        cx.notify();
                    })),
            )
    }
    let jobs_count = view
        .job_page
        .as_ref()
        .map_or(view.snapshot.jobs.len(), |v| v.len());
    if jobs_count == 0 {
        return body
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child("Vos imports apparaîtront ici, avec leur progression."),
            )
            .child(
                div().child(
                    Button::new("imports-add-file")
                        .label("Ajouter un fichier audio depuis mon ordinateur")
                        .on_click(cx.listener(|v, _, _, cx| v.add_files(cx))),
                ),
            )
            .into_any_element();
    }
    let mut history = div().flex().gap_2().child(format!(
        "Historique · {}–{}",
        view.job_offset + 1,
        view.job_offset + jobs_count
    ));
    if view.job_offset > 0 || jobs_count >= 200 {
        history = history
            .child(
                Button::new("jobs-newer")
                    .label("Plus récentes")
                    .disabled(view.job_offset == 0)
                    .on_click(cx.listener(|v, _, _, cx| {
                        v.load_job_page(v.job_offset.saturating_sub(200), cx)
                    })),
            )
            .child(
                Button::new("jobs-older")
                    .label("Plus anciennes")
                    .disabled(jobs_count < 200)
                    .on_click(cx.listener(|v, _, _, cx| v.load_job_page(v.job_offset + 200, cx))),
            );
    }
    body = body.child(history);
    let jobs = gpui_kit::uniform_list(
        "durable-job-list",
        jobs_count,
        cx.processor(
            |view: &mut AppView, range: std::ops::Range<usize>, _, cx: &mut Context<AppView>| {
                range
                    .map(|index| {
                        let job = &view
                            .job_page
                            .as_ref()
                            .map_or(&view.snapshot.jobs, |v| v.as_ref())[index];
                        let id = job.id;
                        let title = job_title(job, &view.snapshot, &view.youtube_titles);
                        let failed = matches!(job.state, JobState::Failed { .. });
                        let state_line = match &job.state {
                            JobState::Failed { failure } | JobState::Retry { failure, .. } => {
                                format!("{} · {}", job.state.label(), failure_text(failure))
                            }
                            JobState::Queued | JobState::Running { .. } => {
                                format!("{}…", job.state.label())
                            }
                            JobState::WaitingForAuthentication
                            | JobState::Conflict { .. }
                            | JobState::Reconciling
                            | JobState::Completed { .. }
                            | JobState::Cancelled => job.state.label().to_owned(),
                        };
                        let mut actions = div().flex().items_center().gap_2();
                        match &job.state {
                            JobState::Failed { .. } | JobState::Retry { .. } => {
                                actions = actions.child(
                                    Button::new(element_id("retry", id))
                                        .label("Réessayer")
                                        .on_click(cx.listener(move |v, _, _, cx| {
                                            v.send(Command::Retry(id), cx)
                                        })),
                                );
                            }
                            JobState::WaitingForAuthentication => {
                                actions = actions.child(
                                    Button::new(element_id("connect-job", id))
                                        .label("Se connecter")
                                        .on_click(
                                            cx.listener(|v, _, _, cx| v.send(Command::Connect, cx)),
                                        ),
                                );
                            }
                            JobState::Conflict { .. } | JobState::Reconciling => {
                                actions = actions.child(
                                    Button::new(element_id("resolve-job", id))
                                        .label("Vérifier la publication")
                                        .on_click(cx.listener(move |v, _, window, cx| {
                                            show_details(v, id, window, cx)
                                        })),
                                );
                            }
                            JobState::Completed { output } => match output {
                                JobOutput::Imported { media } => {
                                    let media = *media;
                                    if view.snapshot.media.iter().any(|item| item.id == media) {
                                        actions = actions.child(
                                            Button::new(element_id("open-imported-media", id))
                                                .label("Écouter dans ma médiathèque")
                                                .on_click(cx.listener(move |v, _, window, cx| {
                                                    v.navigate(Route::Library, cx);
                                                    v.select_media(media, window, cx);
                                                    cx.notify();
                                                })),
                                        );
                                    } else {
                                        actions = actions.child(
                                            div()
                                                .text_sm()
                                                .text_color(cx.theme().muted_foreground)
                                                .child("Média retiré de votre médiathèque"),
                                        );
                                    }
                                }
                                JobOutput::Encoded { .. } | JobOutput::Published { .. } => {}
                            },
                            JobState::Cancelled => {}
                            JobState::Queued | JobState::Running { .. } => {
                                actions = actions.child(
                                    Button::new(element_id("cancel", id))
                                        .label("Annuler")
                                        .on_click(cx.listener(move |v, _, _, cx| {
                                            v.send(Command::Cancel(id), cx)
                                        })),
                                );
                            }
                        }
                        if !matches!(job.state, JobState::Conflict { .. } | JobState::Reconciling) {
                            actions = actions.child(
                                Button::new(element_id("import-details", id))
                                    .label("Détails")
                                    .ghost()
                                    .on_click(cx.listener(move |v, _, window, cx| {
                                        show_details(v, id, window, cx)
                                    })),
                            );
                        }
                        div()
                            .w_full()
                            .h(px(128.))
                            .p_3()
                            .border_1()
                            .border_color(cx.theme().border)
                            .rounded_lg()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .truncate()
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(if failed {
                                        cx.theme().danger
                                    } else {
                                        cx.theme().muted_foreground
                                    })
                                    .truncate()
                                    .child(state_line),
                            )
                            .child(actions)
                            .into_any_element()
                    })
                    .collect()
            },
        ),
    )
    .w_full()
    .flex_1()
    .min_h_0();
    body.child(jobs).into_any_element()
}
fn job_title(job: &Job, snapshot: &Snapshot, youtube_titles: &BTreeMap<String, String>) -> String {
    if let JobState::Completed {
        output: JobOutput::Imported { media },
    } = &job.state
        && let Some(item) = snapshot.media.iter().find(|item| item.id == *media)
    {
        return item.title.clone();
    }
    match &job.request {
        JobRequest::Import { selection } => match selection {
            ImportSelection::LocalFile { path } => path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            ImportSelection::Youtube { url, .. } => youtube_titles.get(url).map_or_else(
                || "Vidéo YouTube".into(),
                |title| format!("YouTube · {title}"),
            ),
        },
        JobRequest::Encode { recipe } => snapshot
            .recipes
            .iter()
            .find(|item| item.id == *recipe)
            .and_then(|recipe| snapshot.media.iter().find(|item| item.id == recipe.media))
            .map_or_else(
                || "Préparation audio locale".into(),
                |item| format!("Préparation · {}", item.title),
            ),
        JobRequest::Publish { snapshot, .. } => format!("Publication · {}", snapshot.title),
    }
}
fn show_details(view: &mut AppView, id: JobId, window: &mut Window, cx: &mut Context<AppView>) {
    let Some(job) = view
        .job_page
        .as_ref()
        .map_or(&view.snapshot.jobs, |v| v.as_ref())
        .iter()
        .find(|job| job.id == id)
        .cloned()
    else {
        return;
    };
    let title = job_title(&job, &view.snapshot, &view.youtube_titles);
    let owner = cx.entity().downgrade();
    window.open_dialog(cx, move |dialog, _, cx| {
        let content = owner
            .update(cx, |view, cx| details_content(view, &job, cx))
            .unwrap_or_else(|_| {
                div()
                    .child("Cette médiathèque a été fermée.")
                    .into_any_element()
            });
        dialog.title(title.clone()).w(px(700.)).child(content)
    });
}
fn details_content(view: &mut AppView, job: &Job, cx: &mut Context<AppView>) -> AnyElement {
    let id = job.id;
    let mut body = div().flex().flex_col().gap_3().child(job.state.label());
    if let JobRequest::Import {
        selection: ImportSelection::Youtube { url, .. },
    } = &job.request
    {
        let source = url.clone();
        body = body
            .child(div().text_sm().child(format!("Lien source : {url}")))
            .child(
                Button::new(element_id("copy-import-url", id))
                    .label("Copier le lien source")
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(source.clone()))
                    })),
            );
    }
    match &job.state {
        JobState::Failed { failure } | JobState::Retry { failure, .. } => {
            body = body.child(failure_text(failure)).child(
                Button::new(element_id("retry-details", id))
                    .label("Réessayer")
                    .on_click(cx.listener(move |v, _, window, cx| {
                        v.send(Command::Retry(id), cx);
                        window.close_dialog(cx);
                    })),
            );
        }
        JobState::Conflict { .. } => {
            body = body.child(
                "La playlist a aussi été modifiée sur Yoto. Choisissez la version à conserver.",
            );
            for (choice, label) in [
                (ConflictChoice::KeepLocal, "Publier ma version"),
                (ConflictChoice::UseRemote, "Conserver la version Yoto"),
                (ConflictChoice::CreateCopy, "Créer une copie"),
            ] {
                body = body.child(Button::new(element_id(label, id)).label(label).on_click(
                    cx.listener(move |v, _, window, cx| {
                        v.send(Command::Resolve { job: id, choice }, cx);
                        window.close_dialog(cx);
                    }),
                ));
            }
        }
        JobState::Reconciling => {
            body = body.child("La réponse de Yoto est incertaine. Vérifiez la publication avant de la renvoyer.")
                .child(Button::new(element_id("reconcile", id)).label("Vérifier auprès de Yoto").primary().on_click(cx.listener(move |v, _, window, cx| { v.send(Command::Reconcile(id), cx); window.close_dialog(cx); })))
                .child(div().text_sm().child("Si votre compte a été déconnecté, vous pouvez associer explicitement l’ID de la carte déjà créée :"))
                .child(Input::new(&view.receipt_card))
                .child(Button::new(element_id("attach-receipt", id)).label("Associer cette carte").on_click(cx.listener(move |v, _, window, cx| match CardId::new(v.receipt_card.read(cx).value().to_string()) {
                    Ok(card) => { v.send(Command::AttachPublicationReceipt { job: id, card }, cx); window.close_dialog(cx); }
                    Err(error) => { v.error = Some(error); cx.notify(); }
                })));
        }
        JobState::Completed { output } => match output {
            JobOutput::Published { .. } => {
                body = body.child("Enregistré sur votre compte Yoto. Pour l’écouter sur une carte, associez cette playlist à une carte « Make Your Own » dans l’application Yoto.");
            }
            JobOutput::Imported { .. } | JobOutput::Encoded { .. } => {}
        },
        JobState::Queued
        | JobState::Running { .. }
        | JobState::WaitingForAuthentication
        | JobState::Cancelled => {}
    }
    body.into_any_element()
}
