//! Parent-facing French wording for failures and durations.
//!
//! The service and domain crates report failures in developer English (they are shared
//! with the MCP server). Every message the desktop app shows goes through `failure_text`
//! so a non-technical parent reads a plain explanation and what to do next.
use super::*;
use gpui_kit::component::{WindowExt, button::ButtonVariant};

/// The product name, the same in the window, the sidebar and every dialog.
pub const PRODUCT_NAME: &str = "YotoMediaManager";

/// Plain-language text for a failure, in French, with the next step when there is one.
pub fn failure_text(failure: &Failure) -> String {
    let known = |message: &str| known_message(message).map(str::to_owned);
    match failure {
        Failure::Validation(message) => known(message).unwrap_or_else(|| message.clone()),
        Failure::Media(message) => known(message)
            .unwrap_or_else(|| format!("L’audio n’a pas pu être traité. Détail : {message}")),
        Failure::Storage(message) => known(message).unwrap_or_else(|| {
            format!("Un problème est survenu sur cet ordinateur. Détail : {message}")
        }),
        Failure::Tool(message) => format!(
            "Un composant de l’application est manquant ou endommagé. Réinstallez l’application. Détail : {message}"
        ),
        Failure::Connection => {
            "Pas de connexion Internet. Vérifiez votre réseau puis réessayez.".into()
        }
        Failure::Authentication => {
            "Connectez votre compte Yoto dans « Compte » pour continuer.".into()
        }
        Failure::Permission(message) => known(message).unwrap_or_else(|| PERMISSION_DENIED.into()),
        Failure::RateLimit {
            retry_after_seconds,
        } => {
            format!("Yoto a reçu trop de demandes. Réessayez dans {retry_after_seconds} secondes.")
        }
        Failure::Compatibility(message) => known(message).unwrap_or_else(|| {
            format!(
                "Yoto a répondu d’une manière inattendue. Réessayez plus tard. Détail : {message}"
            )
        }),
        Failure::Conflict => {
            "Cette playlist a changé sur Yoto. Rechargez-la avant de continuer.".into()
        }
        Failure::Ambiguous => AMBIGUOUS.into(),
        Failure::Cancelled => "Opération annulée.".into(),
        Failure::Transcode {
            track,
            problem,
            percent,
        } => transcode_text(track, *problem, *percent),
    }
}

/// Why Yoto's conversion of a track did not finish, naming the track.
fn transcode_text(track: &str, problem: TranscodeProblem, percent: Option<u8>) -> String {
    match (problem, percent) {
        (TranscodeProblem::Failed, _) => format!(
            "Yoto n’a pas pu convertir la piste « {track} ». Préparez de nouveau l’audio, puis réessayez."
        ),
        (TranscodeProblem::Stalled, Some(percent)) => format!(
            "La conversion de la piste « {track} » par Yoto est restée bloquée à {percent} %, même après un nouvel envoi. Réessayez plus tard."
        ),
        (TranscodeProblem::Stalled, None) => format!(
            "La conversion de la piste « {track} » par Yoto est restée bloquée, même après un nouvel envoi. Réessayez plus tard."
        ),
        (TranscodeProblem::TimedOut, Some(percent)) => format!(
            "Yoto n’a pas fini de convertir la piste « {track} » dans le temps prévu (arrêt à {percent} %). Réessayez plus tard."
        ),
        (TranscodeProblem::TimedOut, None) => format!(
            "Yoto n’a pas fini de convertir la piste « {track} » dans le temps prévu. Réessayez plus tard."
        ),
    }
}

/// What a running job is doing, with Yoto's conversion progress when it reports one.
pub fn phase_text(phase: &JobPhase) -> String {
    match phase {
        JobPhase::Ingesting {
            percent: Some(percent),
            ..
        } => format!("Traitement par Yoto ({percent} %)"),
        JobPhase::Acquiring
        | JobPhase::Analyzing
        | JobPhase::Encoding
        | JobPhase::Uploading { .. }
        | JobPhase::Ingesting { percent: None, .. }
        | JobPhase::Committing
        | JobPhase::Verifying { .. } => phase.label().into(),
    }
}

