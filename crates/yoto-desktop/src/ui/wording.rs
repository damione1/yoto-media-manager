//! Parent-facing wording for failures and durations, in the interface language.
//!
//! The service and domain crates report failures in developer English (they are shared
//! with the MCP server). Every message the desktop app shows goes through `failure_text`
//! so a non-technical parent reads a plain explanation and what to do next.
use super::*;
use gpui_kit::component::{WindowExt, button::ButtonVariant};

/// The product name, the same in the window, the sidebar and every dialog.
pub const PRODUCT_NAME: &str = "YotoMediaManager";

/// Plain-language text for a failure, with the next step when there is one.
pub fn failure_text(failure: &Failure) -> String {
    let known = |message: &str| known_message(message).map(str::to_owned);
    match failure {
        Failure::Validation(message) => known(message)
            .or_else(|| yoto_rejected_text(message))
            .unwrap_or_else(|| message.clone()),
        Failure::Media(message) => known(message).unwrap_or_else(|| {
            tr!(
                "L’audio n’a pas pu être traité. Détail : {message}",
                message = message
            )
        }),
        Failure::Storage(message) => known(message).unwrap_or_else(|| {
            tr!(
                "Un problème est survenu sur cet ordinateur. Détail : {message}",
                message = message
            )
        }),
        Failure::Tool(message) => tr!(
            "Un composant de l’application est manquant ou endommagé. Réinstallez l’application. Détail : {message}",
            message = message
        ),
        Failure::Connection => {
            tr!("Pas de connexion Internet. Vérifiez votre réseau puis réessayez.").into()
        }
        Failure::Authentication => {
            tr!("Connectez votre compte Yoto dans « Compte » pour continuer.").into()
        }
        Failure::Permission(message) => {
            known(message).unwrap_or_else(|| permission_denied().into())
        }
        Failure::RateLimit {
            retry_after_seconds,
        } => {
            tr!(
                "Yoto a reçu trop de demandes. Réessayez dans {retry_after_seconds} secondes.",
                retry_after_seconds = retry_after_seconds
            )
        }
        Failure::Compatibility(message) => known(message).unwrap_or_else(|| {
            tr!(
                "Yoto a répondu d’une manière inattendue. Réessayez plus tard. Détail : {message}",
                message = message
            )
        }),
        Failure::Conflict => {
            tr!("Cette playlist a changé sur Yoto. Rechargez-la avant de continuer.").into()
        }
        Failure::Ambiguous => ambiguous().into(),
        Failure::Cancelled => tr!("Opération annulée.").into(),
        Failure::Transcode {
            track,
            problem,
            percent,
        } => transcode_text(track, *problem, *percent),
    }
}

/// A refusal Yoto did not explain in terms the app recognises, with what Yoto answered.
fn yoto_rejected_text(message: &str) -> Option<String> {
    let (status, said) = yoto_domain::messages::parse_yoto_rejected(message)?;
    Some(match said {
        Some(said) => tr!(
            "Yoto a refusé la demande (code {status}). Réponse de Yoto : {said}",
            status = status,
            said = said
        ),
        None => tr!("Yoto a refusé la demande (code {status}).", status = status),
    })
}

/// Why Yoto's conversion of a track did not finish, naming the track.
fn transcode_text(track: &str, problem: TranscodeProblem, percent: Option<u8>) -> String {
    match (problem, percent) {
        (TranscodeProblem::Failed, _) => tr!(
            "Yoto n’a pas pu convertir la piste « {track} ». Préparez de nouveau l’audio, puis réessayez.",
            track = track
        ),
        (TranscodeProblem::Stalled, Some(percent)) => tr!(
            "La conversion de la piste « {track} » par Yoto est restée bloquée à {percent} %, même après un nouvel envoi. Réessayez plus tard.",
            track = track,
            percent = percent
        ),
        (TranscodeProblem::Stalled, None) => tr!(
            "La conversion de la piste « {track} » par Yoto est restée bloquée, même après un nouvel envoi. Réessayez plus tard.",
            track = track
        ),
        (TranscodeProblem::TimedOut, Some(percent)) => tr!(
            "Yoto n’a pas fini de convertir la piste « {track} » dans le temps prévu (arrêt à {percent} %). Réessayez plus tard.",
            track = track,
            percent = percent
        ),
        (TranscodeProblem::TimedOut, None) => tr!(
            "Yoto n’a pas fini de convertir la piste « {track} » dans le temps prévu. Réessayez plus tard.",
            track = track
        ),
    }
}

/// What a running job is doing, with Yoto's conversion progress when it reports one. A
/// publication sending several tracks (a media cut into parts) shows one count and one
/// percentage over all of them, whichever track is moving.
pub fn phase_text(phase: &JobPhase) -> String {
    match phase {
        JobPhase::Uploading {
            progress: Some(progress),
            ..
        }
        | JobPhase::Ingesting {
            progress: Some(progress),
            ..
        } if progress.total > 1 => tr!(
            "Envoi à Yoto : {done}/{total} pistes · {percent} %",
            done = progress.done,
            total = progress.total,
            percent = progress.percent
        ),
        JobPhase::Ingesting {
            percent: Some(percent),
            ..
        } => tr!("Traitement par Yoto ({percent} %)", percent = percent),
        JobPhase::Acquiring
        | JobPhase::Analyzing
        | JobPhase::Encoding
        | JobPhase::Uploading { .. }
        | JobPhase::Ingesting { percent: None, .. }
        | JobPhase::Committing
        | JobPhase::Verifying { .. } => labels::job_phase(phase).into(),
    }
}

