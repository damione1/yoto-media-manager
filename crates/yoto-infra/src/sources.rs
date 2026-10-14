use crate::{
    assets::AssetStore,
    media::{Tools, command},
    media_error, storage,
};
use serde::Deserialize;
use std::path::PathBuf;
use yoto_domain::*;
struct Acquired {
    path: PathBuf,
    title: String,
    provenance: Provenance,
    _workspace: Option<tempfile::TempDir>,
}
trait SourceProvider {
    fn acquire(&self, selection: &ImportSelection, store: &AssetStore) -> Result<Acquired>;
}
struct LocalFileSource;
struct YoutubeSource<'a> {
    tools: &'a Tools,
}
impl SourceProvider for LocalFileSource {
    fn acquire(&self, selection: &ImportSelection, _store: &AssetStore) -> Result<Acquired> {
        match selection {
            ImportSelection::LocalFile { path } => {
                if !path.is_file() {
                    return Err(Failure::Validation(messages::AUDIO_FILE_MISSING.into()));
                }
                Ok(Acquired {
                    path: path.clone(),
                    title: path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into(),
                    provenance: Provenance::LocalFile {
                        name: path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into(),
                    },
                    _workspace: None,
                })
            }
            ImportSelection::Youtube { .. } => {
                Err(Failure::Validation("Wrong source provider".into()))
            }
        }
    }
}
fn youtube_url(raw: &str) -> Result<url::Url> {
    let u = url::Url::parse(raw)
        .map_err(|_| Failure::Validation(messages::YOUTUBE_URL_INVALID.into()))?;
    let host = u
        .host_str()
        .ok_or_else(|| Failure::Validation(messages::URL_NO_HOST.into()))?;
    if u.scheme() != "https"
        || !matches!(
            host,
            "youtube.com" | "www.youtube.com" | "m.youtube.com" | "music.youtube.com" | "youtu.be"
        )
        || !u.username().is_empty()
        || u.password().is_some()
    {
        return Err(Failure::Validation(messages::YOUTUBE_HTTPS_ONLY.into()));
    }
    Ok(u)
}
fn setup(tools: &Tools) -> std::process::Command {
    let mut c = command(&tools.youtube);
    c.args([
        "--ignore-config",
        "--no-plugin-dirs",
        "--no-remote-components",
        "--no-warnings",
        "--no-progress",
        "--js-runtimes",
    ])
    .arg(format!("deno:{}", tools.deno.display()));
    c
}
/// Why yt-dlp could not fetch a video, read from its error output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum YoutubeProblem {
    Private,
    AgeRestricted,
    GeoRestricted,
    Unavailable,
    Outdated,
    BotCheck,
    Network,
    Unknown,
}
impl YoutubeProblem {
    /// Recognise the failure from yt-dlp's error output. Order matters: YouTube's bot check
    /// and age gate both say « Sign in to confirm », and its rate limit starts with
    /// « Video unavailable ». A video is only called unavailable when the message says so.
    fn classify(stderr: &str) -> Self {
        let text = stderr.to_lowercase().replace('’', "'");
        let has = |needles: &[&str]| needles.iter().any(|n| text.contains(n));
        if has(&[
            "not a bot",
            "confirm you are not",
            "http error 429",
            "too many requests",
            "content isn't available, try again later",
        ]) {
            Self::BotCheck
        } else if has(&[
            "private video",
            "video is private",
            "members-only",
            "members only",
            "join this channel",
        ]) {
            Self::Private
        } else if has(&[
            "confirm your age",
            "age-restricted",
            "age restricted",
            "inappropriate for some users",
        ]) {
            Self::AgeRestricted
        } else if has(&[
            "in your country",
            "geo restricted",
            "geo-restricted",
            "geo restriction",
            "from your location",
        ]) {
            Self::GeoRestricted
        } else if has(&[
            "video unavailable",
            "has been removed",
            "no longer available",
            "has been terminated",
            "unsupported url",
        ]) {
            Self::Unavailable
        } else if has(&[
            "unable to extract",
            "extraction failed",
            "failed to extract",
            "please update",
            "is outdated",
            "precondition check failed",
            "requested format is not available",
            "no video formats found",
            // YouTube refuses the media itself once the downloader no longer signs its
            // requests the way YouTube expects; the page and metadata still load.
            "http error 403",
        ]) {
            Self::Outdated
        } else if has(&[
            "unable to download",
            "urlopen error",
            "connection reset",
            "connection refused",
            "connection aborted",
            "timed out",
            "name resolution",
            "nodename nor servname",
            "getaddrinfo",
            "network is unreachable",
            "remote end closed",
            "incompleteread",
            "ssl:",
            "http error 5",
        ]) {
            Self::Network
        } else {
            Self::Unknown
        }
    }
    /// How much the problem tells about the real cause. Each attempt uses another YouTube
    /// client, and a later client often fails more vaguely than an earlier one, so the
    /// strongest problem seen is the one reported.
    fn weight(self) -> u8 {
        match self {
            Self::Unknown => 0,
            Self::Network => 1,
            Self::Outdated => 2,
            Self::BotCheck => 3,
            Self::Private | Self::AgeRestricted | Self::GeoRestricted | Self::Unavailable => 4,
        }
    }
    /// Another attempt, possibly with another YouTube client, may succeed.
    fn transient(self) -> bool {
        match self {
            Self::Outdated | Self::BotCheck | Self::Network | Self::Unknown => true,
            Self::Private | Self::AgeRestricted | Self::GeoRestricted | Self::Unavailable => false,
        }
    }
    fn failure(self) -> Failure {
        Failure::Media(
            match self {
                Self::Private => messages::YOUTUBE_PRIVATE,
                Self::AgeRestricted => messages::YOUTUBE_AGE_RESTRICTED,
                Self::GeoRestricted => messages::YOUTUBE_GEO_RESTRICTED,
                Self::Unavailable => messages::YOUTUBE_UNAVAILABLE,
                Self::Outdated => messages::YOUTUBE_OUTDATED,
                Self::BotCheck => messages::YOUTUBE_BOT_CHECK,
                Self::Network => messages::YOUTUBE_NETWORK,
                Self::Unknown => messages::YOUTUBE_FAILED,
            }
            .into(),
        )
    }
}
/// What a yt-dlp run is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// Describe the video or playlist.
    Metadata,
    /// Fetch the audio of a video that was just described successfully.
    Download,
}
/// YouTube player client used by each attempt; `None` keeps yt-dlp's own choice.
const PLAYER_CLIENTS: [Option<&str>; 4] = [None, Some("android"), Some("ios"), Some("tv")];
/// Pause before the second, third and fourth attempts.
const RETRY_DELAYS: [std::time::Duration; 3] = [
    std::time::Duration::from_secs(1),
    std::time::Duration::from_secs(3),
    std::time::Duration::from_secs(8),
];
/// Run yt-dlp with the arguments `prepare` adds, retrying passing failures with a pause
/// and another YouTube player client each time. Returns standard output, capped at `limit`.
fn run_youtube(
    tools: &Tools,
    step: Step,
    limit: usize,
    prepare: impl Fn(&mut std::process::Command) -> Result<()>,
) -> Result<Vec<u8>> {
    let mut problem = YoutubeProblem::Unknown;
    for (attempt, client) in PLAYER_CLIENTS.iter().enumerate() {
        if attempt > 0 {
            crate::process::pause(RETRY_DELAYS[attempt - 1])?;
        }
        let mut command = setup(tools);
        if let Some(client) = client {
            command
                .arg("--extractor-args")
                .arg(format!("youtube:player_client={client}"));
        }
        prepare(&mut command)?;
        let output = crate::process::output(command, limit, std::time::Duration::from_secs(600))?;
        if output.status.success() {
            return Ok(output.stdout);
        }
        let mut found = YoutubeProblem::classify(&String::from_utf8_lossy(&output.stderr));
        if step == Step::Download && found == YoutubeProblem::Unavailable {
            // The video was described a moment ago, so it still exists: a client that now
            // calls it unavailable is refused by a downloader YouTube no longer accepts.
            found = YoutubeProblem::Outdated;
        }
        if found.weight() > problem.weight() {
            problem = found;
        }
        if !problem.transient() {
            break;
        }
    }
    Err(problem.failure())
}
/// Longest title kept from YouTube, so it stays readable on a player and in lists.
pub const YOUTUBE_TITLE_MAX: usize = 100;
/// Words that describe an upload rather than the work itself.
const TITLE_NOISE: &[&str] = &[
    "official",
    "officiel",
    "officielle",
    "video",
    "vidéo",
    "videoclip",
    "audio",
    "lyric",
    "lyrics",
    "paroles",
    "clip",
    "visualizer",
    "visualiser",
    "hd",
    "hq",
    "4k",
    "1080p",
    "720p",
    "topic",
];
/// Words that may accompany the noise words without carrying meaning on their own.
const TITLE_FILLER: &[&str] = &["music", "musical", "musique", "with", "avec", "the", "en"];
/// A bracketed or trailing part such as « Official Music Video », « HD » or « Topic ».
fn title_noise(part: &str) -> bool {
    let lower = part.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| c.is_whitespace() || matches!(c, '-' | '/' | ',' | '&' | '+'))
        .filter(|w| !w.is_empty())
        .collect();
    !words.is_empty()
        && words
            .iter()
            .all(|w| TITLE_NOISE.contains(w) || TITLE_FILLER.contains(w))
        && words.iter().any(|w| TITLE_NOISE.contains(w))
}
/// Remove « (Official Video) », « [HD] », « - Topic », lyric and audio tags from a
/// YouTube title, tidy its spacing and cap its length. Meaningful brackets such as
/// « (Live) » or « (Remix) » are kept. Returns an empty string only for an empty title.
pub fn clean_youtube_title(raw: &str) -> String {
    let mut kept = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(open) = rest.find(['(', '[', '【']) {
        let opener = rest[open..].chars().next().unwrap_or('(');
        let closer = match opener {
            '[' => ']',
            '【' => '】',
            _ => ')',
        };
        let inner_start = open + opener.len_utf8();
        let Some(length) = rest[inner_start..].find(closer) else {
            break;
        };
        let inner_end = inner_start + length;
        kept.push_str(&rest[..open]);
        if !title_noise(&rest[inner_start..inner_end]) {
            kept.push_str(&rest[open..inner_end + closer.len_utf8()]);
        }
        rest = &rest[inner_end + closer.len_utf8()..];
    }
    kept.push_str(rest);
    let mut title = kept.split_whitespace().collect::<Vec<_>>().join(" ");
    const SEPARATORS: [&str; 5] = [" - ", " | ", " – ", " — ", " // "];
    loop {
        title = title
            .trim_end_matches(|c: char| {
                c.is_whitespace() || matches!(c, '-' | '|' | '–' | '—' | ':' | ',')
            })
            .to_owned();
        let cut = SEPARATORS
            .iter()
            .filter_map(|s| title.rfind(s).map(|at| (at, s.len())))
            .max_by_key(|(at, _)| *at);
        match cut {
            Some((at, length)) if at > 0 && title_noise(&title[at + length..]) => {
                title.truncate(at)
            }
            Some(_) | None => break,
        }
    }
    if title.is_empty() {
        title = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    }
    if title.chars().count() > YOUTUBE_TITLE_MAX {
        let mut short: String = title.chars().take(YOUTUBE_TITLE_MAX - 1).collect();
        if let Some(space) = short.rfind(' ').filter(|at| *at > YOUTUBE_TITLE_MAX / 2) {
            short.truncate(space);
        }
        title = format!(
                "{}…",
                short.trim_end_matches(
                    |c: char| c.is_whitespace() || matches!(c, '-' | '|' | ',' | ':')
                )
            );
    }
    title
}
#[derive(Deserialize)]
struct YoutubeInfo {
    #[serde(rename = "_type")]
    kind: Option<String>,
    id: String,
    title: Option<String>,
    duration: Option<f64>,
    webpage_url: Option<String>,
    entries: Option<Vec<YoutubeInfo>>,
}
pub fn inspect_youtube(tools: &Tools, url: &str) -> Result<SourceInspection> {
    let u = youtube_url(url)?;
    let stdout = run_youtube(tools, Step::Metadata, 4_000_000, |c| {
        c.args([
            "--flat-playlist",
            "--playlist-end",
            "100",
            "--dump-single-json",
            "--",
        ])
        .arg(u.as_str());
        Ok(())
    })?;
    let info: YoutubeInfo = serde_json::from_slice(&stdout).map_err(media_error)?;
    inspection(info)
}
fn inspection(info: YoutubeInfo) -> Result<SourceInspection> {
    let kind = if info.entries.is_some()
        || matches!(info.kind.as_deref(), Some("playlist" | "multi_video"))
    {
        SourceShape::Playlist
    } else {
        SourceShape::Video
    };
    let entries = match info.entries {
        Some(v) => v,
        None => vec![info],
    };
    let entries = entries
        .into_iter()
        .map(|e| {
            Ok(SourceEntry {
                id: e.id.clone(),
                title: e
                    .title
                    .map(|t| clean_youtube_title(&t))
                    .filter(|t| !t.is_empty())
                    .unwrap_or(e.id.clone()),
                duration: e.duration.map(MediaTime::from_seconds).transpose()?,
                url: e
                    .webpage_url
                    .unwrap_or_else(|| format!("https://www.youtube.com/watch?v={}", e.id)),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(SourceInspection { kind, entries })
}
impl SourceProvider for YoutubeSource<'_> {
    fn acquire(&self, selection: &ImportSelection, store: &AssetStore) -> Result<Acquired> {
        match selection {
            ImportSelection::Youtube { url, entry } => {
                let u = youtube_url(url)?;
                if u.query_pairs().any(|(k, _)| k == "list") && entry.is_none() {
                    return Err(Failure::Validation(
                        messages::YOUTUBE_PLAYLIST_SELECT.into(),
                    ));
                }
                let chosen = match entry {
                    Some(id) => {
                        if !id
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                        {
                            return Err(Failure::Validation(messages::VIDEO_ID_INVALID.into()));
                        }
                        format!("https://www.youtube.com/watch?v={id}")
                    }
                    None => u.into(),
                };
                let metadata = run_youtube(self.tools, Step::Metadata, 1_000_000, |c| {
                    c.args(["--no-playlist", "--dump-single-json", "--"])
                        .arg(&chosen);
                    Ok(())
                })?;
                let info: YoutubeInfo = serde_json::from_slice(&metadata).map_err(media_error)?;
                if info.duration.unwrap_or(0.0) > 7200.0 {
                    return Err(Failure::Validation(messages::VIDEO_TOO_LONG.into()));
                }
                let workspace = store.workspace()?;
                let output = workspace.path().join("source.audio");
                run_youtube(self.tools, Step::Download, 1_000_000, |c| {
                    // A failed attempt must not leave a finished-looking output behind.
                    match std::fs::remove_file(&output) {
                        Ok(()) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => return Err(storage(e)),
                    }
                    c.args([
                        "--no-playlist",
                        "--format",
                        "bestaudio",
                        "--max-filesize",
                        "1G",
                        "--ffmpeg-location",
                    ])
                    .arg(&self.tools.ffmpeg)
                    .arg("--output")
                    .arg(&output)
                    .arg("--")
                    .arg(&chosen);
                    Ok(())
                })?;
                Ok(Acquired {
                    path: output,
                    title: info
                        .title
                        .map(|t| clean_youtube_title(&t))
                        .filter(|t| !t.is_empty())
                        .unwrap_or_else(|| info.id.clone()),
                    provenance: Provenance::Youtube {
                        url: chosen,
                        video_id: info.id,
                    },
                    _workspace: Some(workspace),
                })
            }
            ImportSelection::LocalFile { .. } => {
                Err(Failure::Validation("Wrong source provider".into()))
            }
        }
    }
}
pub fn import(
    tools: &Tools,
    store: &AssetStore,
    selection: &ImportSelection,
) -> Result<(MediaItem, Asset, MediaRecipe, Asset)> {
    let local = LocalFileSource;
    let youtube = YoutubeSource { tools };
    let provider: &dyn SourceProvider = match selection {
        ImportSelection::LocalFile { .. } => &local,
        ImportSelection::Youtube { .. } => &youtube,
    };
    let acquired = provider.acquire(selection, store)?;
    let info = tools.probe(&acquired.path)?;
    let extension = match selection {
        ImportSelection::LocalFile { path } => path
            .extension()
            .and_then(|s| s.to_str())
            .filter(|s| s.len() < 12 && s.chars().all(|c| c.is_ascii_alphanumeric()))
            .unwrap_or("audio"),
        ImportSelection::Youtube { .. } => "audio",
    };
    let original = store.put(&acquired.path, extension)?;
    let waveform = tools.waveform(&acquired.path, info.duration)?;
    let lease = store.lease("json")?;
    std::fs::write(
        lease.path(),
        serde_json::to_vec(&waveform).map_err(storage)?,
    )
    .map_err(storage)?;
    let waveform = store.put(lease.path(), "json")?;
    let id = MediaId::new();
    let recipe = MediaRecipe {
        id: RecipeId::new(),
        media: id,
        original: original.id,
        trim: TrimRange::new(MediaTime::ZERO, info.duration, info.duration)?,
        normalization: Normalization::Preserve,
        result: None,
        info: None,
        tool_version: tools.validate()?,
        part: None,
    };
    let media = MediaItem {
        id,
        title: acquired.title,
        author: String::new(),
        tags: vec![],
        original: original.id,
        info,
        provenance: acquired.provenance,
        recipes: vec![recipe.id],
        icon: None,
    };
    Ok((media, original, recipe, waveform))
}
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn youtube_download_uses_an_absent_output_and_cleans_parts() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let tool = root.path().join("downloader");
        std::fs::write(
            &tool,
            r#"#!/bin/sh
metadata=0
output=
while [ "$#" -gt 0 ]; do
 case "$1" in --dump-single-json) metadata=1;; --output) shift; output="$1";; esac
 shift
done
if [ "$metadata" = 1 ]; then printf '{"id":"video1","title":"Title","duration":3}'; exit 0; fi
[ ! -e "$output" ] || exit 7
printf 'audio' > "$output"
printf 'partial' > "$output.part"
printf 'resume' > "$output.ytdl"
"#,
        )
        .unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut tools = Tools::at(root.path().to_owned());
        tools.youtube = tool;
        let store = AssetStore::new(root.path().join("store"), [1; 32]).unwrap();
        let acquired = YoutubeSource { tools: &tools }
            .acquire(
                &ImportSelection::Youtube {
                    url: "https://www.youtube.com/watch?v=video1".into(),
                    entry: None,
                },
                &store,
            )
            .unwrap();
        assert_eq!(std::fs::read(&acquired.path).unwrap(), b"audio");
        let workspace = acquired.path.parent().unwrap().to_owned();
        assert!(workspace.join("source.audio.part").exists());
        drop(acquired);
        assert!(!workspace.exists());
    }
}