/// « Outil YouTube » in « Compte » (advanced mode): the tool imports use and its updates.
pub mod youtube_tool {
    use yoto_infra::youtube_tool::{ToolActivity, ToolOrigin, ToolProblem, YoutubeToolStatus};
    pub const TITLE: &str = "Outil YouTube";
    pub const NOTE: &str = "Il sert à importer les vidéos YouTube. L’application le met à jour toute seule une fois par jour, et aussitôt qu’un import échoue parce que YouTube a changé.";
    pub const UPDATE: &str = "Mettre à jour l’outil YouTube";
    /// Which version imports use, and where it comes from.
    pub fn version(status: &YoutubeToolStatus) -> String {
        let origin = match status.origin {
            ToolOrigin::Bundled => "fournie avec l’application",
            ToolOrigin::Downloaded => "mise à jour par l’application",
        };
        match &status.version {
            Some(version) => format!("Version {version} ({origin})"),
            None => format!("Version inconnue ({origin})"),
        }
    }
    /// What the last or current update attempt says, when there is something to say.
    pub fn activity(activity: &ToolActivity) -> Option<&'static str> {
        Some(match activity {
            ToolActivity::Idle => return None,
            ToolActivity::Checking => "Recherche d’une nouvelle version…",
            ToolActivity::Current => "L’outil YouTube est à jour.",
            ToolActivity::Updated => "L’outil YouTube a été mis à jour.",
            ToolActivity::Failed(ToolProblem::Network) => {
                "Impossible de vérifier les mises à jour. Vérifiez votre connexion Internet puis réessayez."
            }
            ToolActivity::Failed(ToolProblem::Integrity) => {
                "La nouvelle version n’a pas passé la vérification de sécurité : elle n’a pas été installée. L’outil actuel reste utilisé."
            }
            ToolActivity::Failed(ToolProblem::Unusable) => {
                "La nouvelle version ne démarre pas sur cet ordinateur. L’outil actuel reste utilisé."
            }
            ToolActivity::Failed(ToolProblem::Storage) => {
                "La nouvelle version n’a pas pu être enregistrée sur cet ordinateur. L’outil actuel reste utilisé."
            }
            ToolActivity::Failed(ToolProblem::Unsupported) => {
                "Cet ordinateur ne permet pas de mettre à jour l’outil YouTube. Il le sera avec la prochaine version de l’application."
            }
        })
    }
}

const PERMISSION_DENIED: &str = "Yoto n’autorise pas cette action pour ce compte. Reconnectez votre compte pour accorder l’accès.";
const AMBIGUOUS: &str = "Yoto a peut-être reçu la modification. Vérifiez le résultat dans « Imports » avant de réessayer.";