/// Names of the shared domain states and places. The domain crate keeps its own labels for
/// other clients; the desktop app words them here, in its interface language.
pub mod labels {
    use yoto_domain::*;
    pub fn job_phase(phase: &JobPhase) -> &'static str {
        match phase {
            JobPhase::Acquiring => tr!("Téléchargement"),
            JobPhase::Analyzing => tr!("Analyse de l’audio"),
            JobPhase::Encoding => tr!("Préparation de l’audio"),
            JobPhase::Uploading { .. } => tr!("Envoi de l’audio à Yoto"),
            JobPhase::Ingesting { .. } => tr!("Traitement par Yoto"),
            JobPhase::Committing => tr!("Enregistrement de la playlist"),
            JobPhase::Verifying { .. } => tr!("Vérification finale"),
        }
    }
    pub fn job_state(state: &JobState) -> &'static str {
        match state {
            JobState::Queued => tr!("En attente"),
            JobState::Running { phase } => job_phase(phase),
            JobState::Retry { .. } => tr!("Nouvel essai automatique prévu"),
            JobState::WaitingForAuthentication => tr!("Connexion au compte Yoto requise"),
            JobState::Conflict { .. } => tr!("Modifiée aussi sur Yoto · à vérifier"),
            JobState::Reconciling => tr!("Résultat à vérifier auprès de Yoto"),
            JobState::Completed { .. } => tr!("Terminé"),
            JobState::Failed { .. } => tr!("Échec"),
            JobState::Cancelled => tr!("Annulé"),
        }
    }
    pub fn route(route: Route) -> &'static str {
        match route {
            Route::Library => tr!("Médiathèque"),
            Route::Playlists => tr!("Playlists"),
            Route::Imports => tr!("Imports"),
            Route::Players => tr!("Lecteurs"),
            Route::Account => tr!("Compte"),
        }
    }
    pub fn remote_collection(collection: RemoteCollection) -> &'static str {
        match collection {
            RemoteCollection::Playlists => tr!("Playlists Yoto"),
            RemoteCollection::Devices => tr!("Lecteurs"),
            RemoteCollection::Groups => tr!("Groupes"),
            RemoteCollection::Icons => tr!("Icônes"),
        }
    }
    pub fn playlist_sync(state: &PlaylistSyncState) -> &'static str {
        match state {
            PlaylistSyncState::LocalOnly => tr!("Sur cet ordinateur"),
            PlaylistSyncState::CloudOnly | PlaylistSyncState::Synced => tr!("Sur Yoto"),
            PlaylistSyncState::Modified => tr!("Modification à confirmer sur Yoto"),
            PlaylistSyncState::Publishing => tr!("Envoi vers Yoto…"),
            PlaylistSyncState::AuthenticationRequired => tr!("Reconnectez votre compte Yoto"),
            PlaylistSyncState::Conflict => tr!("Modifiée sur Yoto · à vérifier"),
            PlaylistSyncState::Uncertain => tr!("Publication à vérifier"),
            PlaylistSyncState::Failed { .. } => tr!("Envoi échoué · voir les détails"),
        }
    }
    pub fn player_command(command: &PlayerCommand) -> &'static str {
        match command {
            PlayerCommand::Volume { .. } => tr!("Régler le volume"),
            PlayerCommand::Ambient { .. } => tr!("Lumière"),
            PlayerCommand::SleepTimer { .. } => tr!("Minuterie"),
            PlayerCommand::Reboot => tr!("Redémarrer"),
            PlayerCommand::Start { .. } => tr!("Lire sur le lecteur"),
            PlayerCommand::Stop => tr!("Arrêter"),
            PlayerCommand::Pause => tr!("Pause"),
            PlayerCommand::Resume => tr!("Reprendre"),
            PlayerCommand::BluetoothOn => tr!("Activer Bluetooth"),
            PlayerCommand::BluetoothOff => tr!("Désactiver Bluetooth"),
            PlayerCommand::BluetoothDeleteBond { .. } => tr!("Oublier Bluetooth"),
            PlayerCommand::BluetoothConnect { .. } => tr!("Connecter Bluetooth"),
            PlayerCommand::BluetoothDisconnect => tr!("Déconnecter Bluetooth"),
            PlayerCommand::BluetoothState => tr!("État Bluetooth"),
            PlayerCommand::DisplayPreview { .. } => tr!("Aperçu de l’icône"),
            PlayerCommand::Events => tr!("Actualiser la lecture"),
            PlayerCommand::Status => tr!("Actualiser l’état"),
        }
    }
}

/// Title of each part of a media too long for one Yoto track, as Yoto will show it;
/// `{title}`, `{part}` and `{parts}` are replaced by the service.
pub fn part_title_template() -> &'static str {
    tr!("{title} — partie {part}/{parts}")
}

/// Title of the track a chapter mark left unnamed becomes, under the chapter named like
/// the media; `{part}` and `{parts}` are replaced by the service.
pub fn mark_title_template() -> &'static str {
    tr!("Partie {part}")
}

/// Under a media too long for one Yoto track: what happens when it joins a playlist.
pub fn split_note(parts: usize) -> String {
    tr!(
        "Trop long pour une seule piste Yoto (1 h au plus) : ajouté à une playlist, il sera découpé en {parts} parties.",
        parts = parts
    )
}

/// Under a media with chapter marks: the chapter it becomes in a playlist, and how
/// many tracks it holds.
pub fn chapters_note(chapters: usize) -> String {
    trn!(
        "Ajouté à une playlist, cet audio deviendra un chapitre de {count} piste.",
        "Ajouté à une playlist, cet audio deviendra un chapitre de {count} pistes, une par chapitre marqué.",
        chapters
    )
}

/// Under a media with chapter marks, when some chapters are too long for one Yoto track.
pub fn chapters_split_note(long: usize) -> String {
    trn!(
        "{count} chapitre est trop long pour une seule piste Yoto : il sera aussi découpé en parties.",
        "{count} chapitres sont trop longs pour une seule piste Yoto : ils seront aussi découpés en parties.",
        long
    )
}

