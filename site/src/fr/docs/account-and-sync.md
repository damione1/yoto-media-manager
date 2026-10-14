---
title: Compte Yoto et synchronisation
order: 6
---

# Compte Yoto et synchronisation

## Connecter votre compte

1. Ouvrez `Compte` et choisissez `Connecter mon compte Yoto`.
2. Votre navigateur ouvre la page de connexion de Yoto.
3. Connectez-vous et acceptez les accès dont l’application a besoin.
4. Revenez à YotoMediaManager après la confirmation. Le compte connecté devient actif et vos playlists se chargent automatiquement. La barre latérale affiche aussi le compte utilisé.

Vous saisissez votre mot de passe sur le site de Yoto. L’application garde les autorisations de connexion dans le coffre sécurisé de votre système d’exploitation. Vous n’avez pas besoin de créer une clé de développeur pour utiliser l’application configurée.

Avec plusieurs comptes, choisissez `Utiliser ce compte` avant de modifier une playlist. `Ajouter un compte` lance une autre connexion. Les médias distants et les liens avec les playlists appartiennent à chaque compte, et le compte choisi est conservé d’un démarrage à l’autre.

## Retrouver vos playlists

Vos playlists « Make Your Own » se chargent automatiquement depuis Yoto au démarrage, après la connexion et quand vous ouvrez `Playlists`. Cliquez sur un titre pour ouvrir son contenu actuel. `Actualiser depuis Yoto` sur cet écran recharge la liste.

Les playlists, les lecteurs, les groupes et les icônes ont chacun leur état de chargement. Une erreur de chargement des lecteurs laisse visibles les playlists bien récupérées. `Voir le détail` dans `Compte` montre ces états. `Actualiser` sur cette page actualise aussi les lecteurs et les autres collections.

Les références audio d’une playlist restent distantes. Ouvrir une playlist ne télécharge pas automatiquement ses fichiers audio.

## Envoyer les changements

Dans le dialogue `Ajouter des médias`, cochez votre audio et choisissez `Ajouter`. La préparation, l’envoi et la confirmation démarrent aussitôt. L’indicateur d’activité et le message de progression restent visibles pendant l’opération ; les pistes apparaissent après confirmation.

Les changements de playlist et les nouvelles playlists sont envoyés directement à Yoto. Il n’y a pas de bouton de publication séparé. L’application se souvient des médias déjà envoyés et met à jour la même playlist sur Yoto. Une nouvelle version audio peut demander un nouvel envoi.

Importer ou découper de l’audio dans la médiathèque est une opération locale. L’audio est envoyé à Yoto quand vous l’ajoutez à une playlist.

## Reconnaître les versions déjà envoyées

Les choix de version audio affichent leur état pour le compte actif :

| État dans l’application | Signification |
| --- | --- |
| `Local uniquement` | Cette version n’a pas d’envoi confirmé pour ce compte. |
| `Envoi en cours` | Une tâche d’envoi utilise cette version. |
| `Envoyé sur Yoto` | Le lien avec le média Yoto a été enregistré. |
| `À vérifier` | L’application doit confirmer le résultat d’un envoi. |
| `Échec d’envoi` | Consultez la tâche concernée dans `Imports`. |

Un original peut avoir plusieurs versions, chacune avec son propre état. Changer de compte peut donc changer l’état affiché.

## Comprendre un résultat incertain

Yoto peut accepter une mise à jour avant que l’application n’enregistre sa confirmation, par exemple après une coupure réseau ou une erreur locale. La tâche vous demande alors de vérifier le résultat distant.

Dans `Imports`, ouvrez `Vérifier la publication`, puis choisissez `Vérifier auprès de Yoto`. L’application vérifie ce qui a été créé ou modifié avant d’autoriser un autre envoi. Un nouvel envoi est bloqué pendant cette vérification pour éviter les doublons.

## Résoudre un conflit

Si une autre application a modifié la playlist, la mise à jour peut demander de choisir entre votre version et celle de Yoto. Lisez les choix proposés avant de décider. Vous pouvez garder une copie pour préserver votre travail.

## Mode avancé

Le `Mode avancé`, dans `Compte`, affiche des réglages techniques dont la plupart des familles n’ont jamais besoin : lecture interactive, événements de piste, plages de lecture aléatoire et flux dans les playlists ; valeurs Yoto brutes, raccourcis et commandes techniques pour les lecteurs ; liaison manuelle d’une carte après un envoi incertain ; et `Ouvrir une autre médiathèque`, pour ouvrir dans une nouvelle fenêtre une autre médiathèque de cet ordinateur, par exemple une médiathèque importée. Il est désactivé à chaque démarrage de l’application.

## Se déconnecter

Dans `Compte`, ouvrez le bouton `⋯` du compte (`Options du compte`) et choisissez `Déconnecter ce compte`. Ses autorisations et ses données distantes en cache sont supprimées. Votre audio original et votre travail local sont conservés.

Après une nouvelle connexion, vos playlists se chargent de nouveau automatiquement. Se déconnecter ne supprime pas les créations déjà publiées sur Yoto. L’application garde des références minimales aux publications confirmées pour retrouver la même création lors d’une mise à jour ultérieure. Un résultat réellement incertain doit quand même être résolu après la reconnexion.