/// Plain French for each service and domain message a parent can run into. Keys are the
/// shared constants, so a renamed message breaks the build instead of showing English.
fn known_message(message: &str) -> Option<&'static str> {
    use yoto_domain::messages as m;
    Some(match message {
        m::YOUTUBE_URL_INVALID | m::URL_NO_HOST => {
            "Ce lien n’est pas une adresse valide. Collez un lien YouTube complet (https://…)."
        }
        m::YOUTUBE_HTTPS_ONLY => "Seuls les liens YouTube (youtube.com ou youtu.be) sont acceptés.",
        m::VIDEO_TOO_LONG => "Cette vidéo dure plus de 2 heures ; l’import est limité à 2 heures.",
        m::YOUTUBE_FAILED => {
            "Le téléchargement YouTube a échoué. La vidéo est peut-être privée, supprimée ou indisponible dans votre pays."
        }
        m::YOUTUBE_PRIVATE => {
            "Cette vidéo YouTube est privée ou réservée aux membres d’une chaîne : elle ne peut pas être importée."
        }
        m::YOUTUBE_AGE_RESTRICTED => {
            "Cette vidéo YouTube est réservée aux adultes : YouTube demande une connexion pour la lire, elle ne peut donc pas être importée."
        }
        m::YOUTUBE_GEO_RESTRICTED => "Cette vidéo YouTube n’est pas disponible dans votre pays.",
        m::YOUTUBE_UNAVAILABLE => {
            "Cette vidéo YouTube n’existe plus ou a été retirée. Vérifiez le lien."
        }
        m::YOUTUBE_OUTDATED => {
            "YouTube a changé et l’application ne sait plus lire cette vidéo, même après avoir cherché une version plus récente de son outil YouTube. Réessayez plus tard."
        }
        m::YOUTUBE_BOT_CHECK => {
            "YouTube bloque pour un moment les téléchargements depuis votre connexion. Réessayez dans quelques minutes."
        }
        m::YOUTUBE_NETWORK => {
            "YouTube ne répond pas. Vérifiez votre connexion Internet puis réessayez."
        }
        m::YOUTUBE_PLAYLIST_SELECT => {
            "Ce lien est une playlist YouTube : choisissez les vidéos à importer."
        }
        m::VIDEO_ID_INVALID => "Cette vidéo n’est pas reconnue. Vérifiez le lien.",
        m::AUDIO_FILE_MISSING => {
            "Ce fichier est introuvable. Il a peut-être été déplacé ou supprimé."
        }
        m::AUDIO_UNDECODABLE | m::NO_AUDIO_STREAM | m::NO_DECODED_AUDIO_STREAM => {
            "Ce fichier n’est pas un fichier audio lisible."
        }
        m::AUDIO_EMPTY | m::NO_DURATION => "Ce fichier audio est vide.",
        m::ENCODING_FAILED | m::WAVEFORM_FAILED => {
            "La préparation de l’audio a échoué. Réessayez ; si le problème persiste, réimportez le fichier."
        }
        m::PREPARED_TRACK_TOO_LARGE | m::TRACK_EXCEEDS_LIMITS => {
            "Cet extrait dépasse ce que Yoto accepte (1 h ou 100 Mo par piste). Raccourcissez-le."
        }
        m::TRACK_LONGER_THAN_YOTO_ALLOWS => {
            "Une piste de cette playlist dure plus d’une heure, la limite de Yoto. Retirez-la puis ajoutez de nouveau ce média : il sera découpé en plusieurs parties."
        }
        m::PREVIEW_TOO_LONG => {
            "Cet audio est trop long pour être écouté ici. Découpez-le en parties plus courtes."
        }
        m::PLAYLIST_EXCEEDS_LIMITS => {
            "Cette playlist dépasse ce que Yoto accepte (100 pistes, 5 h ou 500 Mo)."
        }
        m::YOTO_TRACK_TOO_LONG => {
            "Yoto refuse les pistes de plus de 60 minutes. Raccourcissez cette piste ou découpez-la en plusieurs pistes."
        }
        m::YOTO_TRACK_TOO_LARGE => {
            "Yoto refuse les pistes de plus de 100 Mo. Raccourcissez cette piste ou découpez-la en plusieurs pistes."
        }
        m::YOTO_PLAYLIST_TOO_LONG => {
            "Yoto refuse les playlists de plus de 5 heures au total. Retirez des pistes ou répartissez-les sur deux playlists."
        }
        m::YOTO_PLAYLIST_TOO_LARGE => {
            "Yoto refuse les playlists de plus de 500 Mo au total. Retirez des pistes ou répartissez-les sur deux playlists."
        }
        m::STREAM_URL_INVALID => {
            "Ce lien de flux n’est pas une adresse valide. Collez une adresse complète qui commence par https://."
        }
        m::STREAM_HTTPS_ONLY => {
            "Seuls les liens de flux qui commencent par https:// sont acceptés."
        }
        m::MEDIA_IN_PLAYLIST => {
            "Ce média est utilisé dans une playlist. Retirez-le de la playlist avant de le supprimer."
        }
        m::MEDIA_IN_JOB => {
            "Une tâche utilise encore ce média. Attendez qu’elle se termine ou annulez-la dans « Imports »."
        }
        m::MEDIA_AWAITS_RECONCILE => {
            "Un envoi vers Yoto doit d’abord être vérifié dans « Imports » avant de supprimer ce média."
        }
        m::MEDIA_ABSENT | m::MEDIA_NOT_FOUND | m::RECIPE_NOT_FOUND => {
            "Ce média n’existe plus dans votre médiathèque."
        }
        m::PLAYLIST_NOT_FOUND => "Cette playlist n’existe plus.",
        m::TASK_NOT_FOUND => "Cette tâche n’existe plus.",
        m::PUBLICATION_PENDING => {
            "Un envoi précédent de cette playlist vers Yoto attend encore une vérification. Ouvrez « Imports » pour le vérifier, puis enregistrez de nouveau."
        }
        m::PUBLICATION_DURABLE => {
            "Cet envoi a peut-être déjà abouti sur Yoto. Choisissez « Vérifier la publication » plutôt que « Réessayer »."
        }
        m::TASK_NEEDS_RESOLUTION => {
            "Cette tâche ne peut pas être relancée telle quelle. Ouvrez ses détails."
        }
        m::TASK_COMPLETED => "Cette tâche est déjà terminée.",
        m::RESTART_BUSY => {
            "Terminez les imports, préparations et envois en cours avant d’installer la mise à jour."
        }
        m::RESTART_PENDING | m::RESTARTING => {
            "L’application redémarre pour installer une mise à jour."
        }
        m::UPDATE_OTHER_INSTANCE => {
            "Fermez l’autre fenêtre de l’application avant d’installer la mise à jour."
        }
        m::UPDATE_HELPER_FAILED => {
            "La mise à jour n’a pas pu démarrer. Téléchargez la nouvelle version depuis GitHub Releases."
        }
        m::OAUTH_PORT_BUSY => {
            "Une autre tentative de connexion est encore ouverte. Fermez-la puis réessayez."
        }
        m::BROWSER_FAILED => {
            "Le navigateur n’a pas pu s’ouvrir. Ouvrez-le vous-même puis réessayez."
        }
        m::AUTHORIZATION_DECLINED => {
            "Vous avez refusé l’accès dans le navigateur. Réessayez et acceptez les autorisations demandées."
        }
        m::YOTO_DENIED => "Yoto refuse cette action pour ce compte.",
        m::YOTO_NOT_FOUND => {
            "Yoto ne trouve plus cet élément. Actualisez depuis Yoto puis réessayez."
        }
        m::SELECT_PLAYER => "Choisissez un lecteur.",
        m::PLAYER_NAME_EMPTY | m::DEVICE_NAME_EMPTY => "Entrez un nom pour le lecteur.",
        m::GROUP_NAME_LENGTH => "Le nom du groupe doit contenir entre 1 et 100 caractères.",
        m::IMAGE_TOO_LARGE | m::FAMILY_IMAGE_TOO_LARGE => "L’image dépasse 8 Mo.",
        m::IMAGE_DIMENSIONS_EXCEED | m::IMAGE_DIMENSIONS_TOO_LARGE => {
            "L’image est trop grande (8192 pixels maximum)."
        }
        m::PIXEL_ICON_SIZE | m::PIXEL_IMAGE_INVALID => "Cette image ne peut pas servir d’icône.",
        m::ICON_NOT_UPLOADED => "Cette icône n’a pas encore été envoyée à Yoto.",
        m::AUDIO_OUTPUT_SILENT => {
            "La sortie audio ne répond pas. Vérifiez le son de votre ordinateur."
        }
        m::RECOVERY_KEY_FORMAT | m::RECOVERY_KEY_LENGTH => INVALID_RECOVERY_KEY,
        m::UNEXPECTED_REPLY => {
            "L’application a reçu une réponse inattendue. Réessayez ; si le problème continue, redémarrez l’application."
        }
        m::EXPORT_EXISTS => {
            "Un fichier de ce nom existe déjà à cet endroit. Choisissez un autre nom."
        }
        m::BACKUP_AUTH_FAILED => {
            "La clé de récupération ne correspond pas à cette sauvegarde, ou la sauvegarde a été modifiée."
        }
        m::BACKUP_UNSUPPORTED | m::BACKUP_PATH_INVALID => {
            "Ce dossier n’est pas une sauvegarde valide. Choisissez le dossier de sauvegarde lui-même."
        }
        m::BACKUP_NEWER => {
            "Cette sauvegarde vient d’une version plus récente de l’application. Mettez l’application à jour puis réessayez."
        }
        m::BACKUP_DATABASE_ABSENT | m::BACKUP_INTEGRITY | m::BACKUP_DATABASE_INVALID => {
            "Cette sauvegarde est incomplète ou endommagée ; elle ne peut pas être restaurée."
        }
        m::BACKUP_DESTINATION_EXISTS | m::RESTORE_DESTINATION_EXISTS => {
            "Un dossier du même nom existe déjà à cet endroit. Réessayez dans un instant ou choisissez un autre dossier."
        }
        m::BACKUP_NEEDS_PARENT | m::RESTORE_NEEDS_PARENT => {
            "Choisissez un dossier sur cet ordinateur, pas la racine d’un disque."
        }
        m::TITLE_LENGTH => "Le titre doit contenir entre 1 et 140 caractères.",
        m::PLAYLIST_EMPTY => "Ajoutez au moins un média à la playlist.",
        m::PLAYLIST_TRACKS_MAX => "Une playlist Yoto accepte au plus 100 pistes.",
        m::CHAPTER_EMPTY => "Un chapitre doit contenir au moins une piste.",
        m::SHUFFLE_RANGE_INVALID | m::SHUFFLE_INVALID => {
            "La plage de lecture aléatoire est invalide."
        }
        m::DUPLICATE_CHAPTER_KEY => "Deux chapitres portent la même clé Yoto.",
        m::DUPLICATE_TRACK_KEY => "Deux pistes portent la même clé Yoto.",
        m::TRACK_KEY_COLLISION => {
            "Ces deux pistes portent la même clé Yoto. Déplacez-les dans des chapitres distincts pour conserver leurs références."
        }
        m::UNPARSED_EVENTS => {
            "Cette playlist contient des événements interactifs non reconnus. Vérifiez-les avant de réorganiser les pistes."
        }
        m::REFERENCE_EVENTS => {
            "Retirez d’abord les événements interactifs qui renvoient à une piste (mode avancé) avant de réorganiser la playlist."
        }
        m::SHUFFLE_TARGETS_CHANGED | m::SHUFFLE_CHAPTER_REMOVED => {
            "Ce changement touche une plage de lecture aléatoire. Retirez d’abord cette plage dans les options de la playlist."
        }
        m::SHUFFLE_NOT_CONTIGUOUS => {
            "Les chapitres d’une plage de lecture aléatoire doivent rester côte à côte."
        }
        m::CHAPTER_ABSENT => "Ce chapitre n’existe plus.",
        m::CHAPTER_BOUNDARY => "Ce chapitre est déjà en bout de liste.",
        m::TRACK_ABSENT => "Cette piste n’existe plus.",
        m::DESTINATION_ABSENT => "Le chapitre de destination n’existe plus.",
        m::TRACK_BOUNDARY => "Cette piste est déjà en bout de chapitre.",
        m::TRIM_RANGE_INVALID => {
            "Le début doit précéder la fin, et les deux doivent rester dans la durée de l’audio."
        }
        m::TIME_FORMAT => "Indiquez une heure au format HH:MM, par exemple 07:30.",
        m::COLOUR_FORMAT => "Indiquez une couleur au format #RRGGBB, par exemple #FF8800.",
        m::LIMITS_NONNEGATIVE => {
            "Les limites et les durées doivent être des nombres entiers positifs."
        }
        m::VOLUME_RANGE => "Le volume doit être compris entre 0 et 100.",
        _ => return None,
    })
}

