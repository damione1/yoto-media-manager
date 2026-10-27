---
title: "Back up and restore your audio library"
navTitle: Export and import your library
description: "Export an encrypted backup of your YotoMediaManager library, save its recovery key and import it on another computer while preserving your audio."
order: 8
---

# Back up and restore your audio library

Keep a copy of your local audio library or move it to another computer. This export is separate from the playlists saved on your [Yoto account](account-and-sync.md).

Your library contains your original audio and prepared versions. It is stored locally in encrypted form. The catalog also keeps cached data and tracks Yoto operations; the playlists themselves belong to your Yoto account. Exporting the library gives you a protected copy you can import on a new installation or another computer.

## Export the library

1. Open `Library`, then the `⋯` menu (`More options`) at the top right.
2. Choose `Export the library…`.
3. Choose `Choose a folder…` and pick where the export goes, for example a USB drive. The app creates a `Yoto library` folder there, with the date in its name.
4. The dialog shows a recovery key made for this export, in eight groups of letters and digits. Choose `Copy` and keep it in your password manager, or write it down on paper. Keep it apart from the export.
5. Choose `Export` and wait for the result. The dialog says where the export was saved, or why it failed.

The export includes the catalog and its audio files. Yoto login sessions are not part of it; you must reconnect Yoto after importing.

The recovery key is required to open the export. Nobody can import it without the key, not even you: an export alone is not enough to recover your library. A new key is made for each export.

## Import a library

1. Open `Library`, then the `⋯` menu.
2. Choose `Import a library…`.
3. Paste or type the recovery key noted at export time. Spaces and dashes between groups do not matter, nor does letter case. The key is checked before anything else.
4. Choose `Choose the export…` and select the export folder (or the folder that contains it).
5. Choose `Import`. Every file is checked; this can take a few minutes for a large library.
6. Choose `Open the imported library` to open it in a new window, then reconnect your Yoto account in `Account`.

Importing never replaces anything: the imported library is created in a new folder next to your current library, and the current window stays open. To reopen it later, turn on `Advanced mode` in `Account` and choose `Open another library`.

## Export an original

To recover one file, select it in `Library` and choose `Export the original` under `More options`. Export produces a playable audio file, with its original extension, in the location you choose; an existing file is never overwritten.

## Clean up unused files

In the `Library` `⋯` menu, `Free up unused space` removes files no longer referenced by media, versions, playlists, or active tasks. Files still in use are preserved.

To remove media used by a playlist, remove its tracks first. Export the library before reorganizing a large one.
