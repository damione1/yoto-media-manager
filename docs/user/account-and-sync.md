---
title: Yoto account and sync
slug: /account-and-sync
sidebar_position: 6
---

# Yoto account and sync

## Connect your account

1. Open `Compte` and choose `Connecter mon compte Yoto`.
2. Your browser opens the Yoto login page.
3. Sign in there and approve the access the app needs.
4. Return to Yoto Atelier after confirmation. The connected account becomes active and your playlists load automatically. The sidebar also shows the account in use.

You enter your password on Yoto's website. The app keeps login authorizations in your operating system's secure vault. You do not need to create a developer key to use the configured app.

With multiple accounts, choose `Utiliser ce compte` before editing a playlist. `Ajouter un compte` starts another login. Remote media and playlist associations belong to each account, and your account selection is preserved across restarts.

## Find your playlists

Your Make Your Own playlists load automatically from Yoto at startup, after login, and when you open `Playlists`. Click a title to open its current content. `Actualiser depuis Yoto` on that screen reloads the list.

Playlists, players, groups, and icons have separate loading states. A player loading error leaves successfully retrieved playlists visible. `Voir le détail` in `Compte` shows these states. `Actualiser` on that page also refreshes players and the other collections.

Audio references in a playlist stay remote. Opening a playlist does not automatically download its audio files.

## Send changes

In the `Ajouter des médias` dialog, check your audio and choose `Ajouter`. Preparation, upload, and confirmation start immediately. The spinner and progress message stay visible during the operation; tracks appear after confirmation.

Playlist changes and new playlists are sent directly to Yoto. There is no separate publish button. The app remembers media already uploaded and updates the same playlist on Yoto. A new audio version may require another upload.

Importing or trimming audio in the library is a local operation. It is sent to Yoto when you add it to a playlist.

## Recognize versions already uploaded

Audio version choices show their state for the active account:

| App state | Meaning |
| --- | --- |
| `Local uniquement` | This version has no confirmed upload for this account. |
| `Envoi en cours` | An upload task is using this version. |
| `Envoyé sur Yoto` | The association with the Yoto media has been recorded. |
| `À vérifier` | The app needs to confirm an upload's result. |
| `Échec d’envoi` | Check the relevant task in `Imports`. |

An original can have several versions, each with its own state. Changing accounts can therefore change the displayed state.

## Understand an uncertain result

Yoto may accept an update before the app records its confirmation, for example after a network interruption or local error. The task then asks you to verify the remote result.

In `Imports`, open `Vérifier la publication`, then choose `Vérifier auprès de Yoto`. The app checks what was created or changed before allowing another upload. A new upload is blocked during this check to avoid duplicates.

## Resolve a conflict

If another app changed the playlist, the update may require a choice between your version and the Yoto version. Read the proposed choices before deciding. You can keep a copy to preserve your work.

## Disconnect

In `Compte`, open `Options` for the account and choose `Déconnecter ce compte`. Its authorizations and cached remote data are removed. Your original audio and local work are preserved.

After reconnecting, your playlists load automatically again. Disconnecting does not delete creations already published on Yoto. The app keeps minimal references to confirmed publications so it can find the same creation during a later update. A genuinely uncertain result still needs to be resolved after reconnecting.
