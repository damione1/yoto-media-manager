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
fn bounded_output(command: std::process::Command, limit: usize) -> Result<Vec<u8>> {
    let output = crate::process::output(command, limit, std::time::Duration::from_secs(600))?;
    if !output.status.success() {
        return Err(Failure::Media(messages::YOUTUBE_FAILED.into()));
    }
    Ok(output.stdout)
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
    let mut c = setup(tools);
    c.args([
        "--flat-playlist",
        "--playlist-end",
        "100",
        "--dump-single-json",
        "--",
    ])
    .arg(u.as_str());
    let info: YoutubeInfo =
        serde_json::from_slice(&bounded_output(c, 4_000_000)?).map_err(media_error)?;
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
                title: e.title.unwrap_or(e.id.clone()),
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
                let mut metadata = setup(self.tools);
                metadata
                    .args(["--no-playlist", "--dump-single-json", "--"])
                    .arg(&chosen);
                let info: YoutubeInfo =
                    serde_json::from_slice(&bounded_output(metadata, 1_000_000)?)
                        .map_err(media_error)?;
                if info.duration.unwrap_or(0.0) > 7200.0 {
                    return Err(Failure::Validation(messages::VIDEO_TOO_LONG.into()));
                }
                let workspace = store.workspace()?;
                let output = workspace.path().join("source.audio");
                let mut c = setup(self.tools);
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
                bounded_output(c, 1_000_000)?;
                Ok(Acquired {
                    path: output,
                    title: info.title.unwrap_or_else(|| info.id.clone()),
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