const INVALID_RECOVERY_KEY: &str = "Cette clé de récupération n’est pas valide : elle doit contenir 64 caractères (chiffres et lettres de a à f). Vérifiez-la ou générez-en une nouvelle pour une nouvelle sauvegarde.";

/// Plain French for an updater failure, with what to do next.
pub fn update_failure_text(error: &yoto_updater::UpdateError) -> String {
    use yoto_updater::UpdateError;
    match error {
        UpdateError::Network(_) => {
            "Le serveur des mises à jour ne répond pas. Vérifiez votre connexion Internet puis réessayez.".into()
        }
        UpdateError::Manifest(_) | UpdateError::Signature | UpdateError::Archive(_) => {
            "La mise à jour reçue n’est pas valide et n’a pas été installée. Réessayez plus tard.".into()
        }
        UpdateError::Install(detail) => format!(
            "La mise à jour n’a pas pu être installée. Téléchargez la nouvelle version depuis GitHub Releases. Détail : {detail}"
        ),
        UpdateError::UnsupportedPlatform => {
            "Les mises à jour automatiques ne sont pas disponibles sur cet ordinateur. Téléchargez la nouvelle version depuis GitHub Releases.".into()
        }
        UpdateError::Cancelled => "Téléchargement annulé.".into(),
    }
}

