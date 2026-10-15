use super::*;
use gpui_kit::component::WindowExt;
use gpui_kit::component::{checkbox::Checkbox, spinner::Spinner};
use yoto_infra::service::ConflictChoice;
impl AppView {
    pub fn begin_youtube(&mut self, cx: &mut Context<Self>) {
        if self.youtube_import_pending {
            return;
        }
        let url = self.youtube.read(cx).value().trim().to_owned();
        if url.is_empty() {
            self.report_error(
                Failure::Validation(tr!("Collez un lien YouTube pour importer son audio.").into()),
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
        self.status = tr!("Recherche du titre et des informations YouTube…").into();
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
                match joined(result).and_then(Reply::into_entries) {
                    Ok(inspection) => match inspection.kind {
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
                                view.status = tr!(
                                    "Import de « {title} » lancé. Suivez sa progression dans « Imports ».",
                                    title = entry.title
                                );
                                view.clear_youtube_link(cx);
                            } else {
                                view.report_error(
                                    Failure::Validation(
                                        tr!("Cette vidéo n’est pas disponible. Vérifiez le lien et réessayez.").into(),
                                    ),
                                    cx,
                                );
                            }
                        }
                        SourceShape::Playlist => {
                            view.youtube_selection_url = Some(url.clone());
                            view.youtube_entries = inspection.entries;
                            view.status = if view.youtube_entries.is_empty() {
                                tr!("Cette playlist ne contient aucune vidéo disponible.").into()
                            } else {
                                tr!("Cochez les vidéos de cette playlist à importer.").into()
                            };
                        }
                    },
                    Err(error) => view.report_error(error, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }
    /// Empty the link field once its import is queued, so a second click cannot import
    /// the same video twice.
    pub(super) fn clear_youtube_link(&mut self, cx: &mut Context<Self>) {
        let input = self.youtube.clone();
        if let Some(window) = cx.windows().first().copied() {
            cx.defer(move |cx| {
                let _ = cx.update_window(window, |_, window, cx| {
                    input.update(cx, |input, cx| input.set_value("", window, cx));
                });
            });
        }
    }
    /// Remove finished activities from the list. An older page is a copy taken when it
    /// was opened, so it is updated here as well.
    fn dismiss_jobs(&mut self, ids: Vec<JobId>, cx: &mut Context<Self>) {
        if let Some(page) = self.job_page.as_mut() {
            for job in Arc::make_mut(page).iter_mut() {
                if ids.contains(&job.id) {
                    job.dismissed = true;
                }
            }
        }
        self.send(Command::DismissJobs(ids), cx);
        cx.notify();
    }
}
pub fn render(view: &mut AppView, _: &mut Window, cx: &mut Context<AppView>) -> AnyElement {
    use theme::{Height, Look};
    let pending = view.youtube_import_pending;
    let youtube = theme::card()
        .flex_1()
        .min_w(px(320.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(theme::icon_tile(
                    IconName::MonitorPlay,
                    &theme::PEACH,
                    36.,
                    12.,
                ))
                .child(theme::heading(tr!("Lien YouTube"), 18.)),
        )
        .child(
            div()
                .flex()
                .gap(px(8.))
                .child(
                    theme::field(Input::new(&view.youtube))
                        .h(px(50.))
                        .disabled(pending)
                        .flex_1(),
                )
                .child(theme::pill(
                    Button::new("inspect-youtube")
                        .label(if pending { tr!("Recherche…") } else { tr!("Importer") })
                        .loading(pending)
                        .on_click(cx.listener(|v, _, _, cx| v.begin_youtube(cx))),
                    Look::Primary,
                    Height::Large,
                    pending,
                    cx,
                )
                .h(px(50.))),
        )
        .child(theme::note(
            tr!("Une vidéo ou une playlist publique. On récupère seulement l’audio ; pour une playlist, vous choisirez les vidéos à garder."),
        ));
    let drop_zone = {
        use gpui_kit::component::button::{ButtonCustomVariant, ButtonRounded};
        Button::new("imports-add-file")
            .custom(
                ButtonCustomVariant::new(cx)
                    .color(theme::c(theme::SAND))
                    .hover(theme::c(theme::CREAM))
                    .active(theme::c(theme::SAND)),
            )
            .accessibility_label(tr!("Ajouter un fichier audio depuis mon ordinateur"))
            .bg(theme::c(theme::SAND))
            .rounded(ButtonRounded::Size(px(22.)))
            .border_2()
            .border_dashed()
            .border_color(theme::c(theme::BOX_BORDER))
            .w(px(300.))
            .flex_shrink_0()
            .h_auto()
            .min_h(px(170.))
            .p(px(16.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .size(px(44.))
                            .rounded_full()
                            .bg(theme::c(theme::SURFACE))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                gpui_kit::component::Icon::new(IconName::Upload)
                                    .size(px(20.))
                                    .text_color(theme::c(theme::PEACH.icon)),
                            ),
                    )
                    .child(theme::heading(tr!("Ajouter un fichier audio"), 17.))
                    .child(
                        theme::meta(tr!("ou glissez-le ici · MP3, M4A, WAV…")).text_size(px(14.)),
                    ),
            )
            .on_click(cx.listener(|v, _, _, cx| v.add_files(cx)))
    };
    let mut main = div()
        .id("imports-content")
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_y_scroll()
        .py(px(24.))
        .px(px(28.))
        .flex()
        .flex_col()
        .gap(px(18.))
        .child(theme::screen_header(
            tr!("Importer de l’audio"),
            tr!("Depuis YouTube ou depuis un fichier de votre ordinateur."),
        ))
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap(px(14.))
                .child(youtube)
                .child(drop_zone),
        );
    let choosing = view.youtube_selection_url.as_deref()
        == Some(view.youtube.read(cx).value().trim())
        && !view.youtube_entries.is_empty();
    if choosing {
        let all_selected = view.selected_entries.len() == view.youtube_entries.len();
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
                                .h(px(48.))
                                .flex()
                                .items_center()
                                .gap(px(12.))
                                .min_w_0()
                                .child(
                                    Checkbox::new(element_id("youtube-entry", id.clone()))
                                        .large()
                                        .accessibility_label(entry.title.clone())
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
                                .child(
                                    div()
                                        .w(px(64.))
                                        .h(px(36.))
                                        .flex_shrink_0()
                                        .rounded(px(8.))
                                        .bg(theme::c(theme::family_for(&entry.id).soft)),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .font_weight(FontWeight::BOLD)
                                        .child(entry.title.clone()),
                                )
                                .children(
                                    entry
                                        .duration
                                        .map(|d| theme::meta(wording::position_text(d.seconds()))),
                                )
                                .into_any_element()
                        })
                        .collect()
                },
            ),
        )
        .h(px(240.))
        .flex_shrink_0();
        main = main.child(
            theme::card()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(
                            div()
                                .flex_1()
                                .child(theme::heading(tr!("Vidéos de la playlist"), 18.))
                                .child(theme::meta(tr!("Choisissez les vidéos à importer."))),
                        )
                        .child(theme::pill(
                            Button::new("youtube-select-all")
                                .label(if all_selected {
                                    tr!("Tout désélectionner")
                                } else {
                                    tr!("Tout sélectionner")
                                })
                                .on_click(cx.listener(move |v, _, _, cx| {
                                    if all_selected {
                                        v.selected_entries.clear();
                                    } else {
                                        v.selected_entries =
                                            v.youtube_entries.iter().map(|e| e.id.clone()).collect();
                                    }
                                    cx.notify();
                                })),
                            Look::Link(&theme::PEACH),
                            Height::Small,
                            false,
                            cx,
                        )),
                )
                .child(choices)
                .child(
                    theme::dialog_actions()
                        .child(theme::pill(
                            Button::new("cancel-youtube-selection")
                                .label(tr!("Annuler"))
                                .on_click(cx.listener(|v, _, _, cx| {
                                    v.youtube_entries.clear();
                                    v.selected_entries.clear();
                                    v.youtube_selection_url = None;
                                    v.clear_youtube_link(cx);
                                    cx.notify();
                                })),
                            Look::Outline,
                            Height::Large,
                            false,
                            cx,
                        ))
                        .child(theme::pill(
                            Button::new("import-youtube")
                                .label(tr!(
                                    "Importer la sélection ({count})",
                                    count = view.selected_entries.len()
                                ))
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
                                    v.status = trn!(
                                        "{count} vidéo en cours d’import. Suivez sa progression dans « Imports ».",
                                        "{count} vidéos en cours d’import. Suivez leur progression dans « Imports ».",
                                        count
                                    );
                                    v.clear_youtube_link(cx);
                                    cx.notify();
                                })),
                            Look::Primary,
                            Height::Large,
                            view.selected_entries.is_empty(),
                            cx,
                        )),
                ),
        );
    }
    div()
        .size_full()
        .flex()
        .child(main)
        .child(activity(view, cx))
        .into_any_element()
}