/// Grouped autosave of the playlist editor.
pub mod autosave {
    /// Tag by the title while changes wait for their write or are being written.
    pub fn saving() -> &'static str {
        tr!("Enregistrement…")
    }
    /// Tag by the title once Yoto holds what is shown.
    pub fn saved() -> &'static str {
        tr!("Enregistré sur Yoto")
    }
    /// Tag when the last changes could not be written.
    pub fn unsaved() -> &'static str {
        tr!("Non enregistré")
    }
    /// Tag and banner title when the last write to Yoto failed.
    pub fn send_failed() -> &'static str {
        tr!("Envoi échoué")
    }
    /// Tag when something needs the parent (uncertain write, newer Yoto version).
    pub fn to_check() -> &'static str {
        tr!("À vérifier")
    }
    /// Tags on the tracks while the playlist is being written, and once it is.
    pub fn track_sending() -> &'static str {
        tr!("Envoi sur Yoto…")
    }
    pub fn track_on_yoto() -> &'static str {
        tr!("Sur Yoto")
    }
    /// While media are added on Yoto.
    pub fn writing() -> &'static str {
        tr!("Enregistrement sur Yoto…")
    }
    pub fn writing_and_checking() -> &'static str {
        tr!("Envoi et vérification sur Yoto…")
    }
    /// Under the playlist options.
    pub fn automatic() -> &'static str {
        tr!(
            "Les modifications sont enregistrées automatiquement sur Yoto, dès que vous faites une pause ou quittez un champ."
        )
    }
    /// Why changes wait when a playlist needs the parent first.
    pub fn needs_check() -> &'static str {
        tr!("Cette playlist doit d’abord être vérifiée : ouvrez-la ou consultez « Imports ».")
    }
    /// A write that failed, naming the playlist (the parent may be elsewhere in the app).
    pub fn not_saved(title: &str, reason: &str) -> String {
        tr!(
            "Les modifications de « {title} » n’ont pas pu être enregistrées sur Yoto. {reason}",
            title = title,
            reason = reason
        )
    }
    /// Shown while the app writes the last changes before quitting.
    pub fn quit_saving() -> &'static str {
        tr!("Enregistrement des dernières modifications sur Yoto avant de quitter…")
    }
    /// The question asked when some changes could not be written before quitting.
    pub fn quit_title() -> &'static str {
        tr!("Des modifications ne sont pas enregistrées sur Yoto")
    }
    pub fn quit_anyway() -> &'static str {
        tr!("Quitter quand même")
    }
    /// When Yoto has not confirmed a write in time.
    pub fn quit_timed_out() -> &'static str {
        tr!("Yoto n’a pas encore confirmé l’enregistrement.")
    }
    /// The body of that question: one line per playlist, then what quitting means.
    pub fn quit_description(playlists: &[(String, String)]) -> String {
        let lines = playlists
            .iter()
            .map(|(title, reason)| tr!("« {title} » : {reason}", title = title, reason = reason))
            .collect::<Vec<_>>()
            .join("\n");
        tr!(
            "{lines}\n\nSi vous quittez maintenant, ces modifications seront perdues.",
            lines = lines
        )
    }
}

/// Signing in to Yoto in the browser, and giving up on a sign-in that never comes back.
pub mod connect {
    /// On the connect button while the browser sign-in runs.
    pub fn waiting() -> &'static str {
        tr!("Connexion dans votre navigateur…")
    }
    /// Next to it, to stop waiting.
    pub fn cancel() -> &'static str {
        tr!("Annuler")
    }
    pub fn cancel_hint() -> &'static str {
        tr!("Arrêter d’attendre la connexion dans le navigateur")
    }
    /// Once the sign-in was cancelled; the parent can connect again.
    pub fn cancelled() -> &'static str {
        tr!("Connexion annulée.")
    }
}

/// « Outil YouTube » in « Compte » (advanced mode): the tool imports use and its updates.
pub mod youtube_tool {
    use yoto_infra::youtube_tool::{ToolActivity, ToolOrigin, ToolProblem, YoutubeToolStatus};
    pub fn title() -> &'static str {
        tr!("Outil YouTube")
    }
    pub fn note() -> &'static str {
        tr!(
            "Il sert à importer les vidéos YouTube. L’application le met à jour toute seule une fois par jour, et aussitôt qu’un import échoue parce que YouTube a changé."
        )
    }
    pub fn update() -> &'static str {
        tr!("Mettre à jour l’outil YouTube")
    }
    /// Which version imports use, and where it comes from.
    pub fn version(status: &YoutubeToolStatus) -> String {
        let origin = match status.origin {
            ToolOrigin::Bundled => tr!("fournie avec l’application"),
            ToolOrigin::Downloaded => tr!("mise à jour par l’application"),
        };
        match &status.version {
            Some(version) => tr!(
                "Version {version} ({origin})",
                version = version,
                origin = origin
            ),
            None => tr!("Version inconnue ({origin})", origin = origin),
        }
    }
    /// What the last or current update attempt says, when there is something to say.
    pub fn activity(activity: &ToolActivity) -> Option<&'static str> {
        Some(match activity {
            ToolActivity::Idle => return None,
            ToolActivity::Checking => tr!("Recherche d’une nouvelle version…"),
            ToolActivity::Current => tr!("L’outil YouTube est à jour."),
            ToolActivity::Updated => tr!("L’outil YouTube a été mis à jour."),
            ToolActivity::Failed(ToolProblem::Network) => {
                tr!(
                    "Impossible de vérifier les mises à jour. Vérifiez votre connexion Internet puis réessayez."
                )
            }
            ToolActivity::Failed(ToolProblem::Integrity) => {
                tr!(
                    "La nouvelle version n’a pas passé la vérification de sécurité : elle n’a pas été installée. L’outil actuel reste utilisé."
                )
            }
            ToolActivity::Failed(ToolProblem::Unusable) => {
                tr!(
                    "La nouvelle version ne démarre pas sur cet ordinateur. L’outil actuel reste utilisé."
                )
            }
            ToolActivity::Failed(ToolProblem::Storage) => {
                tr!(
                    "La nouvelle version n’a pas pu être enregistrée sur cet ordinateur. L’outil actuel reste utilisé."
                )
            }
            ToolActivity::Failed(ToolProblem::Unsupported) => {
                tr!(
                    "Cet ordinateur ne permet pas de mettre à jour l’outil YouTube. Il le sera avec la prochaine version de l’application."
                )
            }
        })
    }
}

/// « Autre médiathèque » in « Compte » (advanced mode).
pub mod other_library {
    pub fn title() -> &'static str {
        tr!("Autre médiathèque")
    }
    pub fn note() -> &'static str {
        tr!(
            "Ouvrez une autre médiathèque de cet ordinateur, par exemple une médiathèque importée. Elle s’ouvre dans une nouvelle fenêtre."
        )
    }
    pub fn open() -> &'static str {
        tr!("Ouvrir une autre médiathèque")
    }
    pub fn picker() -> &'static str {
        tr!("Ouvrir une autre médiathèque Yoto")
    }
}

