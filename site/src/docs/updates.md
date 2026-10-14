---
title: Keep the app up to date
order: 10
---

# Keeping the app up to date

YotoMediaManager checks for new releases when you open it. When a new version is available, **Mises à jour**, at the right of the status bar at the bottom of the window, shows a dot and lets you download it without leaving the app.

You can also open **Mises à jour** and choose **Rechercher une mise à jour** at any time.

1. Choose **Télécharger la mise à jour**. You can see the download progress and cancel it.
2. Wait for the app to prepare the download. Your current version keeps working until you choose to install.
3. Finish any imports, audio preparation or Yoto uploads, then choose **Installer et redémarrer**.

The app restarts with the new version. Your audio library, playlists and account connections are kept.

If another copy of the app is running, close it before installing. If the app cannot install the update in its current folder, move it to a writable folder or download the new version from [GitHub Releases](https://github.com/damione1/yoto-media-manager/releases).

Earlier versions were installed under the name **Yoto**. On macOS, the update installs **YotoMediaManager** in the same folder and removes the old **Yoto** app. On Windows, the next installer moves the app to its new folder and renames its Start menu shortcut. Your library and sign-in are kept either way.

A build you made yourself shows the version **nightly**. It does not look for updates on its own and is never replaced by one; download a published version from GitHub Releases to switch.

If a download cannot be verified, the app keeps your existing version and shows an error. Choose **Réessayer** to check again.

## The YouTube tool

The tool used for YouTube imports is updated separately, without a new version of the app: the app looks for a newer release once a day and whenever an import fails because YouTube changed. A download is used only after its published checksum has been verified and it starts correctly; otherwise the app keeps the version it already has. `Compte` → `Mode avancé` shows the version in use and lets you update it on demand with `Mettre à jour l’outil YouTube`.