/// The right column: every import, preparation and sending, newest first.
fn activity(view: &AppView, cx: &mut Context<AppView>) -> AnyElement {
    use theme::{Height, Look};
    let page = view
        .job_page
        .as_ref()
        .map_or(&view.snapshot.jobs, |v| v.as_ref());
    let jobs: Vec<&Job> = page.iter().filter(|job| !job.dismissed).collect();
    let finished: Vec<JobId> = jobs
        .iter()
        .filter(|job| dismissable(&job.state))
        .map(|job| job.id)
        .collect();
    let mut aside = div()
        .w(px(380.))
        .flex_shrink_0()
        .h_full()
        .border_l_2()
        .border_color(theme::c(theme::DIVIDER))
        .flex()
        .flex_col()
        .child(
            div()
                .pt(px(24.))
                .px(px(22.))
                .pb(px(12.))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(10.))
                .child(theme::heading(tr!("Activités"), 22.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .children((!jobs.is_empty()).then(|| {
                            theme::meta(format!(
                                "{}–{}",
                                view.job_offset + 1,
                                view.job_offset + jobs.len()
                            ))
                        }))
                        .children((finished.len() > 1).then(|| {
                            theme::pill(
                                Button::new("dismiss-finished-jobs")
                                    .label(tr!("Tout effacer"))
                                    .tooltip(tr!("Retirer de la liste les activités terminées"))
                                    .on_click(cx.listener(move |v, _, _, cx| {
                                        v.dismiss_jobs(finished.clone(), cx)
                                    })),
                                Look::Ghost,
                                Height::Tiny,
                                false,
                                cx,
                            )
                        })),
                ),
        );
    if jobs.is_empty() {
        return aside
            .child(div().px(px(22.)).child(theme::note(tr!(
                "Vos imports apparaîtront ici, avec leur progression."
            ))))
            .into_any_element();
    }
    let mut list = div()
        .id("durable-job-list")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .px(px(22.))
        .pb(px(12.))
        .flex()
        .flex_col()
        .gap(px(12.));
    for job in jobs {
        list = list.child(job_card(view, job, cx));
    }
    aside = aside.child(list);
    if view.job_offset > 0 || page.len() >= 200 {
        aside = aside.child(
            div()
                .px(px(22.))
                .py(px(12.))
                .flex()
                .justify_between()
                .child(theme::pill(
                    Button::new("jobs-newer")
                        .label(tr!("Plus récentes"))
                        .on_click(cx.listener(|v, _, _, cx| {
                            v.load_job_page(v.job_offset.saturating_sub(200), cx)
                        })),
                    Look::Soft(&theme::LIGHT_SAND),
                    Height::Small,
                    view.job_offset == 0,
                    cx,
                ))
                .child(theme::pill(
                    Button::new("jobs-older")
                        .label(tr!("Plus anciennes"))
                        .on_click(
                            cx.listener(|v, _, _, cx| v.load_job_page(v.job_offset + 200, cx)),
                        ),
                    Look::Soft(&theme::LIGHT_SAND),
                    Height::Small,
                    page.len() < 200,
                    cx,
                )),
        );
    }
    aside.into_any_element()
}
fn job_card(view: &AppView, job: &Job, cx: &mut Context<AppView>) -> AnyElement {
    use theme::{Height, Look};
    let id = job.id;
    let title = job_title(job, &view.snapshot, &view.youtube_titles);
    let family = match job.request {
        JobRequest::Publish { .. } => &theme::SKY,
        JobRequest::Import { .. } | JobRequest::Encode { .. } => &theme::BUTTER,
    };
    let details = theme::pill(
        Button::new(element_id("import-details", id))
            .label(tr!("Détails"))
            .on_click(cx.listener(move |v, _, window, cx| show_details(v, id, window, cx))),
        Look::Ghost,
        Height::Tiny,
        false,
        cx,
    );
    let spinner = || {
        Spinner::new()
            .color(theme::c(family.accent))
            .into_any_element()
    };
    let rose_mark = || {
        div()
            .size(px(26.))
            .flex_shrink_0()
            .rounded_full()
            .bg(theme::c(theme::ROSE.soft))
            .text_color(theme::c(theme::ROSE.ink))
            .font_weight(FontWeight::EXTRA_BOLD)
            .flex()
            .items_center()
            .justify_center()
            .child("!")
            .into_any_element()
    };
    let (mark, line, line_color, actions): (AnyElement, String, u32, Vec<AnyElement>) = match &job
        .state
    {
        JobState::Queued => (
            spinner(),
            tr!("En attente…").into(),
            family.ink,
            vec![
                theme::pill(
                    Button::new(element_id("cancel", id))
                        .label(tr!("Annuler"))
                        .on_click(cx.listener(move |v, _, _, cx| v.send(Command::Cancel(id), cx))),
                    Look::Outline,
                    Height::Tiny,
                    false,
                    cx,
                )
                .into_any_element(),
                details.into_any_element(),
            ],
        ),
        JobState::Running { phase } => (
            spinner(),
            match &job.request {
                JobRequest::Publish { snapshot, .. } => {
                    tr!(
                        "{phase} · « {title} »",
                        phase = phase_text(phase),
                        title = snapshot.title
                    )
                }
                JobRequest::Import { .. } | JobRequest::Encode { .. } => {
                    format!("{}…", phase_text(phase))
                }
            },
            family.ink,
            vec![
                theme::pill(
                    Button::new(element_id("cancel", id))
                        .label(tr!("Annuler"))
                        .on_click(cx.listener(move |v, _, _, cx| v.send(Command::Cancel(id), cx))),
                    Look::Outline,
                    Height::Tiny,
                    false,
                    cx,
                )
                .into_any_element(),
                details.into_any_element(),
            ],
        ),
        JobState::Failed { failure } | JobState::Retry { failure, .. } => (
            rose_mark(),
            if matches!(job.state, JobState::Retry { .. }) {
                format!(
                    "{} · {}",
                    wording::labels::job_state(&job.state),
                    failure_text(failure)
                )
            } else {
                failure_text(failure)
            },
            theme::MUTED,
            vec![
                theme::pill(
                    Button::new(element_id("retry", id))
                        .label(tr!("Réessayer"))
                        .on_click(cx.listener(move |v, _, _, cx| v.send(Command::Retry(id), cx))),
                    Look::Outline,
                    Height::Tiny,
                    false,
                    cx,
                )
                .into_any_element(),
                details.into_any_element(),
            ],
        ),
        JobState::WaitingForAuthentication => (
            rose_mark(),
            wording::labels::job_state(&job.state).into(),
            theme::MUTED,
            vec![
                theme::pill(
                    Button::new(element_id("connect-job", id))
                        .label(tr!("Se connecter"))
                        .on_click(cx.listener(|v, _, _, cx| v.send(Command::Connect, cx))),
                    Look::Primary,
                    Height::Tiny,
                    false,
                    cx,
                )
                .into_any_element(),
                details.into_any_element(),
            ],
        ),
        JobState::Conflict { .. } => {
            let mut actions = vec![];
            for (choice, look) in CONFLICT_CHOICES {
                let label = conflict_label(choice);
                actions.push(
                    theme::pill(
                        Button::new(element_id(label, id))
                            .label(label)
                            .on_click(cx.listener(move |v, _, _, cx| {
                                v.send(Command::Resolve { job: id, choice }, cx)
                            })),
                        look,
                        Height::Tiny,
                        false,
                        cx,
                    )
                    .into_any_element(),
                );
            }
            let playlist = match &job.request {
                JobRequest::Publish { snapshot, .. } => snapshot.title.clone(),
                JobRequest::Import { .. } | JobRequest::Encode { .. } => title.clone(),
            };
            return theme::card()
                    .p(px(14.))
                    .gap(px(10.))
                    .rounded(px(18.))
                    .border_t_4()
                    .border_color(theme::c(theme::BUTTER.accent))
                    .child(
                        div()
                            .font_weight(FontWeight::EXTRA_BOLD)
                            .child(tr!("Conflit sur « {playlist} »", playlist = playlist)),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(relative(1.45))
                            .text_color(theme::c(theme::INK_SOFT))
                            .child(tr!("La playlist a aussi été modifiée sur Yoto. Choisissez la version à conserver.")),
                    )
                    .child(div().flex().flex_wrap().gap(px(6.)).children(actions))
                    .into_any_element();
        }
        JobState::Reconciling => (
            rose_mark(),
            wording::labels::job_state(&job.state).into(),
            theme::MUTED,
            vec![
                theme::pill(
                    Button::new(element_id("resolve-job", id))
                        .label(tr!("Vérifier la publication"))
                        .on_click(
                            cx.listener(move |v, _, window, cx| show_details(v, id, window, cx)),
                        ),
                    Look::Primary,
                    Height::Tiny,
                    false,
                    cx,
                )
                .into_any_element(),
            ],
        ),
        JobState::Completed { output } => match output {
            JobOutput::Imported { media } => {
                let media = *media;
                let present = view.snapshot.media.iter().any(|item| item.id == media);
                (
                    theme::done_mark(26.).into_any_element(),
                    if present {
                        tr!("Prêt dans la médiathèque").into()
                    } else {
                        tr!("Média retiré de votre médiathèque").into()
                    },
                    if present {
                        theme::MINT.ink
                    } else {
                        theme::MUTED
                    },
                    if present {
                        vec![
                            theme::pill(
                                Button::new(element_id("open-imported-media", id))
                                    .label(tr!("Écouter"))
                                    .on_click(cx.listener(move |v, _, window, cx| {
                                        v.navigate(Route::Library, cx);
                                        v.select_media(media, window, cx);
                                        cx.notify();
                                    })),
                                Look::Soft(&theme::MINT),
                                Height::Tiny,
                                false,
                                cx,
                            )
                            .into_any_element(),
                        ]
                    } else {
                        vec![]
                    },
                )
            }
            JobOutput::Encoded { .. } => (
                theme::done_mark(26.).into_any_element(),
                tr!("Version préparée").into(),
                theme::MINT.ink,
                vec![],
            ),
            JobOutput::Published { .. } => (
                theme::done_mark(26.).into_any_element(),
                tr!("Enregistré sur Yoto").into(),
                theme::MINT.ink,
                vec![details.into_any_element()],
            ),
        },
        JobState::Cancelled => (
            div()
                .size(px(26.))
                .flex_shrink_0()
                .rounded_full()
                .bg(theme::c(theme::SAND))
                .into_any_element(),
            tr!("Annulé").into(),
            theme::MUTED,
            vec![details.into_any_element()],
        ),
    };
    // Running tasks keep their « Annuler »; only what is over can leave the list.
    let dismiss = dismissable(&job.state).then(|| {
        theme::round(
            Button::new(element_id("dismiss-job", id))
                .icon(IconName::X)
                .accessibility_label(tr!("Retirer de la liste"))
                .tooltip(tr!("Retirer de la liste"))
                .on_click(cx.listener(move |v, _, _, cx| v.dismiss_jobs(vec![id], cx))),
            Look::Ghost,
            28.,
            false,
            cx,
        )
    });
    let mut card = theme::card()
        .p(px(14.))
        .gap(px(8.))
        .rounded(px(18.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(mark)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .font_weight(FontWeight::EXTRA_BOLD)
                        .truncate()
                        .child(title),
                )
                .children(dismiss),
        )
        .child(
            div()
                .text_size(px(13.))
                .font_weight(FontWeight::BOLD)
                .text_color(theme::c(line_color))
                .child(line),
        );
    if !actions.is_empty() {
        card = card.child(div().flex().flex_wrap().gap(px(6.)).children(actions));
    }
    card.into_any_element()
}
/// Finished, failed and cancelled tasks can be removed from the list; the others are
/// still waiting on something and keep their own actions.
fn dismissable(state: &JobState) -> bool {
    match state {
        JobState::Completed { .. } | JobState::Failed { .. } | JobState::Cancelled => true,
        JobState::Queued
        | JobState::Running { .. }
        | JobState::Retry { .. }
        | JobState::WaitingForAuthentication
        | JobState::Conflict { .. }
        | JobState::Reconciling => false,
    }
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
                || tr!("Vidéo YouTube").into(),
                |title| format!("YouTube · {title}"),
            ),
        },
        JobRequest::Encode { recipe } => snapshot
            .recipes
            .iter()
            .find(|item| item.id == *recipe)
            .and_then(|recipe| snapshot.media.iter().find(|item| item.id == recipe.media))
            .map_or_else(
                || tr!("Préparation audio locale").into(),
                |item| tr!("Préparation · {title}", title = item.title),
            ),
        JobRequest::Publish { snapshot, .. } => {
            tr!("Publication · {title}", title = snapshot.title)
        }
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
                    .child(tr!("Cette médiathèque a été fermée."))
                    .into_any_element()
            });
        dialog.title(title.clone()).w(px(700.)).child(content)
    });
}
/// The three answers to a playlist changed both here and on Yoto, in the order offered.
const CONFLICT_CHOICES: [(ConflictChoice, theme::Look); 3] = [
    (ConflictChoice::KeepLocal, theme::Look::Primary),
    (ConflictChoice::UseRemote, theme::Look::Outline),
    (ConflictChoice::CreateCopy, theme::Look::Ghost),
];
/// One label per conflict answer, shared by the job card and its details.
pub(super) fn conflict_label(choice: ConflictChoice) -> &'static str {
    match choice {
        ConflictChoice::KeepLocal => tr!("Publier ma version"),
        ConflictChoice::UseRemote => tr!("Garder la version Yoto"),
        ConflictChoice::CreateCopy => tr!("Créer une copie"),
    }
}
fn details_content(view: &mut AppView, job: &Job, cx: &mut Context<AppView>) -> AnyElement {
    use theme::{Height, Look};
    let id = job.id;
    let state_family = match &job.state {
        JobState::Completed { .. } => &theme::MINT,
        JobState::Failed { .. }
        | JobState::Retry { .. }
        | JobState::WaitingForAuthentication
        | JobState::Reconciling => &theme::ROSE,
        JobState::Conflict { .. } => &theme::BUTTER,
        JobState::Queued | JobState::Running { .. } => &theme::SKY,
        JobState::Cancelled => &theme::STONE,
    };
    let actions = || div().flex().flex_wrap().gap(px(8.));
    let mut body = div()
        .flex()
        .flex_col()
        .gap(px(14.))
        .child(div().flex().child(theme::tag(
            wording::labels::job_state(&job.state),
            state_family,
        )));
    if let JobRequest::Import {
        selection: ImportSelection::Youtube { url, .. },
    } = &job.request
    {
        let source = url.clone();
        body = body.child(
            theme::inset()
                .child(
                    div()
                        .font_weight(FontWeight::EXTRA_BOLD)
                        .child(tr!("Vidéo YouTube d’origine")),
                )
                .child(theme::meta(url.clone()).truncate())
                .child(
                    div().flex().child(theme::pill(
                        Button::new(element_id("copy-import-url", id))
                            .icon(IconName::Link)
                            .label(tr!("Copier le lien"))
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(source.clone()))
                            })),
                        Look::Outline,
                        Height::Small,
                        false,
                        cx,
                    )),
                ),
        );
    }
    match &job.state {
        JobState::Failed { failure } | JobState::Retry { failure, .. } => {
            body = body
                .child(
                    theme::inset().bg(theme::c(theme::ROSE.soft)).child(
                        div()
                            .text_color(theme::c(theme::ROSE.ink))
                            .font_weight(FontWeight::BOLD)
                            .child(failure_text(failure)),
                    ),
                )
                .child(
                    actions().child(theme::pill(
                        Button::new(element_id("retry-details", id))
                            .label(tr!("Réessayer"))
                            .on_click(cx.listener(move |v, _, window, cx| {
                                v.send(Command::Retry(id), cx);
                                window.close_dialog(cx);
                            })),
                        Look::Primary,
                        Height::Medium,
                        false,
                        cx,
                    )),
                );
        }
        JobState::Conflict { .. } => {
            let mut choices = actions();
            for (choice, look) in CONFLICT_CHOICES {
                let label = conflict_label(choice);
                choices = choices.child(theme::pill(
                    Button::new(element_id(label, id))
                        .label(label)
                        .on_click(cx.listener(move |v, _, window, cx| {
                            v.send(Command::Resolve { job: id, choice }, cx);
                            window.close_dialog(cx);
                        })),
                    look,
                    Height::Medium,
                    false,
                    cx,
                ));
            }
            body = body
                .child(theme::note(tr!(
                    "La playlist a aussi été modifiée sur Yoto. Choisissez la version à conserver."
                )))
                .child(choices);
        }
        JobState::Reconciling => {
            body = body
                .child(theme::note(
                    tr!("Yoto n’a pas confirmé cet envoi. Vérifiez-le avant de le renvoyer, pour ne pas créer la playlist deux fois."),
                ))
                .child(actions().child(theme::pill(
                    Button::new(element_id("reconcile", id))
                        .label(tr!("Vérifier auprès de Yoto"))
                        .on_click(cx.listener(move |v, _, window, cx| {
                            v.send(Command::Reconcile(id), cx);
                            window.close_dialog(cx);
                        })),
                    Look::Primary,
                    Height::Medium,
                    false,
                    cx,
                )))
                .child(theme::note(
                    tr!("Vous avez regardé dans l’application Yoto et la playlist vous convient ? Gardez-la telle quelle : rien n’est renvoyé, et la playlist se met à jour ici."),
                ))
                .child(actions().child(theme::pill(
                    Button::new(element_id("keep-yoto", id))
                        .label(tr!("Garder la version de Yoto"))
                        .on_click(cx.listener(move |v, _, window, cx| {
                            v.send(Command::KeepYotoVersion(id), cx);
                            window.close_dialog(cx);
                        })),
                    Look::Outline,
                    Height::Medium,
                    false,
                    cx,
                )));
            if view.advanced {
                body = body.child(
                    theme::inset()
                        .child(
                            div()
                                .font_weight(FontWeight::EXTRA_BOLD)
                                .child(tr!("La carte existe déjà ?")),
                        )
                        .child(theme::note(
                            tr!("Si l’envoi a créé une carte avant une déconnexion, collez son identifiant (affiché dans l’application Yoto) pour la relier à cet envoi."),
                        ))
                        .child(theme::small_field(Input::new(&view.receipt_card)))
                        .child(div().flex().child(theme::pill(
                            Button::new(element_id("attach-receipt", id))
                                .label(tr!("Relier cette carte"))
                                .on_click(cx.listener(move |v, _, window, cx| {
                                    match CardId::new(v.receipt_card.read(cx).value().trim()) {
                                        Ok(card) => {
                                            v.send(
                                                Command::AttachPublicationReceipt { job: id, card },
                                                cx,
                                            );
                                            window.close_dialog(cx);
                                        }
                                        Err(_) => v.report_error(
                                            Failure::Validation(
                                                tr!("Collez l’identifiant de la carte, tel qu’il apparaît dans l’application Yoto.")
                                                    .into(),
                                            ),
                                            cx,
                                        ),
                                    }
                                })),
                            Look::Outline,
                            Height::Small,
                            false,
                            cx,
                        ))),
                );
            }
        }
        JobState::Completed { output } => match output {
            JobOutput::Published { .. } => {
                body = body.child(theme::note(
                    tr!("Enregistré sur votre compte Yoto. Pour l’écouter sur une carte, associez cette playlist à une carte « Make Your Own » dans l’application Yoto."),
                ));
            }
            JobOutput::Imported { .. } | JobOutput::Encoded { .. } => {}
        },
        JobState::Queued
        | JobState::Running { .. }
        | JobState::WaitingForAuthentication
        | JobState::Cancelled => {}
    }
    body.child(div().flex().justify_end().child(theme::dialog_close(
        element_id("close-job-details-footer", id),
        theme::pill(
            Button::new(element_id("close-job-details", id)).label(tr!("Fermer")),
            Look::Outline,
            Height::Medium,
            false,
            cx,
        ),
    )))
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn job(request: JobRequest, state: JobState) -> Job {
        Job {
            id: JobId::new(),
            request,
            state,
            attempts: 0,
            created: 0,
            checkpoint: None,
            dismissed: false,
        }
    }
    #[core::prelude::v1::test]
    fn youtube_imports_show_the_title_learned_while_inspecting() {
        let url = "https://www.youtube.com/watch?v=abc".to_owned();
        let request = JobRequest::Import {
            selection: ImportSelection::Youtube {
                url: url.clone(),
                entry: None,
            },
        };
        let snapshot = Snapshot::default();
        let mut titles = BTreeMap::new();
        let queued = job(request.clone(), JobState::Queued);
        assert_eq!(job_title(&queued, &snapshot, &titles), "Vidéo YouTube");
        titles.insert(url, "La petite sirène".to_owned());
        assert_eq!(
            job_title(&queued, &snapshot, &titles),
            "YouTube · La petite sirène"
        );
        let local = job(
            JobRequest::Import {
                selection: ImportSelection::LocalFile {
                    path: "/musique/berceuse.mp3".into(),
                },
            },
            JobState::Queued,
        );
        assert_eq!(job_title(&local, &snapshot, &titles), "berceuse.mp3");
    }
}