#[cfg(all(test, unix))]
mod retry_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    /// A stand-in downloader that logs its arguments and fails with `stderr` until
    /// `failures` attempts have been made.
    fn downloader(root: &std::path::Path, failures: u32, stderr: &str) -> Tools {
        let tool = root.join("downloader");
        let log = root.join("calls");
        std::fs::write(
            &tool,
            format!(
                r#"#!/bin/sh
echo "$*" >> '{log}'
count=$(wc -l < '{log}')
if [ "$count" -le {failures} ]; then echo '{stderr}' >&2; exit 1; fi
printf '{{"id":"video1","title":"Song","duration":3}}'
"#,
                log = log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut tools = Tools::at(root.to_owned());
        tools.youtube = tool;
        tools
    }
    fn calls(root: &std::path::Path) -> Vec<String> {
        std::fs::read_to_string(root.join("calls"))
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }
    #[test]
    fn a_passing_network_failure_is_retried_with_another_player_client() {
        let root = tempfile::tempdir().unwrap();
        let tools = downloader(
            root.path(),
            1,
            "ERROR: [youtube] video1: Unable to download webpage: <urlopen error timed out>",
        );
        let inspection = inspect_youtube(&tools, "https://www.youtube.com/watch?v=video1").unwrap();
        assert_eq!(inspection.entries[0].title, "Song");
        let calls = calls(root.path());
        assert_eq!(calls.len(), 2);
        assert!(!calls[0].contains("player_client"));
        assert!(calls[1].contains("youtube:player_client=android"));
    }
    #[test]
    fn a_private_video_fails_at_once_with_its_own_message() {
        let root = tempfile::tempdir().unwrap();
        let tools = downloader(
            root.path(),
            9,
            "ERROR: [youtube] video1: Private video. Sign in if you have been granted access",
        );
        let failure = inspect_youtube(&tools, "https://www.youtube.com/watch?v=video1")
            .unwrap_err()
            .to_string();
        assert!(failure.contains(messages::YOUTUBE_PRIVATE), "{failure}");
        assert_eq!(calls(root.path()).len(), 1);
    }
    /// A stand-in downloader that describes the video, then fails each download attempt
    /// with the matching line of `stderr`.
    fn failing_download(root: &std::path::Path, stderr: &[&str]) -> Tools {
        for (attempt, line) in stderr.iter().enumerate() {
            std::fs::write(root.join(format!("stderr{}", attempt + 1)), line).unwrap();
        }
        let tool = root.join("downloader");
        std::fs::write(
            &tool,
            format!(
                r#"#!/bin/sh
case "$*" in *--dump-single-json*) printf '{{"id":"video1","title":"Song","duration":3}}'; exit 0;; esac
echo "$*" >> '{log}'
count=$(wc -l < '{log}' | tr -d ' ')
cat '{root}/stderr'"$count" >&2
exit 1
"#,
                log = root.join("calls").display(),
                root = root.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut tools = Tools::at(root.to_owned());
        tools.youtube = tool;
        tools
    }
    fn download(tools: &Tools, root: &std::path::Path) -> String {
        let store = AssetStore::new(root.join("store"), [1; 32]).unwrap();
        let failure = YoutubeSource { tools }
            .acquire(
                &ImportSelection::Youtube {
                    url: "https://www.youtube.com/watch?v=video1".into(),
                    entry: None,
                },
                &store,
            )
            .err()
            .unwrap();
        failure.to_string()
    }
    #[test]
    fn an_outdated_downloader_is_reported_as_outdated_after_every_client() {
        let root = tempfile::tempdir().unwrap();
        // Output captured with an outdated downloader: a refused download for the default
        // client, no audio for the others, and a vague last word from the last client.
        let tools = failing_download(
            root.path(),
            &[
                "ERROR: unable to download video data: HTTP Error 403: Forbidden",
                "ERROR: [youtube] video1: Requested format is not available. Use --list-formats for a list of available formats",
                "ERROR: [youtube] video1: Requested format is not available. Use --list-formats for a list of available formats",
                "ERROR: [youtube] video1: This video is not available",
            ],
        );
        let failure = download(&tools, root.path());
        assert!(failure.contains(messages::YOUTUBE_OUTDATED), "{failure}");
        assert_eq!(calls(root.path()).len(), PLAYER_CLIENTS.len());
    }
    #[test]
    fn a_vaguer_later_client_does_not_hide_a_bot_check() {
        let root = tempfile::tempdir().unwrap();
        let tools = failing_download(
            root.path(),
            &[
                "ERROR: [youtube] video1: Sign in to confirm you're not a bot",
                "ERROR: [youtube] video1: Requested format is not available",
                "ERROR: unable to download video data: <urlopen error timed out>",
                "",
            ],
        );
        let failure = download(&tools, root.path());
        assert!(failure.contains(messages::YOUTUBE_BOT_CHECK), "{failure}");
    }
    #[test]
    fn a_video_just_described_is_not_called_removed_while_downloading() {
        let root = tempfile::tempdir().unwrap();
        let tools = failing_download(
            root.path(),
            &[
                "ERROR: unable to download video data: HTTP Error 403: Forbidden",
                "ERROR: [youtube] video1: Video unavailable. This video has been removed by the uploader",
            ],
        );
        let failure = download(&tools, root.path());
        assert!(failure.contains(messages::YOUTUBE_OUTDATED), "{failure}");
        assert_eq!(calls(root.path()).len(), PLAYER_CLIENTS.len());
    }
}
#[cfg(test)]
mod classification_tests {
    use super::YoutubeProblem as P;
    #[test]
    fn yt_dlp_errors_are_recognised() {
        let cases = [
            (
                "ERROR: [youtube] abc: Sign in to confirm you’re not a bot. Use --cookies",
                P::BotCheck,
            ),
            (
                "ERROR: unable to download video data: HTTP Error 429: Too Many Requests",
                P::BotCheck,
            ),
            (
                "ERROR: [youtube] abc: Private video. Sign in if you've been granted access",
                P::Private,
            ),
            (
                "ERROR: [youtube] abc: Join this channel to get access to members-only content",
                P::Private,
            ),
            (
                "ERROR: [youtube] abc: Sign in to confirm your age. This video may be inappropriate for some users.",
                P::AgeRestricted,
            ),
            (
                "ERROR: [youtube] abc: The uploader has not made this video available in your country",
                P::GeoRestricted,
            ),
            (
                "ERROR: [youtube] abc: Video unavailable. This video has been removed by the uploader",
                P::Unavailable,
            ),
            (
                "ERROR: [youtube] abc: Unable to extract uploader id; please report this issue",
                P::Outdated,
            ),
            (
                "WARNING: nsig extraction failed: Some formats may be missing",
                P::Outdated,
            ),
            (
                "ERROR: [youtube] abc: Unable to download API page: <urlopen error [Errno 8] nodename nor servname provided>",
                P::Network,
            ),
            (
                "ERROR: unable to download video data: HTTP Error 503: Service Unavailable",
                P::Network,
            ),
            (
                "ERROR: [youtube] abc: Video unavailable. This content isn't available, try again later.",
                P::BotCheck,
            ),
            (
                "ERROR: [youtube] abc: This video is not available",
                P::Unknown,
            ),
            ("", P::Unknown),
        ];
        for (stderr, expected) in cases {
            assert_eq!(P::classify(stderr), expected, "{stderr}");
        }
    }
    /// Error output captured from an outdated downloader, after the video's description
    /// had loaded: YouTube refused the media to the default client and offered no audio
    /// to the other clients.
    const OUTDATED_DEFAULT_CLIENT: &str =
        "ERROR: unable to download video data: HTTP Error 403: Forbidden";
    const OUTDATED_OTHER_CLIENTS: &str = "ERROR: [youtube] dQw4w9WgXcQ: Requested format is not available. Use --list-formats for a list of available formats";
    #[test]
    fn an_outdated_downloader_is_recognised_from_real_output() {
        assert_eq!(P::classify(OUTDATED_DEFAULT_CLIENT), P::Outdated);
        assert_eq!(P::classify(OUTDATED_OTHER_CLIENTS), P::Outdated);
        assert_ne!(P::classify(OUTDATED_OTHER_CLIENTS), P::Unavailable);
    }
    #[test]
    fn the_strongest_problem_outweighs_vaguer_later_ones() {
        assert!(P::Outdated.weight() > P::Network.weight());
        assert!(P::Network.weight() > P::Unknown.weight());
        assert!(P::BotCheck.weight() > P::Outdated.weight());
        assert!(P::Unavailable.weight() > P::BotCheck.weight());
    }
    #[test]
    fn only_passing_failures_are_retried() {
        assert!(P::Network.transient() && P::BotCheck.transient() && P::Outdated.transient());
        assert!(!P::Private.transient() && !P::AgeRestricted.transient());
        assert!(!P::GeoRestricted.transient() && !P::Unavailable.transient());
    }
}
#[cfg(test)]
mod inspection_tests {
    use super::*;
    #[test]
    fn a_single_video_and_one_entry_playlist_keep_distinct_shapes() {
        let video: YoutubeInfo =
            serde_json::from_value(serde_json::json!({"id":"one","title":"Song","_type":"video"}))
                .unwrap();
        let result = inspection(video).unwrap();
        assert_eq!(result.kind, SourceShape::Video);
        assert_eq!(result.entries.len(), 1);
        let playlist: YoutubeInfo = serde_json::from_value(serde_json::json!({"id":"list","_type":"playlist","entries":[{"id":"one","title":"Song"}]})).unwrap();
        let result = inspection(playlist).unwrap();
        assert_eq!(result.kind, SourceShape::Playlist);
        assert_eq!(result.entries.len(), 1);
        let multi: YoutubeInfo = serde_json::from_value(
            serde_json::json!({"id":"multi","_type":"multi_video","entries":[]}),
        )
        .unwrap();
        assert_eq!(inspection(multi).unwrap().kind, SourceShape::Playlist);
    }
}

#[cfg(test)]
mod title_tests {
    use super::{YOUTUBE_TITLE_MAX, clean_youtube_title as clean};
    #[test]
    fn upload_tags_are_removed() {
        let cases = [
            ("Artist - Song (Official Video)", "Artist - Song"),
            ("Artist - Song [Official Music Video]", "Artist - Song"),
            ("Artist - Song (Official Audio) [HD]", "Artist - Song"),
            ("Song (Lyrics)", "Song"),
            ("Song (Lyric Video)", "Song"),
            ("Song [4K]", "Song"),
            ("Artiste - Chanson (Clip Officiel)", "Artiste - Chanson"),
            ("Artiste - Chanson (Paroles)", "Artiste - Chanson"),
            ("Chanson | Vidéo officielle", "Chanson"),
            ("Artist - Topic", "Artist"),
            ("Song - Official Video HD", "Song"),
            ("  Song   with   spaces  ", "Song with spaces"),
            ("Song【MV】(Official Video)", "Song【MV】"),
        ];
        for (raw, expected) in cases {
            assert_eq!(clean(raw), expected, "{raw}");
        }
    }
    #[test]
    fn meaningful_parts_are_kept() {
        for raw in [
            "Song (Live)",
            "Song (Remix)",
            "Song (Full Version)",
            "Le Petit Prince - Chapitre 1",
            "Audio",
            "(Official Video)",
            "Song (unclosed",
        ] {
            let expected = raw.split_whitespace().collect::<Vec<_>>().join(" ");
            assert_eq!(clean(raw), expected, "{raw}");
        }
        assert_eq!(clean(""), "");
    }
    #[test]
    fn long_titles_are_capped_at_a_word() {
        let raw = "Une très longue histoire ".repeat(10);
        let title = clean(&raw);
        assert!(title.chars().count() <= YOUTUBE_TITLE_MAX);
        let kept = title.strip_suffix('…').unwrap();
        assert!(
            raw.starts_with(kept) && raw[kept.len()..].starts_with(' '),
            "{title}"
        );
        let unbroken = "x".repeat(300);
        assert_eq!(clean(&unbroken).chars().count(), YOUTUBE_TITLE_MAX);
    }
}