/// « Exporter la médiathèque… » and « Importer une médiathèque… » in « Médiathèque ».
pub mod library_transfer {
    pub fn export_menu() -> &'static str {
        tr!("Exporter la médiathèque…")
    }
    pub fn import_menu() -> &'static str {
        tr!("Importer une médiathèque…")
    }
    pub fn export_title() -> &'static str {
        tr!("Exporter la médiathèque")
    }
    pub fn import_title() -> &'static str {
        tr!("Importer une médiathèque")
    }
    pub fn export_intro() -> &'static str {
        tr!(
            "Enregistrez une copie protégée de toute votre médiathèque (audios, versions préparées et playlists), par exemple sur une clé USB, pour changer d’ordinateur ou en cas de pépin."
        )
    }
    pub fn import_intro() -> &'static str {
        tr!(
            "Retrouvez une médiathèque exportée depuis cette application. Elle s’ouvre à côté de celle-ci, dans une nouvelle fenêtre : rien n’est remplacé."
        )
    }
    /// Step titles.
    pub fn export_step_folder() -> &'static str {
        tr!("1. Choisissez où enregistrer l’export")
    }
    pub fn export_step_key() -> &'static str {
        tr!("2. Notez la clé de récupération")
    }
    pub fn import_step_key() -> &'static str {
        tr!("1. Collez la clé de récupération")
    }
    pub fn import_step_folder() -> &'static str {
        tr!("2. Choisissez l’export")
    }
    pub fn choose_folder() -> &'static str {
        tr!("Choisir un dossier…")
    }
    pub fn choose_other_folder() -> &'static str {
        tr!("Choisir un autre dossier…")
    }
    pub fn choose_export() -> &'static str {
        tr!("Choisir l’export…")
    }
    pub fn choose_other_export() -> &'static str {
        tr!("Choisir un autre export…")
    }
    pub fn no_folder() -> &'static str {
        tr!("Aucun dossier choisi.")
    }
    pub fn folder_picker() -> &'static str {
        tr!("Où enregistrer l’export de la médiathèque ?")
    }
    pub fn export_picker() -> &'static str {
        tr!("Choisissez le dossier de l’export à importer")
    }
    /// Under the generated key.
    pub fn key_reminder() -> &'static str {
        tr!(
            "Cette clé protège l’export : sans elle, personne ne pourra l’ouvrir, pas même vous. Notez-la sur papier ou dans votre gestionnaire de mots de passe, et gardez-la à part de l’export."
        )
    }
    pub fn key_placeholder() -> &'static str {
        tr!("Clé notée lors de l’export")
    }
    pub fn copy() -> &'static str {
        tr!("Copier")
    }
    pub fn copied() -> &'static str {
        tr!("Copiée")
    }
    pub fn export() -> &'static str {
        tr!("Exporter")
    }
    pub fn import() -> &'static str {
        tr!("Importer")
    }
    pub fn close() -> &'static str {
        tr!("Fermer")
    }
    pub fn exporting() -> &'static str {
        tr!("Export en cours… Cela peut prendre quelques minutes pour une grande médiathèque.")
    }
    pub fn importing() -> &'static str {
        tr!("Import en cours : chaque fichier est vérifié…")
    }
    pub fn open_imported() -> &'static str {
        tr!("Ouvrir la médiathèque importée")
    }
    pub fn key_missing() -> &'static str {
        tr!("Collez d’abord la clé de récupération notée lors de l’export.")
    }
    pub fn key_invalid() -> &'static str {
        tr!(
            "Cette clé de récupération est incomplète ou contient une faute de frappe. Recopiez-la exactement comme elle a été notée lors de l’export (8 groupes de 8 lettres et chiffres)."
        )
    }
    pub fn folder_missing() -> &'static str {
        tr!("Choisissez d’abord un dossier.")
    }
    pub fn export_missing() -> &'static str {
        tr!("Choisissez d’abord le dossier de l’export.")
    }
    pub fn not_an_export() -> &'static str {
        tr!(
            "Ce dossier ne contient pas d’export de médiathèque. Choisissez le dossier créé par « Exporter la médiathèque… »."
        )
    }
    pub fn no_library_folder() -> &'static str {
        tr!("L’application ne trouve pas le dossier de ses médiathèques sur cet ordinateur.")
    }
    /// Name of the folder an export creates, and of an imported library's folder.
    pub fn export_folder_name(date: &str) -> String {
        tr!("Médiathèque Yoto {date}", date = date)
    }
    pub fn imported_folder_name(library: &str, date: &str) -> String {
        tr!("{library} importée {date}", library = library, date = date)
    }
    /// After a successful export or import.
    pub fn exported(path: &str) -> String {
        tr!(
            "Médiathèque exportée dans « {path} ». Gardez bien la clé de récupération.",
            path = path
        )
    }
    pub fn imported(path: &str) -> String {
        tr!(
            "Médiathèque importée dans « {path} ». Ouvrez-la pour la retrouver ; reconnectez ensuite votre compte Yoto dans « Compte ».",
            path = path
        )
    }
    /// After a failure, with the reason in plain French.
    pub fn export_failed(reason: &str) -> String {
        tr!("L’export n’a pas abouti. {reason}", reason = reason)
    }
    pub fn import_failed(reason: &str) -> String {
        tr!("L’import n’a pas abouti. {reason}", reason = reason)
    }
}

fn permission_denied() -> &'static str {
    tr!(
        "Yoto n’autorise pas cette action pour ce compte. Reconnectez votre compte pour accorder l’accès."
    )
}
fn ambiguous() -> &'static str {
    tr!(
        "Yoto a peut-être reçu la modification. Vérifiez le résultat dans « Imports » avant de réessayer."
    )
}

