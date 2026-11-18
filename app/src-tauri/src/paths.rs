//! Which paths the interface may hand to the commands that open, show, copy or read a file.
//!
//! The page only ever names files it was shown: results from the chosen folders, pictures and pages the helper made
//! in the app's own folder, and files the person gave it by the file dialog or by dropping them on the window. A
//! path from anywhere else is refused before anything touches it, so a page that went wrong can't run a program,
//! show a folder outside the chosen ones, or make Windows reach out to another computer (a `\\server\share` path
//! sends this computer's sign-in to that server).

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf, Prefix};
use std::sync::Mutex;

/// Files the person handed over themselves (the file dialog, a drop, a pasted picture), which may be outside the
/// chosen folders.
#[derive(Default)]
pub struct Given(Mutex<HashSet<String>>);

impl Given {
    pub fn add(&self, path: &Path) {
        if let Ok(mut given) = self.0.lock() {
            // A long session drops and picks many files; only the latest few are ever searched by.
            if given.len() >= 256 {
                given.clear();
            }
            given.insert(key(path));
        }
    }

    fn has(&self, path: &Path) -> bool {
        self.0.lock().is_ok_and(|given| given.contains(&key(path)))
    }
}

/// `path` as one comparable string: `\\?\` prefixes off, `/` as `\`, case folded (Windows names ignore case).
fn key(path: &Path) -> String {
    let text = path.to_string_lossy().replace('/', "\\");
    let text = match text.strip_prefix(r"\\?\UNC\") {
        Some(rest) => format!(r"\\{rest}"),
        None => text.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(text),
    };
    text.trim_end_matches('\\').to_lowercase()
}

fn inside(path: &str, folder: &str) -> bool {
    !folder.is_empty() && (path == folder || path.strip_prefix(folder).is_some_and(|rest| rest.starts_with('\\')))
}

/// A path is on another computer when it's `\\server\share\…` (or the same in its `\\?\UNC\` form).
fn is_remote(path: &Path) -> bool {
    match path.components().next() {
        Some(Component::Prefix(prefix)) => matches!(prefix.kind(), Prefix::UNC(..) | Prefix::VerbatimUNC(..)),
        _ => path.to_string_lossy().starts_with("\\\\") || path.to_string_lossy().starts_with("//"),
    }
}

/// `text` as a path the interface may use, or why not. `folders` are the chosen folders and the app's own.
pub fn check(text: &str, folders: &[PathBuf], given: &Given) -> Result<PathBuf, String> {
    let refused = || Err("that file isn't one Effodio was given".to_string());
    if text.is_empty() || text.contains(['\0', '"']) {
        return refused();
    }
    let path = PathBuf::from(text);
    let absolute = match path.components().next() {
        Some(Component::Prefix(_)) => path.has_root(),
        _ => false,
    };
    if !absolute || path.components().any(|part| matches!(part, Component::ParentDir | Component::CurDir)) {
        return refused();
    }
    if given.has(&path) {
        return Ok(path);
    }
    let wanted = key(&path);
    for folder in folders {
        // A chosen folder on another computer is the person's choice; any other remote path is refused above all.
        if inside(&wanted, &key(folder)) {
            return Ok(path);
        }
    }
    if is_remote(&path) {
        return Err("files on other computers can't be opened from here".to_string());
    }
    refused()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folders() -> Vec<PathBuf> {
        vec![PathBuf::from(r"D:\Photos"), PathBuf::from(r"C:\Users\me\AppData\Local\app.effodio.search")]
    }

    #[test]
    fn files_in_the_chosen_folders_and_the_apps_own() {
        let given = Given::default();
        assert!(check(r"D:\Photos\2026\beach.jpg", &folders(), &given).is_ok());
        assert!(check(r"d:\photos\Beach.JPG", &folders(), &given).is_ok());
        assert!(check(r"\\?\D:\Photos\beach.jpg", &folders(), &given).is_ok());
        assert!(check(r"C:\Users\me\AppData\Local\app.effodio.search\Cache\pages\a.jpg", &folders(), &given).is_ok());
    }

    #[test]
    fn anything_else_is_refused() {
        let given = Given::default();
        for path in [
            r"C:\Windows\System32\calc.exe",
            r"D:\PhotosOther\a.jpg",
            r"D:\Photos\..\Secrets\a.txt",
            r#"D:\Photos\a.jpg" /e,\\host\share"#,
            r"\\attacker\share\a.png",
            r"\\?\UNC\attacker\share\a.png",
            r"Photos\a.jpg",
            r"D:Photos\a.jpg",
            "",
        ] {
            assert!(check(path, &folders(), &given).is_err(), "{path}");
        }
    }

    #[test]
    fn files_handed_over_by_the_person() {
        let given = Given::default();
        let picked = r"E:\Elsewhere\face.jpg";
        assert!(check(picked, &folders(), &given).is_err());
        given.add(Path::new(picked));
        assert!(check(picked, &folders(), &given).is_ok());
        assert!(check(r"E:\Elsewhere\other.jpg", &folders(), &given).is_err());
    }

    #[test]
    fn a_chosen_folder_on_another_computer() {
        let given = Given::default();
        let folders = vec![PathBuf::from(r"\\nas\photos")];
        assert!(check(r"\\nas\photos\a.jpg", &folders, &given).is_ok());
        assert!(check(r"\\nas\private\a.jpg", &folders, &given).is_err());
    }
}
