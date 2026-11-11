//! Sound and video frames, decoded by FFmpeg running as a separate process: a broken file can't take the indexer
//! down with it.

use super::Picture;
use anyhow::{anyhow, bail, Context, Result};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

pub const SAMPLE_RATE: usize = 16_000;
/// The model hears 30 seconds at a time; windows start every 25, so no sentence falls between two.
pub const WINDOW_SECONDS: usize = 30;
pub const STEP_SECONDS: usize = 25;
/// A video frame is looked at every this many seconds.
pub const FRAME_STEP: f64 = 3.0;

fn tool(name: &str, variable: &str) -> Option<PathBuf> {
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    if let Some(chosen) = std::env::var_os(variable) {
        let chosen = PathBuf::from(chosen);
        if chosen.is_file() {
            return Some(chosen);
        }
    }
    if let Some(beside) = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|p| p.join(&file))) {
        if beside.is_file() {
            return Some(beside);
        }
    }
    std::env::var_os("PATH")
        .and_then(|paths| std::env::split_paths(&paths).map(|p| p.join(&file)).find(|p| p.is_file()))
        .or_else(|| find(&crate::config::models_folder().join("Tools").join("media"), &file, 0))
}

/// A program somewhere under a folder (the copy of FFmpeg Effodio fetched for itself).
fn find(folder: &Path, file: &str, depth: usize) -> Option<PathBuf> {
    for entry in std::fs::read_dir(folder).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth < 3 {
                if let Some(found) = find(&path, file, depth + 1) {
                    return Some(found);
                }
            }
        } else if path.file_name().is_some_and(|name| name.eq_ignore_ascii_case(file)) {
            return Some(path);
        }
    }
    None
}

/// FFmpeg: `$EFFODIO_FFMPEG`, beside this executable, on the PATH, or the copy Effodio fetched for itself.
pub fn ffmpeg() -> Option<PathBuf> {
    tool("ffmpeg", "EFFODIO_FFMPEG")
}

pub fn ffprobe() -> Option<PathBuf> {
    tool("ffprobe", "EFFODIO_FFPROBE").or_else(|| {
        ffmpeg().and_then(|path| {
            let beside = path.with_file_name(format!("ffprobe{}", std::env::consts::EXE_SUFFIX));
            beside.is_file().then_some(beside)
        })
    })
}

fn command(program: &Path) -> Command {
    let mut command = Command::new(program);
    command.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // no console window
    }
    command
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Probe {
    pub duration: f64,
    pub has_audio: bool,
    pub has_video: bool,
    pub width: u32,
    pub height: u32,
    pub title: Option<String>,
    pub artist: Option<String>,
}

/// What a recording or video holds, read by ffprobe.
pub fn probe(path: &Path) -> Result<Probe> {
    let ffprobe = ffprobe().ok_or_else(|| anyhow!("FFmpeg isn't installed (no ffprobe)"))?;
    let output = command(&ffprobe)
        .args(["-v", "error", "-print_format", "json", "-show_format", "-show_streams"])
        .arg(path)
        .stderr(Stdio::null())
        .output()
        .context("can't run ffprobe")?;
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|_| anyhow!("not a media file"))?;
    let mut out = Probe::default();
    let number = |value: &serde_json::Value| -> f64 {
        value.as_str().and_then(|text| text.parse().ok()).or_else(|| value.as_f64()).unwrap_or(0.0)
    };
    out.duration = number(&json["format"]["duration"]);
    for stream in json["streams"].as_array().into_iter().flatten() {
        match stream["codec_type"].as_str() {
            Some("audio") => out.has_audio = true,
            // Cover art in a song is a "video" stream of one picture.
            Some("video") if stream["disposition"]["attached_pic"].as_i64() != Some(1) => {
                out.has_video = true;
                out.width = stream["width"].as_u64().unwrap_or(0) as u32;
                out.height = stream["height"].as_u64().unwrap_or(0) as u32;
            }
            _ => {}
        }
        if out.duration <= 0.0 {
            out.duration = number(&stream["duration"]);
        }
    }
    let tags = &json["format"]["tags"];
    let tag = |name: &str| -> Option<String> {
        tags.as_object()?.iter().find(|(key, _)| key.eq_ignore_ascii_case(name))?.1.as_str().map(str::to_string)
    };
    out.title = tag("title");
    out.artist = tag("artist");
    if !out.has_audio && !out.has_video {
        bail!("no sound or picture in the file");
    }
    Ok(out)
}

/// A running FFmpeg whose output is read to the end or dropped; either way the process doesn't outlive it.
struct Stream {
    child: Child,
}

