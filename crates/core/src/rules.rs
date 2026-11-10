//! What is never read: secrets, system and app folders, caches, code projects (except their screenshots), datasets,
//! and files that only live in the cloud.

use crate::kinds::{extension, kind_of, FileKind};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The user's own additions to the rules.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Custom {
    /// Folder names or full paths to leave out.
    #[serde(default)]
    pub skip_folders: Vec<String>,
    /// Extensions to leave out, without the dot.
    #[serde(default)]
    pub skip_extensions: Vec<String>,
    /// Kinds to leave out.
    #[serde(default)]
    pub skip_kinds: Vec<FileKind>,
}

/// Folders that hold programs, caches or other machinery, by name (lowercase).
const SKIP_FOLDERS: &[&str] = &[
    "node_modules", "__pycache__", "site-packages", "venv", ".venv", "env", "target", "build", "dist", "out", "bin",
    "obj", "vendor", "cache", "caches", "tmp", "temp", "logs", "$recycle.bin", "system volume information", "windows",
    "program files", "program files (x86)", "programdata", "appdata", "application data", "msocache", "recovery",
    "perflogs", "intel", "nvidia", "steamapps", "steamlibrary", "epic games", "riot games", "thumbnails",
    "cachestorage", "code cache", "gpucache", "service worker", "indexeddb", "local storage", "session storage",
    "onedrivetemp", "winsxs", "installer", "packages", "lib", "libs", "include", "pods", "deriveddata",
];

/// Names that say a folder is a code project: inside one, only screenshots are read.
const PROJECT_MARKERS: &[&str] = &[
    ".git", "package.json", "cargo.toml", "pyproject.toml", "go.mod", "pom.xml", "build.gradle", "cmakelists.txt",
    "makefile", "gemfile", "composer.json", "package.swift", ".hg", ".svn", "requirements.txt", "setup.py",
];

/// File names and extensions that hold keys or passwords.
const SECRET_EXTENSIONS: &[&str] = &[
    "pem", "key", "p12", "pfx", "kdbx", "kdb", "keystore", "jks", "ppk", "gpg", "asc", "crt", "cer", "der",
    "wallet", "ovpn", "rdp",
];
const SECRET_NAMES: &[&str] = &[
    "id_rsa", "id_dsa", "id_ecdsa", "id_ed25519", ".env", ".netrc", ".npmrc", ".pypirc", "credentials",
    "credentials.json", "secrets.json", "secrets.yaml", "secrets.yml", "passwords.txt", "password.txt",
    "wallet.dat", "known_hosts", "authorized_keys", "htpasswd", ".htpasswd",
];
const SECRET_WORDS: &[&str] = &["password", "passwd", "secret", "private_key", "privatekey", "recovery-code",
    "recovery code", "seed phrase", "seedphrase", "mnemonic", "2fa", "backup-codes", "backup codes", "mật khẩu",
    "mat khau"];

/// The largest file of each kind worth reading.
pub fn size_limit(kind: FileKind) -> u64 {
    match kind {
        FileKind::Image | FileKind::Screenshot => 80 << 20,
        FileKind::Pdf => 500 << 20,
        FileKind::Doc => 60 << 20,
        FileKind::Audio => 2 << 30,
        FileKind::Video => 40 << 30,
    }
}

pub fn is_secret(path: &Path) -> bool {
    let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    if SECRET_NAMES.contains(&name.as_str()) || name.starts_with(".env.") {
        return true;
    }
    if SECRET_EXTENSIONS.contains(&extension(path).as_str()) {
        return true;
    }
    SECRET_WORDS.iter().any(|word| name.contains(word))
}

/// A folder the crawler doesn't go into.
pub fn skip_folder(path: &Path, custom: &Custom) -> bool {
    let Some(name) = path.file_name().map(|n| n.to_string_lossy().to_lowercase()) else { return false };
    if name.starts_with('.') || name.starts_with('$') || name.starts_with('~') {
        return true;
    }
    if SKIP_FOLDERS.contains(&name.as_str()) {
        return true;
    }
    let full = path.to_string_lossy().to_lowercase();
    custom.skip_folders.iter().any(|skip| {
        let skip = skip.to_lowercase();
        let skip = skip.trim_end_matches(['\\', '/']);
        name == skip || full == skip
    })
}

/// Whether a folder's own entries say it's a code project.
pub fn is_project(entries: &[String]) -> bool {
    entries.iter().any(|entry| {
        let entry = entry.to_lowercase();
        PROJECT_MARKERS.contains(&entry.as_str()) || entry.ends_with(".sln") || entry.ends_with(".csproj")
            || entry.ends_with(".xcodeproj")
    })
}

