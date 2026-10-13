//! Parent-facing French wording for failures and durations.
//!
//! The service and domain crates report failures in developer English (they are shared
//! with the MCP server). Every message the desktop app shows goes through `failure_text`
//! so a non-technical parent reads a plain explanation and what to do next.
use super::*;
use gpui_kit::component::{WindowExt, button::ButtonVariant};

/// Plain-language text for a failure, in French, with the next step when there is one.
pub fn failure_text(failure: &Failure) -> String {
    match failure {
        Failure::Validation(message) => known_message(message)
            .map(str::to_owned)
            .unwrap_or_else(|| message.clone()),
        Failure::Media(message) => known_message(message).map(str::to_owned).unwrap_or_else(|| {
            format!("L’audio n’a pas pu être traité. Détail : {message}")
        }),
        Failure::Storage(message) => known_message(message).map(str::to_owned).unwrap_or_else(|| {
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
        Failure::Permission(message) => known_message(message).map(str::to_owned).unwrap_or_else(|| {
            "Yoto n’autorise pas cette action pour ce compte. Reconnectez votre compte pour accorder l’accès.".into()
        }),
        Failure::RateLimit {
            retry_after_seconds,
        } => format!(
            "Yoto a reçu trop de demandes. Réessayez dans {retry_after_seconds} secondes."
        ),
        Failure::Compatibility(message) => known_message(message).map(str::to_owned).unwrap_or_else(|| {
            format!("Yoto a répondu d’une manière inattendue. Réessayez plus tard. Détail : {message}")
        }),
        Failure::Conflict => {
            "Cette playlist a changé sur Yoto. Rechargez-la avant de continuer.".into()
        }
        Failure::Ambiguous => {
            "Yoto a peut-être reçu la modification. Vérifiez le résultat dans « Imports » avant de réessayer.".into()
        }
        Failure::Cancelled => "Opération annulée.".into(),
    }
}

/// Messages emitted by the service crate that a parent can actually run into.
fn known_message(message: &str) -> Option<&'static str> {
    Some(match message {
        "Enter a valid YouTube URL" | "URL has no host" => {
            "Ce lien n’est pas une adresse valide. Collez un lien YouTube complet (https://…)."
        }
        "Only HTTPS YouTube URLs are supported" => {
            "Seuls les liens YouTube (youtube.com ou youtu.be) sont acceptés."
        }
        "Video duration exceeds the 2-hour import limit." => {
            "Cette vidéo dure plus de 2 heures ; l’import est limité à 2 heures."
        }
        "YouTube acquisition failed or timed out. The video may be unavailable or require authorization." => {
            "Le téléchargement YouTube a échoué. La vidéo est peut-être privée, supprimée ou indisponible dans votre pays."
        }
        "Inspect the playlist and select entries explicitly." => {
            "Ce lien est une playlist YouTube : choisissez les vidéos à importer."
        }
        "Invalid video id" => "Cette vidéo n’est pas reconnue. Vérifiez le lien.",
        "Select an existing audio file" => {
            "Ce fichier est introuvable. Il a peut-être été déplacé ou supprimé."
        }
        "This file cannot be decoded as audio." | "No audio stream" | "No decoded audio stream" => {
            "Ce fichier n’est pas un fichier audio lisible."
        }
        "Empty audio" | "No duration" => "Ce fichier audio est vide.",
        "Local MP3 encoding failed." | "Waveform decoding failed" => {
            "La préparation de l’audio a échoué. Réessayez ; si le problème persiste, réimportez le fichier."
        }
        "This prepared track exceeds Yoto’s size or duration limit."
        | "Track exceeds Yoto limits" => {
            "Cet extrait dépasse ce que Yoto accepte (1 h ou 100 Mo par piste). Raccourcissez-le."
        }
        "Playlist exceeds Yoto limits" => {
            "Cette playlist dépasse ce que Yoto accepte (100 pistes, 5 h ou 500 Mo)."
        }
        "A playlist references this media/version" => {
            "Ce média est utilisé dans une playlist. Retirez-le de la playlist avant de le supprimer."
        }
        "Cancel/resolve the referencing job before removing media" => {
            "Une tâche utilise encore ce média. Attendez qu’elle se termine ou annulez-la dans « Imports »."
        }
        "Reconcile the durable remote effect before removing referenced media" => {
            "Un envoi vers Yoto doit d’abord être vérifié dans « Imports » avant de supprimer ce média."
        }
        "Media/version absent" | "Media not found" | "Recipe not found" => {
            "Ce média n’existe plus dans votre médiathèque."
        }
        "Playlist not found" => "Cette playlist n’existe plus.",
        "Task not found" => "Cette tâche n’existe plus.",
        "Finish imports, audio processing and Yoto uploads before restarting" => {
            "Terminez les imports, préparations et envois en cours avant d’installer la mise à jour."
        }
        "An application restart is already pending"
        | "The application is restarting for an update" => {
            "L’application redémarre pour installer une mise à jour."
        }
        "Close the other Yoto instance or MCP owner before installing an update" => {
            "Fermez l’autre fenêtre de l’application avant d’installer la mise à jour."
        }
        "The update helper could not start" => {
            "La mise à jour n’a pas pu démarrer. Téléchargez la nouvelle version depuis GitHub Releases."
        }
        "OAuth callback port 8787 is occupied. Close the other connection attempt and retry." => {
            "Une autre tentative de connexion est encore ouverte. Fermez-la puis réessayez."
        }
        "Could not open the browser" => {
            "Le navigateur n’a pas pu s’ouvrir. Ouvrez-le vous-même puis réessayez."
        }
        "Authorization was declined" => {
            "Vous avez refusé l’accès dans le navigateur. Réessayez et acceptez les autorisations demandées."
        }
        "Yoto denied this operation" => "Yoto refuse cette action pour ce compte.",
        "Yoto rejected the request (404)" => {
            "Yoto ne trouve plus cet élément. Actualisez depuis Yoto puis réessayez."
        }
        "Select a player" => "Choisissez un lecteur.",
        "Enter a player name" => "Entrez un nom pour le lecteur.",
        "Image exceeds 8 MB" | "Family image exceeds 8 MB" => "L’image dépasse 8 Mo.",
        "Image dimensions exceed 8192 pixels" | "Image dimensions too large" => {
            "L’image est trop grande (8192 pixels maximum)."
        }
        "A pixel icon must be 16×16" | "Pixel image invalid" => {
            "Cette image ne peut pas servir d’icône."
        }
        "This publication has a durable remote effect; reconcile it rather than retrying" => {
            "Cet envoi a peut-être déjà abouti sur Yoto. Choisissez « Vérifier la publication » plutôt que « Réessayer »."
        }
        "This task requires a separate resolution" => {
            "Cette tâche ne peut pas être relancée telle quelle. Ouvrez ses détails."
        }
        "This task is already completed" => "Cette tâche est déjà terminée.",
        "Icon has not been uploaded" => "Cette icône n’a pas encore été envoyée à Yoto.",
        "Audio output did not respond" => {
            "La sortie audio ne répond pas. Vérifiez le son de votre ordinateur."
        }
        _ => return None,
    })
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
        let unknown = failure_text(&Failure::Storage("disk full".into()));
        assert!(unknown.starts_with("Un problème est survenu"));
        assert!(unknown.contains("disk full"));
    }
    #[core::prelude::v1::test]
    fn durations_are_readable() {
        assert_eq!(duration_text(45.4), "45 s");
        assert_eq!(duration_text(185.), "3 min 05 s");
        assert_eq!(duration_text(3725.), "1 h 02 min");
        assert_eq!(position_text(65.), "1:05");
    }
}