/// Plain French for each service and domain message a parent can run into. Keys are the
/// shared constants, so a renamed message breaks the build instead of showing English.
fn known_message(message: &str) -> Option<&'static str> {
    use yoto_domain::messages as m;
    Some(match message {
        m::YOUTUBE_URL_INVALID | m::URL_NO_HOST => {
            tr!("Ce lien n’est pas une adresse valide. Collez un lien YouTube complet (https://…).")
        }
        m::YOUTUBE_HTTPS_ONLY => {
            tr!("Seuls les liens YouTube (youtube.com ou youtu.be) sont acceptés.")
        }
        m::VIDEO_TOO_LONG => {
            tr!("Cette vidéo dure plus de 2 heures ; l’import est limité à 2 heures.")
        }
        m::YOUTUBE_FAILED => {
            tr!(
                "Le téléchargement YouTube a échoué. La vidéo est peut-être privée, supprimée ou indisponible dans votre pays."
            )
        }
        m::YOUTUBE_PRIVATE => {
            tr!(
                "Cette vidéo YouTube est privée ou réservée aux membres d’une chaîne : elle ne peut pas être importée."
            )
        }
        m::YOUTUBE_AGE_RESTRICTED => {
            tr!(
                "Cette vidéo YouTube est réservée aux adultes : YouTube demande une connexion pour la lire, elle ne peut donc pas être importée."
            )
        }
        m::YOUTUBE_GEO_RESTRICTED => {
            tr!("Cette vidéo YouTube n’est pas disponible dans votre pays.")
        }
        m::YOUTUBE_UNAVAILABLE => {
            tr!("Cette vidéo YouTube n’existe plus ou a été retirée. Vérifiez le lien.")
        }
        m::YOUTUBE_OUTDATED => {
            tr!(
                "YouTube a changé et l’application ne sait plus lire cette vidéo, même après avoir cherché une version plus récente de son outil YouTube. Réessayez plus tard."
            )
        }
        m::YOUTUBE_BOT_CHECK => {
            tr!(
                "YouTube bloque pour un moment les téléchargements depuis votre connexion. Réessayez dans quelques minutes."
            )
        }
        m::YOUTUBE_NETWORK => {
            tr!("YouTube ne répond pas. Vérifiez votre connexion Internet puis réessayez.")
        }
        m::YOUTUBE_PLAYLIST_SELECT => {
            tr!("Ce lien est une playlist YouTube : choisissez les vidéos à importer.")
        }
        m::VIDEO_ID_INVALID => tr!("Cette vidéo n’est pas reconnue. Vérifiez le lien."),
        m::AUDIO_FILE_MISSING => {
            tr!("Ce fichier est introuvable. Il a peut-être été déplacé ou supprimé.")
        }
        m::AUDIO_UNDECODABLE | m::NO_AUDIO_STREAM | m::NO_DECODED_AUDIO_STREAM => {
            tr!("Ce fichier n’est pas un fichier audio lisible.")
        }
        m::AUDIO_EMPTY | m::NO_DURATION => tr!("Ce fichier audio est vide."),
        m::ENCODING_FAILED | m::WAVEFORM_FAILED => {
            tr!(
                "La préparation de l’audio a échoué. Réessayez ; si le problème persiste, réimportez le fichier."
            )
        }
        m::PREPARED_TRACK_TOO_LARGE | m::TRACK_EXCEEDS_LIMITS => {
            tr!(
                "Cet extrait dépasse ce que Yoto accepte (1 h ou 100 Mo par piste). Raccourcissez-le."
            )
        }
        m::TRACK_LONGER_THAN_YOTO_ALLOWS => {
            tr!(
                "Une piste de cette playlist dure plus d’une heure, la limite de Yoto. Retirez-la puis ajoutez de nouveau ce média : il sera découpé en plusieurs parties."
            )
        }
        m::PREVIEW_TOO_LONG => {
            tr!(
                "Cet audio est trop long pour être écouté ici. Découpez-le en parties plus courtes."
            )
        }
        m::PLAYLIST_EXCEEDS_LIMITS => {
            tr!("Cette playlist dépasse ce que Yoto accepte (100 pistes, 5 h ou 500 Mo).")
        }
        m::YOTO_TRACK_TOO_LONG => {
            tr!(
                "Yoto refuse les pistes de plus de 60 minutes. Raccourcissez cette piste ou découpez-la en plusieurs pistes."
            )
        }
        m::YOTO_TRACK_TOO_LARGE => {
            tr!(
                "Yoto refuse les pistes de plus de 100 Mo. Raccourcissez cette piste ou découpez-la en plusieurs pistes."
            )
        }
        m::YOTO_PLAYLIST_TOO_LONG => {
            tr!(
                "Yoto refuse les playlists de plus de 5 heures au total. Retirez des pistes ou répartissez-les sur deux playlists."
            )
        }
        m::YOTO_PLAYLIST_TOO_LARGE => {
            tr!(
                "Yoto refuse les playlists de plus de 500 Mo au total. Retirez des pistes ou répartissez-les sur deux playlists."
            )
        }
        m::STREAM_URL_INVALID => {
            tr!(
                "Ce lien de flux n’est pas une adresse valide. Collez une adresse complète qui commence par https://."
            )
        }
        m::STREAM_HTTPS_ONLY => {
            tr!("Seuls les liens de flux qui commencent par https:// sont acceptés.")
        }
        m::MEDIA_IN_PLAYLIST => {
            tr!(
                "Ce média est utilisé dans une playlist. Retirez-le de la playlist avant de le supprimer."
            )
        }
        m::MEDIA_IN_JOB => {
            tr!(
                "Une tâche utilise encore ce média. Attendez qu’elle se termine ou annulez-la dans « Imports »."
            )
        }
        m::MEDIA_AWAITS_RECONCILE => {
            tr!(
                "Un envoi vers Yoto doit d’abord être vérifié dans « Imports » avant de supprimer ce média."
            )
        }
        m::MEDIA_ABSENT | m::MEDIA_NOT_FOUND | m::RECIPE_NOT_FOUND => {
            tr!("Ce média n’existe plus dans votre médiathèque.")
        }
        m::MEDIA_SELECTION_EMPTY => tr!("Choisissez au moins un média."),
        m::RECIPE_NOT_OF_MEDIA => tr!("Cette version n’appartient pas au média choisi."),
        m::PLAYLIST_NOT_FOUND => tr!("Cette playlist n’existe plus."),
        m::TASK_NOT_FOUND => tr!("Cette tâche n’existe plus."),
        m::PUBLICATION_PENDING => {
            tr!(
                "Un envoi précédent de cette playlist vers Yoto attend encore une vérification. Ouvrez « Imports » pour le vérifier, puis enregistrez de nouveau."
            )
        }
        m::PUBLICATION_DURABLE => {
            tr!(
                "Cet envoi a peut-être déjà abouti sur Yoto. Choisissez « Vérifier la publication » plutôt que « Réessayer »."
            )
        }
        m::TASK_NEEDS_RESOLUTION => {
            tr!("Cette tâche ne peut pas être relancée telle quelle. Ouvrez ses détails.")
        }
        m::TASK_COMPLETED => tr!("Cette tâche est déjà terminée."),
        m::TASK_STILL_ACTIVE => tr!("Cette tâche est encore en cours. Annulez-la d’abord."),
        m::RESTART_BUSY => {
            tr!(
                "Terminez les imports, préparations et envois en cours avant d’installer la mise à jour."
            )
        }
        m::RESTART_PENDING | m::RESTARTING => {
            tr!("L’application redémarre pour installer une mise à jour.")
        }
        m::UPDATE_OTHER_INSTANCE => {
            tr!("Fermez l’autre fenêtre de l’application avant d’installer la mise à jour.")
        }
        m::UPDATE_HELPER_FAILED => {
            tr!(
                "La mise à jour n’a pas pu démarrer. Téléchargez la nouvelle version depuis GitHub Releases."
            )
        }
        m::OAUTH_PORT_BUSY => {
            tr!("Une autre tentative de connexion est encore ouverte. Fermez-la puis réessayez.")
        }
        m::BROWSER_FAILED => {
            tr!("Le navigateur n’a pas pu s’ouvrir. Ouvrez-le vous-même puis réessayez.")
        }
        m::AUTHORIZATION_DECLINED => {
            tr!(
                "Vous avez refusé l’accès dans le navigateur. Réessayez et acceptez les autorisations demandées."
            )
        }
        m::YOTO_DENIED => tr!("Yoto refuse cette action pour ce compte."),
        m::YOTO_NOT_FOUND => {
            tr!("Yoto ne trouve plus cet élément. Actualisez depuis Yoto puis réessayez.")
        }
        m::SELECT_PLAYER => tr!("Choisissez un lecteur."),
        m::PLAYER_NAME_EMPTY | m::DEVICE_NAME_EMPTY => tr!("Entrez un nom pour le lecteur."),
        m::GROUP_NAME_LENGTH => tr!("Le nom du groupe doit contenir entre 1 et 100 caractères."),
        m::IMAGE_TOO_LARGE | m::FAMILY_IMAGE_TOO_LARGE => tr!("L’image dépasse 8 Mo."),
        m::IMAGE_DIMENSIONS_EXCEED | m::IMAGE_DIMENSIONS_TOO_LARGE => {
            tr!("L’image est trop grande (8192 pixels maximum).")
        }
        m::PIXEL_ICON_SIZE | m::PIXEL_IMAGE_INVALID => {
            tr!("Cette image ne peut pas servir d’icône.")
        }
        m::ICON_NOT_UPLOADED => tr!("Cette icône n’a pas encore été envoyée à Yoto."),
        m::ICON_ABSENT => tr!("Cette icône n’est plus disponible. Choisissez-en une autre."),
        m::AUDIO_OUTPUT_SILENT => {
            tr!("La sortie audio ne répond pas. Vérifiez le son de votre ordinateur.")
        }
        m::RECOVERY_KEY_FORMAT | m::RECOVERY_KEY_LENGTH => library_transfer::key_invalid(),
        m::UNEXPECTED_REPLY => {
            tr!(
                "L’application a reçu une réponse inattendue. Réessayez ; si le problème continue, redémarrez l’application."
            )
        }
        m::EXPORT_EXISTS => {
            tr!("Un fichier de ce nom existe déjà à cet endroit. Choisissez un autre nom.")
        }
        m::BACKUP_AUTH_FAILED => {
            tr!(
                "Cette clé de récupération ne correspond pas à cet export, ou l’export a été modifié depuis. Vérifiez la clé notée lors de l’export."
            )
        }
        m::BACKUP_UNSUPPORTED | m::BACKUP_PATH_INVALID => {
            tr!(
                "Ce dossier n’est pas un export de médiathèque. Choisissez le dossier créé par « Exporter la médiathèque… »."
            )
        }
        m::BACKUP_NEWER => {
            tr!(
                "Cet export vient d’une version plus récente de l’application. Mettez l’application à jour puis réessayez."
            )
        }
        m::BACKUP_DATABASE_ABSENT | m::BACKUP_INTEGRITY | m::BACKUP_DATABASE_INVALID => {
            tr!("Cet export est incomplet ou endommagé : il ne peut pas être importé.")
        }
        m::BACKUP_DESTINATION_EXISTS | m::RESTORE_DESTINATION_EXISTS => {
            tr!(
                "Un dossier du même nom existe déjà à cet endroit. Réessayez dans un instant ou choisissez un autre dossier."
            )
        }
        m::BACKUP_NEEDS_PARENT | m::RESTORE_NEEDS_PARENT => {
            tr!("Choisissez un dossier sur cet ordinateur, pas la racine d’un disque.")
        }
        m::TITLE_LENGTH => tr!("Le titre doit contenir entre 1 et 140 caractères."),
        m::PLAYLIST_EMPTY => tr!("Ajoutez au moins un média à la playlist."),
        m::PLAYLIST_TRACKS_MAX => tr!("Une playlist Yoto accepte au plus 100 pistes."),
        m::CHAPTER_EMPTY => tr!("Un chapitre doit contenir au moins une piste."),
        m::SHUFFLE_RANGE_INVALID | m::SHUFFLE_INVALID => {
            tr!("La plage de lecture aléatoire est invalide.")
        }
        m::DUPLICATE_CHAPTER_KEY => tr!("Deux chapitres portent la même clé Yoto."),
        m::DUPLICATE_TRACK_KEY => tr!("Deux pistes portent la même clé Yoto."),
        m::YOTO_VERSION_UNKNOWN => tr!(
            "Cet envoi ne dit pas quelle playlist Yoto garder. Ouvrez-la dans « Playlists » pour la recharger depuis Yoto."
        ),
        m::WIRE_KEY_LENGTH => tr!("Une clé de chapitre ou de piste est trop longue pour Yoto."),
        m::TRACK_KEY_COLLISION => {
            tr!(
                "Ces deux pistes portent la même clé Yoto. Déplacez-les dans des chapitres distincts pour conserver leurs références."
            )
        }
        m::UNPARSED_EVENTS => {
            tr!(
                "Cette playlist contient des événements interactifs non reconnus. Vérifiez-les avant de réorganiser les pistes."
            )
        }
        m::REFERENCE_EVENTS => {
            tr!(
                "Retirez d’abord les événements interactifs qui renvoient à une piste (mode avancé) avant de réorganiser la playlist."
            )
        }
        m::SHUFFLE_TARGETS_CHANGED | m::SHUFFLE_CHAPTER_REMOVED => {
            tr!(
                "Ce changement touche une plage de lecture aléatoire. Retirez d’abord cette plage dans les options de la playlist."
            )
        }
        m::SHUFFLE_NOT_CONTIGUOUS => {
            tr!("Les chapitres d’une plage de lecture aléatoire doivent rester côte à côte.")
        }
        m::CHAPTER_ABSENT => tr!("Ce chapitre n’existe plus."),
        m::CHAPTER_BOUNDARY => tr!("Ce chapitre est déjà en bout de liste."),
        m::TRACK_ABSENT => tr!("Cette piste n’existe plus."),
        m::DESTINATION_ABSENT => tr!("Le chapitre de destination n’existe plus."),
        m::TRACK_BOUNDARY => tr!("Cette piste est déjà en bout de chapitre."),
        m::TRIM_RANGE_INVALID => {
            tr!(
                "Le début doit précéder la fin, et les deux doivent rester dans la durée de l’audio."
            )
        }
        m::CHAPTER_MARKS_INVALID => {
            tr!("Les repères de chapitre doivent se suivre et rester dans la durée de l’audio.")
        }
        m::TIME_FORMAT => tr!("Indiquez une heure au format HH:MM, par exemple 07:30."),
        m::COLOUR_FORMAT => tr!("Indiquez une couleur au format #RRGGBB, par exemple #FF8800."),
        m::LIMITS_NONNEGATIVE => {
            tr!("Les limites et les durées doivent être des nombres entiers positifs.")
        }
        m::VOLUME_RANGE => tr!("Le volume doit être compris entre 0 et 100."),
        _ => return None,
    })
}

