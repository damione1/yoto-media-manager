//! Failure messages a person can meet through the desktop app or the MCP server.
//!
//! They stay in developer English, shared by every client. The desktop app maps each one to
//! plain wording in its interface language by referring to these constants, so a reworded
//! message is caught by the compiler instead of silently showing developer English.

// Domain rules.
pub const TITLE_LENGTH: &str = "Title must contain 1–140 characters.";
pub const PLAYLIST_EMPTY: &str = "Add at least one chapter.";
pub const PLAYLIST_TRACKS_MAX: &str = "A playlist supports at most 100 tracks.";
pub const CHAPTER_EMPTY: &str = "A chapter needs at least one track.";
pub const SHUFFLE_RANGE_INVALID: &str = "Invalid chapter shuffle range.";
pub const DUPLICATE_CHAPTER_KEY: &str = "Duplicate chapter key";
pub const DUPLICATE_TRACK_KEY: &str = "Duplicate track key";
pub const WIRE_KEY_LENGTH: &str = "Chapter and track keys must contain at most 20 characters.";
pub const UNPARSED_EVENTS: &str =
    "Unparsed interactive events must be reviewed before structural edits.";
pub const REFERENCE_EVENTS: &str =
    "Review/remove reference-bearing interactive events before changing playlist structure.";
pub const SHUFFLE_INVALID: &str = "Invalid shuffle";
pub const SHUFFLE_TARGETS_CHANGED: &str = "The structural edit changes shuffle targets. Remove the shuffle group first or preserve its selected chapters.";
pub const SHUFFLE_CHAPTER_REMOVED: &str =
    "This edit removes a selected shuffle chapter. Remove its shuffle group first.";
pub const SHUFFLE_NOT_CONTIGUOUS: &str = "Selected shuffle chapters must remain contiguous.";
pub const CHAPTER_ABSENT: &str = "Chapter absent";
pub const CHAPTER_BOUNDARY: &str = "Chapter boundary";
pub const TRACK_ABSENT: &str = "Track absent";
pub const DESTINATION_ABSENT: &str = "Destination absent";
pub const TRACK_BOUNDARY: &str = "Track boundary";
pub const TRACK_KEY_COLLISION: &str =
    "These tracks share a Yoto key. Move them to separate chapters to keep their references.";
pub const TRIM_RANGE_INVALID: &str = "Choose 0 ≤ start < end ≤ source duration.";
pub const DEVICE_NAME_EMPTY: &str = "Device name cannot be empty";
pub const TIME_FORMAT: &str = "Time must be HH:MM";
pub const COLOUR_FORMAT: &str = "Colour must be #RRGGBB";
pub const LIMITS_NONNEGATIVE: &str = "Limits and timeouts must be nonnegative integers";
pub const VOLUME_RANGE: &str = "Volume must be 0–100";
// Service, storage, media tools and Yoto.
pub const YOUTUBE_URL_INVALID: &str = "Enter a valid YouTube URL";
pub const URL_NO_HOST: &str = "URL has no host";
pub const YOUTUBE_HTTPS_ONLY: &str = "Only HTTPS YouTube URLs are supported";
pub const VIDEO_TOO_LONG: &str = "Video duration exceeds the 2-hour import limit.";
pub const YOUTUBE_FAILED: &str = "YouTube acquisition failed or timed out. The video may be unavailable or require authorization.";
pub const YOUTUBE_PRIVATE: &str = "This YouTube video is private or restricted to channel members.";
pub const YOUTUBE_AGE_RESTRICTED: &str =
    "This YouTube video is age-restricted and requires signing in.";
pub const YOUTUBE_GEO_RESTRICTED: &str = "This YouTube video is not available in this country.";
pub const YOUTUBE_UNAVAILABLE: &str = "This YouTube video is unavailable or was removed.";
pub const YOUTUBE_OUTDATED: &str =
    "The bundled YouTube downloader cannot read this video; it probably needs an update.";
