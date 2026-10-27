---
title: "Résoudre les erreurs d’import et d’envoi Yoto"
navTitle: Dépannage
description: "Résolvez les erreurs d’import, de connexion à Yoto, de playlists et de restauration, ou signalez un problème dans YotoMediaManager."
order: 9
---

# Résoudre les erreurs d’import et d’envoi Yoto

Retrouvez le symptôme ci-dessous avant de relancer un import, un envoi ou une connexion. Si le problème persiste, utilisez le lien de signalement en fin de page.

## L’application ne s’ouvre pas

Sur macOS, vérifiez que l’application vient de la distribution prévue et qu’elle est installée dans **Applications**. Les versions de développement peuvent attendre leur notarisation ; utilisez la distribution recommandée quand elle est disponible.

Si le message parle d’un outil manquant ou d’une vérification d’intégrité, réinstallez le paquet complet. Déplacer seulement certains de ses fichiers internes peut empêcher le démarrage.

## Un import YouTube échoue

Ouvrez le lien dans votre navigateur et vérifiez qu’il s’agit d’une vidéo ou d’une playlist YouTube accessible publiquement. Collez-le de nouveau dans `Imports` et choisissez `Importer`. Pour une playlist, sélectionnez les vidéos voulues.

Les vidéos privées, supprimées, limitées à certaines régions ou qui demandent une connexion peuvent être inaccessibles à l’application. Le détail de la tâche aide à distinguer un problème d’accès, une limite de taille ou une erreur de traitement.

Quand YouTube change, l’outil que l’application utilise pour lire les vidéos peut cesser de fonctionner jusqu’à sa mise à jour. L’application cherche une version plus récente de cet outil une fois par jour, et aussi dès qu’un import échoue pour cette raison ; elle retente alors l’import une fois. Pour le mettre à jour tout de suite, activez le `Mode avancé` dans `Compte` et choisissez `Mettre à jour l’outil YouTube`. Le même endroit affiche la version utilisée.

## La découpe ne fonctionne pas

Vérifiez que le début précède la fin et que les deux positions restent dans la durée de l’audio. Saisissez les positions en secondes. Attendez la fin de l’import avant de lancer la préparation.

Si l’écoute fonctionne mais que la conversion échoue, ouvrez les `Détails` de la tâche dans `Imports`. Votre original reste intact.

## La connexion à Yoto ne se termine pas

Gardez l’application ouverte pendant la connexion dans le navigateur. Revenez à `Compte` après la confirmation. Si l’opération a expiré, recommencez avec `Connecter mon compte Yoto`. Si le navigateur affiche une page d’erreur et que l’application continue d’attendre, choisissez `Annuler` à côté du bouton de connexion, puis recommencez.

Refuser une autorisation peut rendre certaines fonctions indisponibles. Reconnectez le compte pour revoir les accès demandés.

## Mes playlists Yoto n’apparaissent pas

Vérifiez le compte actif dans `Compte`, puis ouvrez `Playlists`. Les playlists se chargent automatiquement depuis Yoto ; `Actualiser depuis Yoto` permet de réessayer après une erreur. La liste montre les créations « Make Your Own » de ce compte.

Vérifiez l’état `Playlists Yoto` pour une erreur d’autorisation ou de chargement. Les erreurs des lecteurs, des groupes et des icônes sont affichées séparément. Ce catalogue couvre les créations « Make Your Own » ; les cartes commerciales n’y figurent pas dans cette version.

## Un envoi reste incertain

Dans `Imports`, ouvrez `Vérifier la publication` et choisissez `Vérifier auprès de Yoto`. L’application vérifie le résultat de l’envoi avant d’autoriser un nouvel envoi. Si plusieurs créations peuvent correspondre, une résolution manuelle peut être nécessaire.

## Mon lecteur ne répond pas

Vérifiez le lecteur sélectionné, sa connexion Wi-Fi et qu’il est bien en ligne. Actualisez `Lecteurs`, puis essayez une commande simple comme changer le volume. Vérifiez le résultat de la commande ; certaines fonctions dépendent du modèle.

## La suppression d’un média est refusée

Le média ou sa version est encore utilisé par une playlist ou une tâche. Retirez la piste concernée ou attendez la fin de la tâche, puis réessayez.

## Ouvrir une médiathèque importée

Juste après un import, choisissez `Ouvrir la médiathèque importée` dans le dialogue d’import. Une nouvelle fenêtre ouvre cette médiathèque et la fenêtre d’origine reste disponible. Pour la rouvrir plus tard, activez le `Mode avancé` dans `Compte`, choisissez `Ouvrir une autre médiathèque` et sélectionnez son dossier, à côté de votre médiathèque habituelle. Vous devrez reconnecter Yoto dans une médiathèque importée.

## La clé de récupération est refusée

Saisissez la clé exactement comme elle a été affichée lors de l’export : huit groupes de huit lettres et chiffres. Les espaces, les tirets et les majuscules n’ont pas d’importance. Chaque export a sa propre clé : la clé d’un autre export ne fonctionne pas. Sans la bonne clé, l’export ne peut pas être ouvert.

## Signaler un problème

Notez la version de l’application, votre système d’exploitation, l’action effectuée et le texte de l’erreur. Une capture d’écran de la zone concernée peut aider. N’y mettez pas de mot de passe, de clé de récupération ni de contenu privé. Pour un problème de traitement, un court exemple audio qui reproduit le problème est utile.

[Signaler un problème sur GitHub](https://github.com/damione1/yoto-media-manager/issues/new) en précisant ces informations.
