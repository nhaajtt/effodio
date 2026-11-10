//! What a file is, decided by its extension and name.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileKind {
    Image,
    Screenshot,
    Pdf,
    Doc,
    Audio,
    Video,
}

impl FileKind {
    pub const ALL: [FileKind; 6] =
        [FileKind::Image, FileKind::Screenshot, FileKind::Pdf, FileKind::Doc, FileKind::Audio, FileKind::Video];

    pub fn as_str(self) -> &'static str {
        match self {
            FileKind::Image => "image",
            FileKind::Screenshot => "screenshot",
            FileKind::Pdf => "pdf",
            FileKind::Doc => "doc",
            FileKind::Audio => "audio",
            FileKind::Video => "video",
        }
    }

    pub fn parse(text: &str) -> Option<FileKind> {
        FileKind::ALL.into_iter().find(|kind| kind.as_str() == text)
    }
}

/// Which encoder made a vector. Text, image and audio vectors sit at slightly different levels for the same query, so
/// ranking centres each on its own average.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Modality {
    Text,
    Image,
    Audio,
    None,
}

impl Modality {
    pub const ALL: [Modality; 4] = [Modality::Text, Modality::Image, Modality::Audio, Modality::None];

    pub fn as_str(self) -> &'static str {
        match self {
            Modality::Text => "text",
            Modality::Image => "image",
            Modality::Audio => "audio",
            Modality::None => "none",
        }
    }

    pub fn parse(text: &str) -> Modality {
        Modality::ALL.into_iter().find(|m| m.as_str() == text).unwrap_or(Modality::None)
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

/// What a segment of a file is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SegmentKind {
    /// The file's name and folders: keyword search only.
    Name,
    /// A picture as a whole.
    Image,
    /// Text read from a picture: keyword search only.
    Ocr,
    /// A PDF page (its text, or its picture when it has little text).
    Page,
    /// A passage of a document.
    Chunk,
    /// A window of sound.
    Sound,
    /// A video frame.
    Frame,
}

impl SegmentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SegmentKind::Name => "name",
            SegmentKind::Image => "image",
            SegmentKind::Ocr => "ocr",
            SegmentKind::Page => "page",
            SegmentKind::Chunk => "chunk",
            SegmentKind::Sound => "sound",
            SegmentKind::Frame => "frame",
        }
    }

    pub fn parse(text: &str) -> SegmentKind {
        match text {
            "image" => SegmentKind::Image,
            "ocr" => SegmentKind::Ocr,
            "page" => SegmentKind::Page,
            "chunk" => SegmentKind::Chunk,
            "sound" => SegmentKind::Sound,
            "frame" => SegmentKind::Frame,
            _ => SegmentKind::Name,
        }
    }
}

const IMAGES: &[&str] = &["jpg", "jpeg", "png", "webp", "gif", "bmp", "tif", "tiff", "heic", "heif", "avif", "jfif"];
const AUDIO: &[&str] = &["mp3", "wav", "m4a", "flac", "ogg", "opus", "aac", "wma", "aiff", "aif", "amr"];
const VIDEO: &[&str] = &["mp4", "mov", "mkv", "webm", "avi", "m4v", "wmv", "mpg", "mpeg", "ts", "3gp", "flv"];
const DOCS: &[&str] = &[
    "txt", "md", "markdown", "rtf", "docx", "odt", "html", "htm", "xlsx", "ods", "pptx", "odp", "csv", "typ", "tex",
    "org", "rst", "log", "epub",
];

pub fn extension(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// The words Windows, the Snipping Tool, browsers and phones put in a screenshot's name, in several languages.
const SCREENSHOT_WORDS: &[&str] = &[
    "screenshot", "screen shot", "screen_shot", "screen-shot", "screencap", "screen capture", "snip", "capture",
    "ảnh chụp màn hình", "anh chup man hinh", "captura", "bildschirmfoto", "スクリーンショット", "截图", "截屏",
    "снимок экрана",
];

/// The kind of a file DigUp-style search reads, or None for everything else.
pub fn kind_of(path: &Path) -> Option<FileKind> {
    let extension = extension(path);
    let extension = extension.as_str();
    if IMAGES.contains(&extension) {
        return Some(if looks_like_screenshot(path) { FileKind::Screenshot } else { FileKind::Image });
    }
    if extension == "pdf" {
        return Some(FileKind::Pdf);
    }
    if AUDIO.contains(&extension) {
        return Some(FileKind::Audio);
    }
    if VIDEO.contains(&extension) {
        // A .ts file next to code is TypeScript; the rules skip code projects, and a stray one fails to decode.
        return Some(FileKind::Video);
    }
    if DOCS.contains(&extension) {
        return Some(FileKind::Doc);
    }
    None
}

pub fn looks_like_screenshot(path: &Path) -> bool {
    let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    if SCREENSHOT_WORDS.iter().any(|word| name.contains(word)) {
        return true;
    }
    path.parent()
        .and_then(Path::file_name)
        .map(|folder| folder.to_string_lossy().to_lowercase())
        .is_some_and(|folder| folder == "screenshots" || folder == "ảnh chụp màn hình" || folder == "screen captures")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_by_extension_and_name() {
        assert_eq!(kind_of(Path::new("C:/a/IMG_2214.JPG")), Some(FileKind::Image));
        assert_eq!(kind_of(Path::new("C:/a/Screenshot 2026-10-01 101010.png")), Some(FileKind::Screenshot));
        assert_eq!(kind_of(Path::new("C:/Pictures/Screenshots/a.png")), Some(FileKind::Screenshot));
        assert_eq!(kind_of(Path::new("C:/a/Ảnh chụp màn hình (3).png")), Some(FileKind::Screenshot));
        assert_eq!(kind_of(Path::new("lease.pdf")), Some(FileKind::Pdf));
        assert_eq!(kind_of(Path::new("talk.m4a")), Some(FileKind::Audio));
        assert_eq!(kind_of(Path::new("trip.mov")), Some(FileKind::Video));
        assert_eq!(kind_of(Path::new("notes.docx")), Some(FileKind::Doc));
        assert_eq!(kind_of(Path::new("app.exe")), None);
        assert_eq!(kind_of(Path::new("noext")), None);
    }
}