pub const YOUTUBE_BOT_CHECK: &str = "YouTube asked to confirm that the request is not from a bot.";
pub const YOUTUBE_NETWORK: &str = "YouTube could not be reached.";
pub const YOUTUBE_PLAYLIST_SELECT: &str = "Inspect the playlist and select entries explicitly.";
pub const VIDEO_ID_INVALID: &str = "Invalid video id";
pub const AUDIO_FILE_MISSING: &str = "Select an existing audio file";
pub const AUDIO_UNDECODABLE: &str = "This file cannot be decoded as audio.";
pub const NO_AUDIO_STREAM: &str = "No audio stream";
pub const NO_DECODED_AUDIO_STREAM: &str = "No decoded audio stream";
pub const AUDIO_EMPTY: &str = "Empty audio";
pub const NO_DURATION: &str = "No duration";
pub const ENCODING_FAILED: &str = "Local MP3 encoding failed.";
pub const WAVEFORM_FAILED: &str = "Waveform decoding failed";
pub const PREPARED_TRACK_TOO_LARGE: &str =
    "This prepared track exceeds Yoto’s size or duration limit.";
pub const TRACK_LONGER_THAN_YOTO_ALLOWS: &str = "This track is longer than Yoto allows; add the media to the playlist again to split it into parts.";
pub const PREVIEW_TOO_LONG: &str = "Preview exceeds the temporary audio limit";
pub const TRACK_EXCEEDS_LIMITS: &str = "Track exceeds Yoto limits";
pub const PLAYLIST_EXCEEDS_LIMITS: &str = "Playlist exceeds Yoto limits";
pub const YOTO_TRACK_TOO_LONG: &str = "Yoto rejected a track longer than 60 minutes.";
pub const YOTO_TRACK_TOO_LARGE: &str = "Yoto rejected a track larger than 100 MB.";
pub const YOTO_PLAYLIST_TOO_LONG: &str = "Yoto rejected a playlist longer than 5 hours in total.";
pub const YOTO_PLAYLIST_TOO_LARGE: &str = "Yoto rejected a playlist larger than 500 MB in total.";
pub const STREAM_URL_INVALID: &str = "Invalid stream URL";
pub const STREAM_HTTPS_ONLY: &str = "Streaming requires HTTPS";
pub const MEDIA_IN_PLAYLIST: &str = "A playlist references this media/version";
pub const MEDIA_IN_JOB: &str = "Cancel/resolve the referencing job before removing media";
pub const MEDIA_AWAITS_RECONCILE: &str =
    "Reconcile the durable remote effect before removing referenced media";
pub const MEDIA_ABSENT: &str = "Media/version absent";
pub const MEDIA_NOT_FOUND: &str = "Media not found";
pub const RECIPE_NOT_FOUND: &str = "Recipe not found";
pub const MEDIA_SELECTION_EMPTY: &str = "Select at least one media.";
pub const RECIPE_NOT_OF_MEDIA: &str = "Recipe does not belong to this media";
pub const PLAYLIST_NOT_FOUND: &str = "Playlist not found";
pub const TASK_NOT_FOUND: &str = "Task not found";
pub const PUBLICATION_PENDING: &str =
    "A publication for this account and playlist is pending. Resolve it before publishing again.";
pub const PUBLICATION_DURABLE: &str =
    "This publication has a durable remote effect; reconcile it rather than retrying";
pub const TASK_NEEDS_RESOLUTION: &str = "This task requires a separate resolution";
pub const TASK_COMPLETED: &str = "This task is already completed";
pub const TASK_STILL_ACTIVE: &str = "This task is still in progress";
pub const RESTART_BUSY: &str =
    "Finish imports, audio processing and Yoto uploads before restarting";
pub const RESTART_PENDING: &str = "An application restart is already pending";
pub const RESTARTING: &str = "The application is restarting for an update";
pub const UPDATE_OTHER_INSTANCE: &str =
    "Close the other Yoto instance or MCP owner before installing an update";
pub const UPDATE_HELPER_FAILED: &str = "The update helper could not start";
pub const OAUTH_PORT_BUSY: &str =
    "OAuth callback port 8787 is occupied. Close the other connection attempt and retry.";
