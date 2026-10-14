---
title: Export and import your library
order: 8
---

# Export and import your library

Your library contains your original audio and prepared versions. It is stored locally in encrypted form. The catalog also keeps cached data and tracks Yoto operations; the playlists themselves belong to your Yoto account. Exporting the library gives you a protected copy you can import on a new installation or another computer.

## Export the library

1. Open `Médiathèque`, then the `⋯` menu (`Plus d’options`) at the top right.
2. Choose `Exporter la médiathèque…`.
3. Choose `Choisir un dossier…` and pick where the export goes, for example a USB drive. The app creates a `Médiathèque Yoto` folder there, with the date in its name.
4. The dialog shows a recovery key made for this export, in eight groups of letters and digits. Choose `Copier` and keep it in your password manager, or write it down on paper. Keep it apart from the export.
5. Choose `Exporter` and wait for the result. The dialog says where the export was saved, or why it failed.

The export includes the catalog and its audio files. Yoto login sessions are not part of it; you must reconnect Yoto after importing.

The recovery key is required to open the export. Nobody can import it without the key, not even you: an export alone is not enough to recover your library. A new key is made for each export.

## Import a library

1. Open `Médiathèque`, then the `⋯` menu.
2. Choose `Importer une médiathèque…`.
3. Paste or type the recovery key noted at export time. Spaces and dashes between groups do not matter, nor does letter case. The key is checked before anything else.
4. Choose `Choisir l’export…` and select the export folder (or the folder that contains it).
5. Choose `Importer`. Every file is checked; this can take a few minutes for a large library.
6. Choose `Ouvrir la médiathèque importée` to open it in a new window, then reconnect your Yoto account in `Compte`.

Importing never replaces anything: the imported library is created in a new folder next to your current library, and the current window stays open. To reopen it later, turn on `Mode avancé` in `Compte` and choose `Ouvrir une autre médiathèque`.

## Export an original

To recover one file, select it in `Médiathèque` and choose `Exporter l’original` under `Plus d’options`. Export produces a playable audio file, with its original extension, in the location you choose; an existing file is never overwritten.

## Clean up unused files

In the `Médiathèque` `⋯` menu, `Libérer l’espace inutilisé` removes files no longer referenced by media, versions, playlists, or active tasks. Files still in use are preserved.

To remove media used by a playlist, remove its tracks first. Export the library before reorganizing a large one.