impl Drop for Stream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The windows of a recording's sound, each 30 seconds of mono samples at 16 kHz (the last one shorter), with the
/// second it starts at. A window that's nearly silent is passed over.
pub struct Windows {
    stream: Stream,
    buffer: Vec<f32>,
    start: usize,
    done: bool,
    /// Samples read since the last window went out: otherwise what's left is only the overlap.
    fresh: usize,
    emitted: bool,
}

pub fn windows(path: &Path, from_second: f64) -> Result<Windows> {
    let ffmpeg = ffmpeg().ok_or_else(|| anyhow!("FFmpeg isn't installed"))?;
    let mut run = command(&ffmpeg);
    run.args(["-v", "error", "-nostdin"]);
    if from_second > 0.0 {
        run.args(["-ss", &format!("{from_second:.3}")]);
    }
    let child = run
        .arg("-i")
        .arg(path)
        .args(["-vn", "-sn", "-dn", "-ac", "1", "-ar", &SAMPLE_RATE.to_string(), "-f", "f32le", "-"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("can't run ffmpeg")?;
    Ok(Windows { stream: Stream { child }, buffer: Vec::new(), start: (from_second as usize), done: false, fresh: 0, emitted: false })
}

impl Iterator for Windows {
    type Item = (f64, Vec<f32>);

    fn next(&mut self) -> Option<Self::Item> {
        let window = WINDOW_SECONDS * SAMPLE_RATE;
        let step = STEP_SECONDS * SAMPLE_RATE;
        loop {
            if !self.done && self.buffer.len() < window {
                let mut bytes = vec![0u8; (window - self.buffer.len()) * 4];
                let mut filled = 0;
                let output = self.stream.child.stdout.as_mut()?;
                while filled < bytes.len() {
                    match output.read(&mut bytes[filled..]) {
                        Ok(0) | Err(_) => {
                            self.done = true;
                            break;
                        }
                        Ok(read) => filled += read,
                    }
                }
                self.fresh += filled / 4;
                self.buffer.extend(
                    bytes[..filled - filled % 4].chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])),
                );
            }
            // Less than two new seconds at the end add nothing the window before didn't hear.
            let least = if self.emitted { 2 * SAMPLE_RATE } else { SAMPLE_RATE / 2 };
            if self.buffer.is_empty() || (self.done && self.fresh < least) {
                return None;
            }
            self.fresh = 0;
            self.emitted = true;
            let take = self.buffer.len().min(window);
            let samples = self.buffer[..take].to_vec();
            let start = self.start as f64;
            let last = self.done && self.buffer.len() <= window;
            if last {
                self.buffer.clear();
            } else {
                self.buffer.drain(..step.min(self.buffer.len()));
                self.start += STEP_SECONDS;
            }
            let energy = samples.iter().map(|s| (s * s) as f64).sum::<f64>() / samples.len() as f64;
            if energy.sqrt() < 0.0015 {
                if last {
                    return None;
                }
                continue;
            }
            return Some((start, samples));
        }
    }
}

/// A video's frames, one every `FRAME_STEP` seconds, scaled to fit 448 × 448, with the second each is from.
pub struct Frames {
    stream: Stream,
    index: usize,
    from: f64,
}

