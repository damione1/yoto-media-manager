use crate::{media_error, storage};
use serde::Deserialize;
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use yoto_domain::*;
/// Loudness the prepared audio aims for: −16 LUFS, true peak −1.5 dBTP, range 11 LU.
const LOUDNESS_TARGET: &str = "I=-16:TP=-1.5:LRA=11";
/// What the first loudness pass measured, as FFmpeg's loudnorm filter reports it.
#[derive(Debug, Clone, PartialEq)]
struct LoudnessMeasure {
    integrated: f64,
    true_peak: f64,
    range: f64,
    threshold: f64,
    offset: f64,
}
impl LoudnessMeasure {
    /// Read the JSON summary loudnorm prints last on FFmpeg's error output. Silence measures
    /// as « -inf », which cannot drive a linear correction, so it is refused.
    fn parse(stderr: &str) -> Option<Self> {
        #[derive(Deserialize)]
        struct Summary {
            input_i: String,
            input_tp: String,
            input_lra: String,
            input_thresh: String,
            target_offset: String,
        }
        let start = stderr.rfind('{')?;
        let end = start + stderr[start..].find('}')?;
        let summary: Summary = serde_json::from_str(&stderr[start..=end]).ok()?;
        let number = |text: &str| text.trim().parse::<f64>().ok().filter(|n| n.is_finite());
        Some(Self {
            integrated: number(&summary.input_i)?,
            true_peak: number(&summary.input_tp)?,
            range: number(&summary.input_lra)?,
            threshold: number(&summary.input_thresh)?,
            offset: number(&summary.target_offset)?,
        })
    }
    /// Second-pass filter applying the measured correction linearly.
    fn filter(&self) -> String {
        format!(
            "loudnorm={LOUDNESS_TARGET}:measured_I={:.2}:measured_TP={:.2}:measured_LRA={:.2}:measured_thresh={:.2}:offset={:.2}:linear=true",
            self.integrated, self.true_peak, self.range, self.threshold, self.offset
        )
    }
}
#[derive(Clone)]
pub struct Tools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub youtube: PathBuf,
    pub deno: PathBuf,
}
impl Tools {
    pub fn bundled() -> Result<Self> {
        let exe = std::env::current_exe().map_err(storage)?;
        let parent = exe
            .parent()
            .ok_or_else(|| Failure::Tool("No executable directory".into()))?;
        #[cfg(target_os = "macos")]
        let root = parent.parent().unwrap_or(parent).join("Resources/tools");
        #[cfg(not(target_os = "macos"))]
        let root = parent.join("tools");
        let root = match std::env::var_os("YOTO_DEV_TOOLS") {
            Some(p) if cfg!(debug_assertions) => PathBuf::from(p),
            Some(_) => root,
            None => root,
        };
        verify_manifest(&root, expected_target())?;
        Ok(Self::at(root))
    }
    pub fn at(root: PathBuf) -> Self {
        let suffix = if cfg!(windows) { ".exe" } else { "" };
        Self {
            ffmpeg: root.join(format!("ffmpeg{suffix}")),
            ffprobe: root.join(format!("ffprobe{suffix}")),
            youtube: root.join("youtube").join(if cfg!(windows) {
                "yt-dlp.exe"
            } else {
                "yt-dlp_macos"
            }),
            deno: root.join(format!("deno{suffix}")),
        }
    }
    pub fn validate(&self) -> Result<String> {
        let mut cmd = command(&self.ffmpeg);
        cmd.arg("-version");
        let o = crate::process::output(cmd, 64_000, std::time::Duration::from_secs(10))
            .map_err(|_| Failure::Tool("FFmpeg is absent from the application bundle".into()))?;
        if !o.status.success() {
            return Err(Failure::Tool("FFmpeg failed to start".into()));
        }
        Ok(String::from_utf8_lossy(&o.stdout)
            .lines()
            .next()
            .unwrap_or("FFmpeg")
            .into())
    }
    pub fn probe(&self, path: &Path) -> Result<AudioInfo> {
        #[derive(Deserialize)]
        struct Probe {
            streams: Vec<Stream>,
            format: Format,
        }
        #[derive(Deserialize)]
        struct Stream {
            codec_name: Option<String>,
            sample_rate: Option<String>,
            channels: Option<u16>,
            codec_type: Option<String>,
        }
        #[derive(Deserialize)]
        struct Format {
            duration: Option<String>,
            size: Option<String>,
        }
        let mut cmd = command(&self.ffprobe);
        cmd.args([
            "-v",
            "error",
            "-show_streams",
            "-show_format",
            "-of",
            "json",
        ])
        .arg(path);
        let out = crate::process::output(cmd, 1_000_000, std::time::Duration::from_secs(30))?;
        if !out.status.success() {
            return Err(Failure::Media(messages::AUDIO_UNDECODABLE.into()));
        }
        if out.stdout.len() > 1_000_000 {
            return Err(Failure::Media("Probe metadata too large".into()));
        }
        let p: Probe = serde_json::from_slice(&out.stdout).map_err(media_error)?;
        let stream = p
            .streams
            .into_iter()
            .find(|s| s.codec_type.as_deref() == Some("audio"))
            .ok_or_else(|| Failure::Media(messages::NO_AUDIO_STREAM.into()))?;
        let duration = MediaTime::from_seconds(
            p.format
                .duration
                .ok_or_else(|| Failure::Media(messages::NO_DURATION.into()))?
                .parse()
                .map_err(media_error)?,
        )?;
        if duration == MediaTime::ZERO {
            return Err(Failure::Media(messages::AUDIO_EMPTY.into()));
        }
        Ok(AudioInfo {
            duration,
            bytes: p
                .format
                .size
                .ok_or_else(|| Failure::Media("No size".into()))?
                .parse()
                .map_err(media_error)?,
            codec: stream
                .codec_name
                .ok_or_else(|| Failure::Media("No codec".into()))?,
            sample_rate: stream
                .sample_rate
                .ok_or_else(|| Failure::Media("No sample rate".into()))?
                .parse()
                .map_err(media_error)?,
            channels: stream
                .channels
                .ok_or_else(|| Failure::Media("No channels".into()))?,
        })
    }
    pub fn encode(&self, input: &Path, output: &Path, recipe: &MediaRecipe) -> Result<AudioInfo> {
        let trim = recipe.trim;
        let mut filters = format!(
            "atrim=start={:.6}:end={:.6},asetpts=PTS-STARTPTS",
            trim.start().seconds(),
            trim.end().seconds()
        );
        match recipe.normalization {
            Normalization::Preserve => {}
            Normalization::Loudness => {
                // Linear correction from a measured first pass keeps the track's dynamics;
                // when the measure fails, the single-pass dynamic filter still evens it out.
                let loudnorm = match self.measure_loudness(input, &filters) {
                    Some(measure) => measure.filter(),
                    None => format!("loudnorm={LOUDNESS_TARGET}"),
                };
                filters.push(',');
                filters.push_str(&loudnorm);
            }
        };
        let mut cmd = command(&self.ffmpeg);
        cmd.args(["-nostdin", "-v", "error", "-y", "-i"])
            .arg(input)
            .args([
                "-map",
                "0:a:0",
                "-vn",
                "-af",
                &filters,
                "-codec:a",
                "libmp3lame",
                "-b:a",
                &format!("{}k", Rules::MP3_BITRATE),
                "-ar",
                &Rules::SAMPLE_RATE.to_string(),
                "-ac",
                "2",
                "-map_metadata",
                "-1",
            ])
            .arg(output)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let status =
            crate::process::output(cmd, 16_000, std::time::Duration::from_secs(1800))?.status;
        if !status.success() {
            return Err(Failure::Media(messages::ENCODING_FAILED.into()));
        }
        let info = self.probe(output)?;
        let error = (info.duration.seconds() - trim.duration().seconds()).abs();
        if error > 0.1 {
            return Err(Failure::Media(format!(
                "Encoded duration differs by {error:.3} seconds."
            )));
        }
        if info.bytes > Rules::TRACK_BYTES_MAX || info.duration.micros() > Rules::TRACK_MICROS_MAX {
            return Err(Failure::Validation(
                messages::PREPARED_TRACK_TOO_LARGE.into(),
            ));
        }
        Ok(info)
    }
    /// First loudness pass: measure the selected range without writing any audio.
    fn measure_loudness(&self, input: &Path, range_filters: &str) -> Option<LoudnessMeasure> {
        let mut cmd = command(&self.ffmpeg);
        cmd.args(["-nostdin", "-hide_banner", "-nostats", "-v", "info", "-i"])
            .arg(input)
            .args([
                "-map",
                "0:a:0",
                "-vn",
                "-af",
                &format!("{range_filters},loudnorm={LOUDNESS_TARGET}:print_format=json"),
                "-f",
                "null",
                "-",
            ]);
        let output =
            crate::process::output(cmd, 16_000, std::time::Duration::from_secs(1800)).ok()?;
        if !output.status.success() {
            return None;
        }
        LoudnessMeasure::parse(&String::from_utf8_lossy(&output.stderr))
    }
    /// Decode the selected range locally to a transient, universally playable PCM WAV.
    /// This never creates a recipe or a persistent asset, and accepts the same formats as import.
    pub fn preview(&self, input: &Path, output: &Path, trim: TrimRange) -> Result<AudioInfo> {
        let source = self.probe(input)?;
        TrimRange::new(trim.start(), trim.end(), source.duration)?;
        let size_limit = trim
            .duration()
            .micros()
            .checked_mul(u64::from(Rules::SAMPLE_RATE) * 4)
            .map(|v| v / 1_000_000 + 65_536)
            .filter(|v| *v <= 2_000_000_000)
            .ok_or_else(|| Failure::Validation(messages::PREVIEW_TOO_LONG.into()))?;
        let filter = format!(
            "atrim=start={:.6}:end={:.6},asetpts=PTS-STARTPTS",
            trim.start().seconds(),
            trim.end().seconds()
        );
        let mut cmd = command(&self.ffmpeg);
        cmd.args(["-nostdin", "-v", "error", "-y", "-i"])
            .arg(input)
            .args([
                "-map",
                "0:a:0",
                "-vn",
                "-af",
                &filter,
                "-codec:a",
                "pcm_s16le",
                "-ar",
                &Rules::SAMPLE_RATE.to_string(),
                "-ac",
                "2",
                "-map_metadata",
                "-1",
                "-fs",
                &size_limit.to_string(),
                "-f",
                "wav",
            ])
            .arg(output);
        let result = crate::process::output(cmd, 16_000, std::time::Duration::from_secs(120))?;
        if !result.status.success() {
            return Err(Failure::Media(
                "Local preview decoding failed or exceeded its time limit".into(),
            ));
        }
        let info = self.probe(output)?;
        if info.bytes > size_limit
            || (info.duration.seconds() - trim.duration().seconds()).abs() > 0.002
        {
            return Err(Failure::Media(
                "Preview duration disagrees with the selected range".into(),
            ));
        }
        Ok(info)
    }
    pub fn waveform(&self, input: &Path, duration: MediaTime) -> Result<Waveform> {
        let mut cmd = command(&self.ffmpeg);
        cmd.args(["-nostdin", "-v", "error", "-i"])
            .arg(input)
            .args([
                "-map", "0:a:0", "-vn", "-ac", "1", "-ar", "8000", "-f", "f32le", "pipe:1",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = crate::process::spawn(cmd, std::time::Duration::from_secs(1800))?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| Failure::Media(messages::NO_DECODED_AUDIO_STREAM.into()))?;
        let total = (duration.seconds() * 8000.0).ceil() as usize;
        let bucket = total.div_ceil(Rules::WAVEFORM_BUCKETS).max(1);
        let mut peaks = vec![];
        let mut lo = 1.0f32;
        let mut hi = -1.0f32;
        let mut n = 0;
        let mut bytes = [0u8; 4];
        loop {
            match stdout.read_exact(&mut bytes) {
                Ok(()) => {
                    let v = f32::from_le_bytes(bytes);
                    if !v.is_finite() {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(Failure::Media("Invalid audio sample".into()));
                    }
                    lo = lo.min(v);
                    hi = hi.max(v);
                    n += 1;
                    if n == bucket {
                        peaks.push((lo, hi));
                        lo = 1.0;
                        hi = -1.0;
                        n = 0;
                        if peaks.len() > Rules::WAVEFORM_BUCKETS + 2 {
                            let _ = child.kill();
                            let _ = child.wait();
                            return Err(Failure::Media(
                                "Audio duration disagrees with probe".into(),
                            ));
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(media_error(e));
                }
            }
        }
        if n > 0 {
            peaks.push((lo, hi));
        }
        if !child.wait().map_err(media_error)?.success() {
            return Err(Failure::Media(messages::WAVEFORM_FAILED.into()));
        }
        let mut levels = vec![peaks];
        while levels.last().map_or(0, Vec::len) > 64 {
            let next = levels
                .last()
                .expect("one level")
                .chunks(2)
                .map(|p| {
                    p.iter()
                        .fold((1.0f32, -1.0f32), |(l, h), (a, b)| (l.min(*a), h.max(*b)))
                })
                .collect();
            levels.push(next);
        }
        Ok(Waveform { levels })
    }
}
pub fn command(path: &Path) -> Command {
    let c = Command::new(path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut c = c;
        c.creation_flags(0x08000000);
        c
    }
    #[cfg(not(windows))]
    c
}
pub fn prepare_icon(input: &Path, output: &Path) -> Result<()> {
    let image = read_image(input)?;
    if image.width() > 8192 || image.height() > 8192 {
        return Err(Failure::Validation(
            messages::IMAGE_DIMENSIONS_TOO_LARGE.into(),
        ));
    }
    let image = image
        .resize_to_fill(16, 16, image::imageops::FilterType::Nearest)
        .to_rgba8();
    image.save(output).map_err(media_error)
}

pub fn read_image(path: &Path) -> Result<image::DynamicImage> {
    if std::fs::metadata(path).map_err(storage)?.len() > 8_000_000 {
        return Err(Failure::Validation(messages::IMAGE_TOO_LARGE.into()));
    }
    let mut reader = image::ImageReader::open(path)
        .map_err(media_error)?
        .with_guessed_format()
        .map_err(media_error)?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64_000_000);
    reader.limits(limits);
    reader.decode().map_err(media_error)
}

pub fn expected_target() -> &'static str {
    if cfg!(target_os = "windows") {
        "x86_64-pc-windows-msvc"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64-apple-darwin"
    } else {
        "x86_64-apple-darwin"
    }
}
/// Verify all packaged files and contained link targets before either entrypoint can execute a helper.
pub fn verify_manifest(root: &Path, target: &str) -> Result<()> {
    use sha2::{Digest, Sha256};
    #[derive(Deserialize)]
    struct Manifest {
        target: String,
        files: std::collections::BTreeMap<String, String>,
        #[serde(default)]
        links: std::collections::BTreeMap<String, String>,
    }
    let root = root.canonicalize().map_err(storage)?;
    let manifest: Manifest =
        serde_json::from_slice(&std::fs::read(root.join("manifest.json")).map_err(storage)?)
            .map_err(storage)?;
    if manifest.target != target {
        return Err(Failure::Tool(
            "Tool manifest targets a different platform".into(),
        ));
    }
    for (name, digest) in &manifest.files {
        let relative = Path::new(name);
        if !relative
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
            || name == "manifest.json"
        {
            return Err(Failure::Tool("Invalid manifest path".into()));
        }
        let path = root.join(relative);
        let resolved = path.canonicalize().map_err(storage)?;
        if !resolved.starts_with(&root)
            || std::fs::symlink_metadata(&path)
                .map_err(storage)?
                .file_type()
                .is_symlink()
        {
            return Err(Failure::Tool("Invalid tool file target".into()));
        }
        let mut input = std::fs::File::open(path).map_err(storage)?;
        let mut hash = Sha256::new();
        let mut buffer = [0; 65536];
        loop {
            let n = input.read(&mut buffer).map_err(storage)?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
        if hex::encode(hash.finalize()) != *digest {
            return Err(Failure::Tool(format!("Tool integrity failure: {name}")));
        }
    }
    for (name, link) in &manifest.links {
        let path = root.join(name);
        let actual = std::fs::read_link(&path).map_err(storage)?;
        if actual != Path::new(link) || !path.canonicalize().map_err(storage)?.starts_with(&root) {
            return Err(Failure::Tool("Tool symlink integrity failure".into()));
        }
    }
    for required in [
        if target.contains("windows") {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        },
        if target.contains("windows") {
            "ffprobe.exe"
        } else {
            "ffprobe"
        },
        if target.contains("windows") {
            "deno.exe"
        } else {
            "deno"
        },
        if target.contains("windows") {
            "youtube/yt-dlp.exe"
        } else {
            "youtube/yt-dlp_macos"
        },
    ] {
        if !manifest.files.contains_key(required) {
            return Err(Failure::Tool(format!(
                "Required helper missing from manifest: {required}"
            )));
        }
    }
    fn inventory(
        root: &Path,
        path: &Path,
        files: &std::collections::BTreeMap<String, String>,
        links: &std::collections::BTreeMap<String, String>,
    ) -> Result<()> {
        for entry in std::fs::read_dir(path).map_err(storage)? {
            let entry = entry.map_err(storage)?;
            let path = entry.path();
            let name = path
                .strip_prefix(root)
                .map_err(storage)?
                .to_string_lossy()
                .replace('\\', "/");
            if name == "manifest.json" {
                continue;
            }
            let kind = entry.file_type().map_err(storage)?;
            if kind.is_symlink() {
                if !links.contains_key(&name) {
                    return Err(Failure::Tool(format!("Unlisted tool link: {name}")));
                }
            } else if kind.is_file() {
                if !files.contains_key(&name) {
                    return Err(Failure::Tool(format!("Unlisted tool file: {name}")));
                }
            } else if kind.is_dir() {
                inventory(root, &path, files, links)?;
            }
        }
        Ok(())
    }
    inventory(&root, &root, &manifest.files, &manifest.links)?;
    if target.contains("windows") {
        for dependency in [
            "avcodec-63.dll",
            "avformat-63.dll",
            "avutil-61.dll",
            "avfilter-12.dll",
            "avdevice-63.dll",
            "swresample-7.dll",
            "swscale-10.dll",
        ] {
            if !manifest.files.contains_key(dependency) {
                return Err(Failure::Tool(format!(
                    "Required FFmpeg dependency absent: {dependency}"
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod loudness_tests {
    use super::LoudnessMeasure;
    const REPORT: &str = r#"Input #0, mp3, from 'in.mp3':
  Duration: 00:00:03.00, start: 0.000000, bitrate: 192 kb/s
[Parsed_loudnorm_2 @ 0x600000] 
{
	"input_i" : "-23.54",
	"input_tp" : "-7.12",
	"input_lra" : "5.30",
	"input_thresh" : "-33.90",
	"output_i" : "-16.02",
	"output_tp" : "-1.50",
	"output_lra" : "4.80",
	"output_thresh" : "-26.40",
	"normalization_type" : "dynamic",
	"target_offset" : "0.02"
}
"#;
    #[test]
    fn the_first_pass_summary_drives_a_linear_second_pass() {
        let measure = LoudnessMeasure::parse(REPORT).unwrap();
        assert_eq!(measure.integrated, -23.54);
        assert_eq!(measure.offset, 0.02);
        assert_eq!(
            measure.filter(),
            "loudnorm=I=-16:TP=-1.5:LRA=11:measured_I=-23.54:measured_TP=-7.12:measured_LRA=5.30:measured_thresh=-33.90:offset=0.02:linear=true"
        );
    }
    #[test]
    fn silence_or_missing_output_falls_back() {
        assert!(LoudnessMeasure::parse(&REPORT.replace("\"-23.54\"", "\"-inf\"")).is_none());
        assert!(LoudnessMeasure::parse("Conversion failed!").is_none());
        assert!(LoudnessMeasure::parse("").is_none());
    }
}