/// Plain French for an updater failure, with what to do next.
pub fn update_failure_text(error: &yoto_updater::UpdateError) -> String {
    use yoto_updater::UpdateError;
    match error {
        UpdateError::Network(_) => {
            tr!("Le serveur des mises à jour ne répond pas. Vérifiez votre connexion Internet puis réessayez.").into()
        }
        UpdateError::Manifest(_) | UpdateError::Signature | UpdateError::Archive(_) => {
            tr!("La mise à jour reçue n’est pas valide et n’a pas été installée. Réessayez plus tard.").into()
        }
        UpdateError::Install(detail) => tr!("La mise à jour n’a pas pu être installée. Téléchargez la nouvelle version depuis GitHub Releases. Détail : {detail}", detail = detail),
        UpdateError::UnsupportedPlatform => {
            tr!("Les mises à jour automatiques ne sont pas disponibles sur cet ordinateur. Téléchargez la nouvelle version depuis GitHub Releases.").into()
        }
        UpdateError::Cancelled => tr!("Téléchargement annulé.").into(),
    }
}

/// A recovery key as typed or pasted, ready for the service: groups, spaces and case
/// are only there to help a parent copy it by hand.
pub fn normalize_recovery_key(typed: &str) -> String {
    typed
        .chars()
        .filter(|c| !c.is_whitespace() && !matches!(c, '-' | '–' | '—'))
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// A recovery key, once normalized, is 64 characters of 0–9 and a–f; the parent-facing
/// reason when it is not.
pub fn check_recovery_key(key: &str) -> std::result::Result<(), &'static str> {
    if key.is_empty() {
        Err(library_transfer::key_missing())
    } else if key.len() != 64 || !key.chars().all(|c| c.is_ascii_hexdigit()) {
        Err(library_transfer::key_invalid())
    } else {
        Ok(())
    }
}