pub fn frames(path: &Path, from_second: f64) -> Result<Frames> {
    let ffmpeg = ffmpeg().ok_or_else(|| anyhow!("FFmpeg isn't installed"))?;
    let mut run = command(&ffmpeg);
    run.args(["-v", "error", "-nostdin"]);
    if from_second > 0.0 {
        run.args(["-ss", &format!("{from_second:.3}")]);
    }
    let filter = format!("fps=1/{FRAME_STEP},scale=448:448:force_original_aspect_ratio=decrease:force_divisible_by=2");
    let child = run
        .arg("-i")
        .arg(path)
        .args(["-an", "-sn", "-dn", "-vf", &filter, "-f", "image2pipe", "-c:v", "ppm", "-"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("can't run ffmpeg")?;
    Ok(Frames { stream: Stream { child }, index: 0, from: from_second })
}

impl Iterator for Frames {
    type Item = (f64, Picture);

    fn next(&mut self) -> Option<Self::Item> {
        let output = self.stream.child.stdout.as_mut()?;
        let picture = read_ppm(output).ok()?;
        // The fps filter's first frame stands for the middle of the first step.
        let second = self.from + self.index as f64 * FRAME_STEP;
        self.index += 1;
        Some((second, picture))
    }
}

/// One frame at `second`, at most `side` pixels wide or tall (previews and thumbnails).
pub fn frame_at(path: &Path, second: f64, side: u32) -> Result<Picture> {
    let ffmpeg = ffmpeg().ok_or_else(|| anyhow!("FFmpeg isn't installed"))?;
    let filter = format!("scale={side}:{side}:force_original_aspect_ratio=decrease:force_divisible_by=2");
    let mut child = command(&ffmpeg)
        .args(["-v", "error", "-nostdin", "-ss", &format!("{:.3}", second.max(0.0))])
        .arg("-i")
        .arg(path)
        .args(["-an", "-frames:v", "1", "-vf", &filter, "-f", "image2pipe", "-c:v", "ppm", "-"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("can't run ffmpeg")?;
    let picture = read_ppm(child.stdout.as_mut().unwrap());
    let _ = child.kill();
    let _ = child.wait();
    picture
}

/// One binary PPM ("P6 width height 255" and packed RGB) from a stream of them.
fn read_ppm(input: &mut impl Read) -> Result<Picture> {
    let token = |input: &mut dyn Read| -> Result<String> {
        let mut out = String::new();
        let mut byte = [0u8; 1];
        loop {
            input.read_exact(&mut byte)?;
            let c = byte[0] as char;
            if c.is_ascii_whitespace() {
                if !out.is_empty() {
                    return Ok(out);
                }
            } else {
                out.push(c);
            }
        }
    };
    if token(input)? != "P6" {
        bail!("not a PPM frame");
    }
    let width: usize = token(input)?.parse()?;
    let height: usize = token(input)?.parse()?;
    let _max = token(input)?;
    if width == 0 || height == 0 || width * height > 64_000_000 {
        bail!("an impossible frame size");
    }
    let mut rgb = vec![0u8; width * height * 3];
    input.read_exact(&mut rgb)?;
    Ok(Picture { width, height, rgb })
}

/// How much a frame differs from another, 0 to 1: the mean difference of their 16 × 16 grey thumbnails. A cut is
/// above about 0.06; the same shot a few seconds on is below.
pub fn difference(a: &[u8; 256], b: &[u8; 256]) -> f64 {
    a.iter().zip(b).map(|(&x, &y)| (x as f64 - y as f64).abs()).sum::<f64>() / (256.0 * 255.0)
}

/// A frame as 16 × 16 grey levels, for `difference`.
pub fn signature(picture: &Picture) -> [u8; 256] {
    let mut out = [0u8; 256];
    if picture.width == 0 || picture.height == 0 {
        return out;
    }
    for cell_y in 0..16 {
        for cell_x in 0..16 {
            let (x0, x1) = (cell_x * picture.width / 16, ((cell_x + 1) * picture.width / 16).max(cell_x * picture.width / 16 + 1));
            let (y0, y1) = (cell_y * picture.height / 16, ((cell_y + 1) * picture.height / 16).max(cell_y * picture.height / 16 + 1));
            let (mut sum, mut count) = (0u64, 0u64);
            for y in (y0..y1.min(picture.height)).step_by(2) {
                for x in (x0..x1.min(picture.width)).step_by(2) {
                    let p = (y * picture.width + x) * 3;
                    sum += (picture.rgb[p] as u64 * 299 + picture.rgb[p + 1] as u64 * 587 + picture.rgb[p + 2] as u64 * 114) / 1000;
                    count += 1;
                }
            }
            out[cell_y * 16 + cell_x] = (sum / count.max(1)) as u8;
        }
    }
    out
}

/// A frame that shows nothing: black, white or one flat colour.
pub fn is_blank(signature: &[u8; 256]) -> bool {
    let (min, max) = signature.iter().fold((255u8, 0u8), |(lo, hi), &v| (lo.min(v), hi.max(v)));
    max - min < 12
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ppm_frames_parse_one_after_another() {
        let mut bytes = Vec::new();
        for value in [10u8, 200u8] {
            bytes.extend_from_slice(b"P6\n2 1\n255\n");
            bytes.extend_from_slice(&[value; 6]);
        }
        let mut input = &bytes[..];
        let first = read_ppm(&mut input).unwrap();
        assert_eq!((first.width, first.height, first.rgb[0]), (2, 1, 10));
        assert_eq!(read_ppm(&mut input).unwrap().rgb[0], 200);
        assert!(read_ppm(&mut input).is_err());
    }

    #[test]
    fn signatures_tell_shots_apart() {
        let flat = Picture { width: 64, height: 64, rgb: vec![40; 64 * 64 * 3] };
        let mut split = flat.clone();
        for y in 0..64 {
            for x in 32..64 {
                let p = (y * 64 + x) * 3;
                split.rgb[p..p + 3].copy_from_slice(&[230, 230, 230]);
            }
        }
        assert!(is_blank(&signature(&flat)));
        assert!(!is_blank(&signature(&split)));
        assert!(difference(&signature(&flat), &signature(&split)) > 0.3);
        assert_eq!(difference(&signature(&split), &signature(&split)), 0.0);
    }
}
