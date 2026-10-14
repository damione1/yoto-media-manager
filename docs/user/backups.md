---
title: Back up and restore
slug: /backups
sidebar_position: 8
---

# Back up and restore

Your library contains your original audio and prepared versions. It is stored locally in encrypted form. The catalog also keeps cached data and tracks Yoto operations; the playlists themselves belong to your Yoto account. A full backup lets you recover your library on a new installation.

## Create a backup

1. Open `Compte`; the `Sauvegarde` card is on the right.
2. Choose `Générer une clé`. The field hides the key; `Afficher` and `Masquer` let you reveal or hide it.
3. Choose `Copier` and keep it in your password manager, separately from the backup folder.
4. Choose `Créer une sauvegarde`, then select its parent folder.
5. Wait for the result. A new Yoto folder contains the backup.

The backup includes the catalog and its audio files. Yoto login sessions stay in the original profile's vault; you must reconnect Yoto after restoring.

The recovery key is required to open the backup. Keep it safe: an encrypted backup alone is not enough to recover your library.

## Restore

1. Open the `Sauvegarde` card in `Compte`.
2. Enter the backup's recovery key.
3. Choose `Restaurer une sauvegarde`.
4. Select the backup folder, then the parent folder for the restored library.
5. Wait while the files and catalog are checked. The app shows the new folder's path.
6. Choose `Ouvrir la médiathèque restaurée` to open that library in a new window.

Restoring creates a new folder and leaves the original library open. `Ouvrir une autre médiathèque`, under `Options avancées` at the bottom of the `Sauvegarde` card, lets you select an existing folder and return to your original profile. Reconnect your Yoto account in the restored profile.

## Export an original

To recover one file, select it in `Médiathèque` and choose `Exporter l’original` under `Plus d’options`. Export produces a playable audio file, with its original extension, in the location you choose; an existing file is never overwritten.

## Clean up unused files

In the `Médiathèque` `⋯` menu, `Libérer l’espace inutilisé` removes files no longer referenced by media, versions, playlists, or active tasks. Files still in use are preserved.

To remove media used by a playlist, remove its tracks first. Keep a recent backup before reorganizing a large library.