/// A recovery key shown in eight groups, so it can be written down, read back and
/// wrapped on two lines.
pub fn grouped_recovery_key(key: &str) -> String {
    key.to_ascii_uppercase()
        .as_bytes()
        .chunks(8)
        .map(|group| String::from_utf8_lossy(group).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Outcome of deleting several media at once: some may be kept because they are in use.
pub fn batch_deletion_text(deleted: u64, kept: u64) -> String {
    let deleted_text = trn!(
        "{count} média supprimé de votre médiathèque.",
        "{count} médias supprimés de votre médiathèque.",
        deleted
    );
    if kept == 0 {
        deleted_text
    } else {
        let deleted_text = trn!(
            "{count} média supprimé.",
            "{count} médias supprimés.",
            deleted
        );
        let kept_text = trn!(
            "{count} média conservé, car encore utilisé dans une playlist ou par une tâche en cours.",
            "{count} médias conservés, car encore utilisés dans une playlist ou par une tâche en cours.",
            kept
        );
        // Two whole sentences: each translates on its own.
        format!("{deleted_text} {kept_text}")
    }
}

/// Whole seconds as a parent reads them: « 3 min 05 s », « 1 h 02 min », « 45 s ».
pub fn duration_text(seconds: f64) -> String {
    let total = seconds.max(0.).round() as u64;
    let (hours, minutes, secs) = (total / 3600, (total / 60) % 60, total % 60);
    if hours > 0 {
        tr!(
            "{hours} h {minutes} min",
            hours = hours,
            minutes = format!("{minutes:02}")
        )
    } else if minutes > 0 {
        tr!(
            "{minutes} min {seconds} s",
            minutes = minutes,
            seconds = format!("{secs:02}")
        )
    } else {
        tr!("{seconds} s", seconds = secs)
    }
}

/// Position in a track as « m:ss ».
pub fn position_text(seconds: f64) -> String {
    let total = seconds.max(0.).round() as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

/// Ask before an action that cannot be undone. `confirm` runs only after the user accepts.
pub fn confirm_destructive(
    window: &mut Window,
    cx: &mut App,
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    action_label: impl Into<SharedString>,
    confirm: impl Fn(&mut Window, &mut App) + 'static,
) {
    let title = title.into();
    let description = description.into();
    let action_label = action_label.into();
    let confirm = std::rc::Rc::new(confirm);
    window.open_alert_dialog(cx, move |alert, _, _| {
        let confirm = confirm.clone();
        alert
            .confirm()
            .title(title.clone())
            .description(description.clone())
            .ok_text(action_label.clone())
            .ok_variant(ButtonVariant::Danger)
            .cancel_text(tr!("Annuler"))
            .on_ok(move |_, window, cx| {
                confirm(window, cx);
                true
            })
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[core::prelude::v1::test]
    fn failures_read_as_plain_french() {
        assert_eq!(
            failure_text(&Failure::Validation("Enter a valid YouTube URL".into())),
            "Ce lien n’est pas une adresse valide. Collez un lien YouTube complet (https://…)."
        );
        assert!(failure_text(&Failure::Connection).contains("connexion Internet"));
        assert!(failure_text(&Failure::Authentication).contains("Compte"));
        assert!(
            failure_text(&Failure::RateLimit {
                retry_after_seconds: 30
            })
            .contains("30 secondes")
        );
        assert_eq!(
            failure_text(&Failure::Validation("Une règle encore inconnue.".into())),
            "Une règle encore inconnue."
        );
        let stuck = failure_text(&Failure::Transcode {
            track: "La Belle au bois dormant".into(),
            problem: TranscodeProblem::Stalled,
            percent: Some(40),
        });
        assert!(stuck.contains("« La Belle au bois dormant »"));
        assert!(stuck.contains("40 %"));
        assert!(!stuck.contains("connexion"));
        let unknown = failure_text(&Failure::Storage("disk full".into()));
        assert!(unknown.starts_with("Un problème est survenu"));
        assert!(unknown.contains("disk full"));
    }
    #[core::prelude::v1::test]
    fn every_shared_message_has_wording_in_both_languages() {
        use crate::i18n::{Language, tests::with_language};
        for message in yoto_domain::messages::ALL {
            let french = with_language(Language::French, || known_message(message));
            let english = with_language(Language::English, || known_message(message));
            assert!(french.is_some(), "No French wording for « {message} »");
            assert!(english.is_some(), "No English wording for « {message} »");
            assert_ne!(
                french, english,
                "« {message} » is not translated to English"
            );
        }
    }
    #[core::prelude::v1::test]
    fn part_titles_are_worded_in_the_interface_language() {
        use crate::i18n::{Language, tests::with_language};
        assert_eq!(
            with_language(Language::French, part_title_template),
            yoto_domain::PART_TITLE
        );
        let english = with_language(Language::English, part_title_template);
        assert_eq!(
            yoto_domain::part_title_with(english, "Lullabies", 2, 3),
            "Lullabies — part 2 of 3"
        );
        assert_eq!(
            with_language(Language::French, mark_title_template),
            yoto_domain::MARK_TITLE
        );
        let english = with_language(Language::English, mark_title_template);
        assert_eq!(yoto_domain::mark_title_with(english, 2, 8), "Part 2");
    }
    #[core::prelude::v1::test]
    fn a_publication_of_several_tracks_shows_one_progress() {
        let recipe = RecipeId::new();
        let upload = UploadId::new("upload-1").unwrap();
        let progress = |done, total, percent| {
            Some(SendProgress {
                done,
                total,
                percent,
            })
        };
        // One track reads as before: what Yoto is doing with it.
        assert_eq!(
            phase_text(&JobPhase::Ingesting {
                recipe,
                upload: upload.clone(),
                percent: Some(40),
                progress: progress(0, 1, 64),
            }),
            "Traitement par Yoto (40 %)"
        );
        assert_eq!(
            phase_text(&JobPhase::Uploading {
                recipe,
                upload: None,
                progress: None,
            }),
            "Envoi de l’audio à Yoto"
        );
        // Several tracks: one count and one percentage, whatever each track is doing.
        assert_eq!(
            phase_text(&JobPhase::Uploading {
                recipe,
                upload: None,
                progress: progress(2, 8, 31),
            }),
            "Envoi à Yoto : 2/8 pistes · 31 %"
        );
        assert_eq!(
            phase_text(&JobPhase::Ingesting {
                recipe,
                upload,
                percent: Some(100),
                progress: progress(7, 8, 96),
            }),
            "Envoi à Yoto : 7/8 pistes · 96 %"
        );
    }
    #[core::prelude::v1::test]
    fn failures_and_durations_read_in_english() {
        use crate::i18n::{Language, tests::with_language};
        with_language(Language::English, || {
            assert!(failure_text(&Failure::Connection).contains("internet connection"));
            let stuck = failure_text(&Failure::Transcode {
                track: "Sleeping Beauty".into(),
                problem: TranscodeProblem::Stalled,
                percent: Some(40),
            });
            assert!(stuck.contains("“Sleeping Beauty”"), "{stuck}");
            assert!(stuck.contains("40%"), "{stuck}");
            assert_eq!(duration_text(185.), "3 min 05 s");
            assert_eq!(
                batch_deletion_text(1, 0),
                "1 media item deleted from your library."
            );
            assert!(
                batch_deletion_text(3, 2).starts_with("3 media items deleted. 2 media items kept")
            );
        });
        assert_eq!(
            batch_deletion_text(1, 0),
            "1 média supprimé de votre médiathèque."
        );
        assert_eq!(
            batch_deletion_text(2, 1),
            "2 médias supprimés. 1 média conservé, car encore utilisé dans une playlist ou par une tâche en cours."
        );
    }
    #[core::prelude::v1::test]
    fn recovery_keys_are_checked_before_any_dialog() {
        assert!(check_recovery_key("").is_err());
        assert!(check_recovery_key("abc").is_err());
        assert!(check_recovery_key(&"g".repeat(64)).is_err());
        assert!(check_recovery_key(&"aF09".repeat(16)).is_ok());
    }
    #[core::prelude::v1::test]
    fn a_recovery_key_reads_back_as_it_was_shown() {
        let key = "0123456789abcdef".repeat(4);
        let shown = grouped_recovery_key(&key);
        assert_eq!(shown.split(' ').count(), 8);
        assert!(shown.starts_with("01234567 89ABCDEF "));
        assert_eq!(normalize_recovery_key(&shown), key);
        assert_eq!(
            normalize_recovery_key(&format!("  {} \n", shown.replace(' ', "-"))),
            key
        );
        assert!(check_recovery_key(&normalize_recovery_key(&shown)).is_ok());
        for message in [
            library_transfer::key_missing(),
            library_transfer::key_invalid(),
            library_transfer::key_reminder(),
        ] {
            assert!(!message.to_lowercase().contains("hexad"), "{message}");
        }
    }
    #[core::prelude::v1::test]
    fn durations_are_readable() {
        assert_eq!(duration_text(45.4), "45 s");
        assert_eq!(duration_text(185.), "3 min 05 s");
        assert_eq!(duration_text(3725.), "1 h 02 min");
        assert_eq!(position_text(65.), "1:05");
    }
}
