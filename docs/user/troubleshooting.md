---
title: Troubleshooting
slug: /troubleshooting
sidebar_position: 9
---

# Troubleshooting

## The app does not open

On macOS, check that the app came from the expected distribution and is installed in **Applications**. Development builds may be awaiting notarization; use the recommended distribution when available.

If the message mentions a missing tool or integrity check, reinstall the complete package. Moving only some of its internal files can prevent startup.

## A YouTube import fails

Open the link in your browser and check that it is a publicly accessible YouTube video or playlist. Paste it into `Imports` again and choose `Importer`. For a playlist, select the videos you want.

Private, deleted, region-restricted, or login-required videos may be inaccessible to the app. Task details help distinguish access problems, size limits, and processing errors.

## Trimming does not work

Check that the start precedes the end and both positions stay within the audio duration. Enter positions in seconds. Wait for the import to finish before starting preparation.

If the preview works but conversion fails, open the task's `Détails` in `Imports`. Your original remains intact.

## Yoto login does not complete

Keep the app open during browser login. Return to `Compte` after confirmation. If the operation expired, start again with `Connecter mon compte Yoto`.

Declining a permission may make some features unavailable. Reconnect the account to review the requested access.

## My Yoto playlists are missing

Check the active account in `Compte`, then open `Playlists`. Playlists load automatically from Yoto; `Actualiser depuis Yoto` lets you try again after an error. The list shows that account's Make Your Own creations.

Check the `Playlists Yoto` state for an authorization or loading error. Player, group, and icon errors are shown separately. This catalog covers Make Your Own creations; commercial cards are not listed in this version.

## An upload remains uncertain

In `Imports`, open `Vérifier la publication` and choose `Vérifier auprès de Yoto`. The app checks the upload's result before allowing a new upload. If several creations could match, it may require manual resolution.

## My player does not respond

Check the selected player, its Wi-Fi connection, and whether it is online. Refresh `Lecteurs`, then try a simple command such as changing volume. Check the command result; some features depend on the model.

## Media deletion is refused

The media or its version is still used by a playlist or task. Remove the relevant track or wait for the task to finish, then try again.

## Open a restored library

After restoring, choose `Ouvrir la médiathèque restaurée` in the advanced options in `Compte`. A new window opens that library and the original window stays available. To find an existing profile, choose `Ouvrir une autre médiathèque` and select its folder. You must reconnect Yoto in a restored profile.

## Report a problem

Note the app version, your operating system, the action you took, and the error text. A screenshot of the relevant area can help. Avoid including passwords, recovery keys, or private content. A short, reproducible audio example is useful for a processing problem.