/// A folder of thousands of similar files with numbered names is a dataset or an export, not something to search.
pub fn looks_like_dataset(names: &[String]) -> bool {
    if names.len() < 2000 {
        return false;
    }
    let numbered = names
        .iter()
        .filter(|name| {
            let stem = name.rsplit_once('.').map_or(name.as_str(), |(stem, _)| stem);
            let digits = stem.chars().filter(char::is_ascii_digit).count();
            digits * 10 >= stem.chars().count().max(1) * 7
        })
        .count();
    numbered * 10 >= names.len() * 9
}

/// The kind a file is read as, or None when the rules leave it out. `in_project`: under a code project's folder.
pub fn accept(path: &Path, size: u64, in_project: bool, custom: &Custom) -> Option<FileKind> {
    let name = path.file_name()?.to_string_lossy();
    if name.starts_with('.') || name.starts_with("~$") || name.ends_with(".tmp") || name.ends_with(".crdownload")
        || name.ends_with(".part")
    {
        return None;
    }
    if is_secret(path) {
        return None;
    }
    let kind = kind_of(path)?;
    if custom.skip_extensions.iter().any(|skip| skip.trim_start_matches('.').eq_ignore_ascii_case(&extension(path))) {
        return None;
    }
    if custom.skip_kinds.contains(&kind) {
        return None;
    }
    if in_project && kind != FileKind::Screenshot {
        return None;
    }
    if size == 0 || size > size_limit(kind) {
        return None;
    }
    // Icons and sprites: too small to show anything.
    if matches!(kind, FileKind::Image | FileKind::Screenshot) && size < 4 << 10 {
        return None;
    }
    Some(kind)
}

/// A file whose bytes aren't on this computer (OneDrive's "online-only"): reading it would download it.
#[cfg(windows)]
pub fn is_cloud_only(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const OFFLINE: u32 = 0x1000;
    const RECALL_ON_OPEN: u32 = 0x0004_0000;
    const RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;
    metadata.file_attributes() & (OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS) != 0
}

#[cfg(not(windows))]
pub fn is_cloud_only(_metadata: &std::fs::Metadata) -> bool {
    false
}

/// Hidden or system, as Explorer marks them.
#[cfg(windows)]
pub fn is_hidden(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & (0x2 | 0x4) != 0
}

#[cfg(not(windows))]
pub fn is_hidden(_metadata: &std::fs::Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_never_read() {
        for name in ["id_rsa", ".env", ".env.local", "server.pem", "vault.kdbx", "My Passwords.docx",
                     "recovery-codes.txt", "mật khẩu wifi.txt"] {
            assert!(is_secret(Path::new(name)), "{name}");
            assert_eq!(accept(Path::new(name), 5000, false, &Custom::default()), None, "{name}");
        }
        assert!(!is_secret(Path::new("lease.pdf")));
    }

    #[test]
    fn code_projects_give_only_screenshots() {
        let custom = Custom::default();
        assert!(is_project(&["src".into(), "Cargo.toml".into()]));
        assert!(!is_project(&["photos".into(), "notes.txt".into()]));
        assert_eq!(accept(Path::new("C:/p/README.md"), 5000, true, &custom), None);
        assert_eq!(accept(Path::new("C:/p/docs/Screenshot 1.png"), 50_000, true, &custom), Some(FileKind::Screenshot));
        assert_eq!(accept(Path::new("C:/d/README.md"), 5000, false, &custom), Some(FileKind::Doc));
    }

    #[test]
    fn machinery_folders_are_skipped() {
        let custom = Custom { skip_folders: vec!["Private".into()], ..Default::default() };
        for folder in ["C:/a/node_modules", "C:/a/.ssh", "C:/Users/x/AppData", "C:/a/private", "C:/$Recycle.Bin"] {
            assert!(skip_folder(Path::new(folder), &custom), "{folder}");
        }
        assert!(!skip_folder(Path::new("C:/Users/x/Documents"), &custom));
    }

    #[test]
    fn datasets_are_recognised() {
        let numbered: Vec<String> = (0..3000).map(|i| format!("{i:06}.jpg")).collect();
        assert!(looks_like_dataset(&numbered));
        let named: Vec<String> = (0..3000).map(|i| format!("holiday in hue day {i}.jpg")).collect();
        assert!(!looks_like_dataset(&named));
        assert!(!looks_like_dataset(&numbered[..100]));
    }

    #[test]
    fn custom_rules_and_sizes() {
        let custom = Custom { skip_extensions: vec![".gif".into()], skip_kinds: vec![FileKind::Video], ..Default::default() };
        assert_eq!(accept(Path::new("a.gif"), 50_000, false, &custom), None);
        assert_eq!(accept(Path::new("a.mp4"), 50_000, false, &custom), None);
        assert_eq!(accept(Path::new("icon.png"), 900, false, &custom), None);
        assert_eq!(accept(Path::new("~$report.docx"), 5000, false, &custom), None);
        assert_eq!(accept(Path::new("photo.jpg"), 50_000, false, &custom), Some(FileKind::Image));
    }
}