pub const BROWSER_FAILED: &str = "Could not open the browser";
pub const AUTHORIZATION_DECLINED: &str = "Authorization was declined";
pub const YOTO_DENIED: &str = "Yoto denied this operation";
pub const YOTO_NOT_FOUND: &str = "Yoto rejected the request (404)";
/// Start of a refusal Yoto did not explain in terms the app recognises; the HTTP status and,
/// after [`YOTO_SAID`], what Yoto answered follow it.
pub const YOTO_REJECTED: &str = "Yoto rejected the request";
pub const YOTO_SAID: &str = " Yoto said: ";
/// A refusal Yoto did not explain in terms the app recognises, keeping what it answered.
pub fn yoto_rejected(status: u16, said: Option<&str>) -> String {
    match said {
        Some(said) => format!("{YOTO_REJECTED} ({status}).{YOTO_SAID}{said}"),
        None => format!("{YOTO_REJECTED} ({status})."),
    }
}
/// The HTTP status and Yoto's answer from a message made by [`yoto_rejected`].
pub fn parse_yoto_rejected(message: &str) -> Option<(u16, Option<&str>)> {
    let rest = message.strip_prefix(YOTO_REJECTED)?.strip_prefix(" (")?;
    let (status, rest) = rest.split_once(')')?;
    let status = status.parse().ok()?;
    let rest = rest.strip_prefix('.')?;
    match rest {
        "" => Some((status, None)),
        rest => Some((status, Some(rest.strip_prefix(YOTO_SAID)?))),
    }
}
pub const SELECT_PLAYER: &str = "Select a player";
pub const PLAYER_NAME_EMPTY: &str = "Enter a player name";
pub const GROUP_NAME_LENGTH: &str = "Group name must contain 1–100 characters";
pub const IMAGE_TOO_LARGE: &str = "Image exceeds 8 MB";
pub const FAMILY_IMAGE_TOO_LARGE: &str = "Family image exceeds 8 MB";
pub const IMAGE_DIMENSIONS_EXCEED: &str = "Image dimensions exceed 8192 pixels";
pub const IMAGE_DIMENSIONS_TOO_LARGE: &str = "Image dimensions too large";
pub const PIXEL_ICON_SIZE: &str = "A pixel icon must be 16×16";
pub const PIXEL_IMAGE_INVALID: &str = "Pixel image invalid";
pub const ICON_NOT_UPLOADED: &str = "Icon has not been uploaded";
pub const AUDIO_OUTPUT_SILENT: &str = "Audio output did not respond";
pub const RECOVERY_KEY_FORMAT: &str =
    "Recovery key must contain 64 hexadecimal characters (256 random bits)";
pub const RECOVERY_KEY_LENGTH: &str = "Recovery key must contain exactly 256 bits";
pub const UNEXPECTED_REPLY: &str = "Unexpected reply for this command";
pub const EXPORT_EXISTS: &str = "Export destination already exists";
pub const BACKUP_AUTH_FAILED: &str =
    "Backup authentication failed: wrong recovery key or altered manifest";
pub const BACKUP_UNSUPPORTED: &str = "Unsupported backup";
pub const BACKUP_NEWER: &str = "Backup requires a newer application";
pub const BACKUP_DATABASE_ABSENT: &str = "Backup database absent";
pub const BACKUP_PATH_INVALID: &str = "Invalid backup path";
pub const BACKUP_INTEGRITY: &str = "Backup file integrity failure";
pub const BACKUP_DATABASE_INVALID: &str = "Encrypted backup database is not valid";
pub const BACKUP_DESTINATION_EXISTS: &str = "Backup destination already exists";
pub const BACKUP_NEEDS_PARENT: &str = "Backup needs a parent directory";
pub const RESTORE_DESTINATION_EXISTS: &str =
    "Restore to a new profile directory; the active catalogue is never overwritten";
pub const RESTORE_NEEDS_PARENT: &str = "Restore needs a parent directory";