/// Shown when a backup starts without a key and the app creates one.
pub const NEW_RECOVERY_KEY: &str = "Une clé de récupération vient d’être créée. Copiez-la et gardez-la en lieu sûr : sans elle, la sauvegarde ne pourra pas être restaurée.";

/// A recovery key is 64 hexadecimal characters; the parent-facing reason when it is not.
pub fn check_recovery_key(key: &str) -> std::result::Result<(), &'static str> {
    if key.is_empty() {
        Err("Collez la clé de récupération de cette sauvegarde (64 caractères).")
    } else if key.len() != 64 || !key.chars().all(|c| c.is_ascii_hexdigit()) {
        Err(INVALID_RECOVERY_KEY)
    } else {
        Ok(())
    }
}

/// Outcome of deleting several media at once: some may be kept because they are in use.
pub fn batch_deletion_text(deleted: u64, kept: u64) -> String {
    if kept == 0 {
        format!("{deleted} média(s) supprimé(s) de votre médiathèque.")
    } else {
        format!(
            "{deleted} média(s) supprimé(s). {kept} média(s) conservé(s), car encore utilisé(s) dans une playlist ou par une tâche en cours."
        )
    }
}

/// Whole seconds as a parent reads them: « 3 min 05 s », « 1 h 02 min », « 45 s ».
pub fn duration_text(seconds: f64) -> String {
    let total = seconds.max(0.).round() as u64;
    let (hours, minutes, secs) = (total / 3600, (total / 60) % 60, total % 60);
    if hours > 0 {
        format!("{hours} h {minutes:02} min")
    } else if minutes > 0 {
        format!("{minutes} min {secs:02} s")
    } else {
        format!("{secs} s")
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
            .cancel_text("Annuler")
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
            failure_text(&Failure::Validation("Choisissez au moins un média.".into())),
            "Choisissez au moins un média."
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
    fn every_shared_message_has_french_wording() {
        for message in yoto_domain::messages::ALL {
            assert!(
                known_message(message).is_some(),
                "No French wording for « {message} »"
            );
        }
    }
    #[core::prelude::v1::test]
    fn recovery_keys_are_checked_before_any_dialog() {
        assert!(check_recovery_key("").is_err());
        assert!(check_recovery_key("abc").is_err());
        assert!(check_recovery_key(&"g".repeat(64)).is_err());
        assert!(check_recovery_key(&"aF09".repeat(16)).is_ok());
    }
    #[core::prelude::v1::test]
    fn durations_are_readable() {
        assert_eq!(duration_text(45.4), "45 s");
        assert_eq!(duration_text(185.), "3 min 05 s");
        assert_eq!(duration_text(3725.), "1 h 02 min");
        assert_eq!(position_text(65.), "1:05");
    }
}