/// Every message above, so a client can check it has wording for each one.
pub const ALL: &[&str] = &[
    TITLE_LENGTH,
    PLAYLIST_EMPTY,
    PLAYLIST_TRACKS_MAX,
    CHAPTER_EMPTY,
    SHUFFLE_RANGE_INVALID,
    DUPLICATE_CHAPTER_KEY,
    DUPLICATE_TRACK_KEY,
    WIRE_KEY_LENGTH,
    UNPARSED_EVENTS,
    REFERENCE_EVENTS,
    SHUFFLE_INVALID,
    SHUFFLE_TARGETS_CHANGED,
    SHUFFLE_CHAPTER_REMOVED,
    SHUFFLE_NOT_CONTIGUOUS,
    CHAPTER_ABSENT,
    CHAPTER_BOUNDARY,
    TRACK_ABSENT,
    DESTINATION_ABSENT,
    TRACK_BOUNDARY,
    TRACK_KEY_COLLISION,
    TRIM_RANGE_INVALID,
    DEVICE_NAME_EMPTY,
    TIME_FORMAT,
    COLOUR_FORMAT,
    LIMITS_NONNEGATIVE,
    VOLUME_RANGE,
    YOUTUBE_URL_INVALID,
    URL_NO_HOST,
    YOUTUBE_HTTPS_ONLY,
    VIDEO_TOO_LONG,
    YOUTUBE_FAILED,
    YOUTUBE_PRIVATE,
    YOUTUBE_AGE_RESTRICTED,
    YOUTUBE_GEO_RESTRICTED,
    YOUTUBE_UNAVAILABLE,
    YOUTUBE_OUTDATED,
    YOUTUBE_BOT_CHECK,
    YOUTUBE_NETWORK,
    YOUTUBE_PLAYLIST_SELECT,
    VIDEO_ID_INVALID,
    AUDIO_FILE_MISSING,
    AUDIO_UNDECODABLE,
    NO_AUDIO_STREAM,
    NO_DECODED_AUDIO_STREAM,
    AUDIO_EMPTY,
    NO_DURATION,
    ENCODING_FAILED,
    WAVEFORM_FAILED,
    PREPARED_TRACK_TOO_LARGE,
    TRACK_LONGER_THAN_YOTO_ALLOWS,
    PREVIEW_TOO_LONG,
    TRACK_EXCEEDS_LIMITS,
    PLAYLIST_EXCEEDS_LIMITS,
    YOTO_TRACK_TOO_LONG,
    YOTO_TRACK_TOO_LARGE,
    YOTO_PLAYLIST_TOO_LONG,
    YOTO_PLAYLIST_TOO_LARGE,
    STREAM_URL_INVALID,
    STREAM_HTTPS_ONLY,
    MEDIA_IN_PLAYLIST,
    MEDIA_IN_JOB,
    MEDIA_AWAITS_RECONCILE,
    MEDIA_ABSENT,
    MEDIA_NOT_FOUND,
    RECIPE_NOT_FOUND,
    MEDIA_SELECTION_EMPTY,
    RECIPE_NOT_OF_MEDIA,
    PLAYLIST_NOT_FOUND,
    TASK_NOT_FOUND,
    PUBLICATION_PENDING,
    PUBLICATION_DURABLE,
    TASK_NEEDS_RESOLUTION,
    TASK_COMPLETED,
    TASK_STILL_ACTIVE,
    RESTART_BUSY,
    RESTART_PENDING,
    RESTARTING,
    UPDATE_OTHER_INSTANCE,
    UPDATE_HELPER_FAILED,
    OAUTH_PORT_BUSY,
    BROWSER_FAILED,
    AUTHORIZATION_DECLINED,
    YOTO_DENIED,
    YOTO_NOT_FOUND,
    SELECT_PLAYER,
    PLAYER_NAME_EMPTY,
    GROUP_NAME_LENGTH,
    IMAGE_TOO_LARGE,
    FAMILY_IMAGE_TOO_LARGE,
    IMAGE_DIMENSIONS_EXCEED,
    IMAGE_DIMENSIONS_TOO_LARGE,
    PIXEL_ICON_SIZE,
    PIXEL_IMAGE_INVALID,
    ICON_NOT_UPLOADED,
    AUDIO_OUTPUT_SILENT,
    RECOVERY_KEY_FORMAT,
    RECOVERY_KEY_LENGTH,
    UNEXPECTED_REPLY,
    EXPORT_EXISTS,
    BACKUP_AUTH_FAILED,
    BACKUP_UNSUPPORTED,
    BACKUP_NEWER,
    BACKUP_DATABASE_ABSENT,
    BACKUP_PATH_INVALID,
    BACKUP_INTEGRITY,
    BACKUP_DATABASE_INVALID,
    BACKUP_DESTINATION_EXISTS,
    BACKUP_NEEDS_PARENT,
    RESTORE_DESTINATION_EXISTS,
    RESTORE_NEEDS_PARENT,
];
