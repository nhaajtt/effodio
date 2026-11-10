//! Walks the chosen folders and lists the files worth reading, by the rules.

use crate::rules::{self, Custom};
use crate::store::Seen;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// A folder to search: all of it, or only some of its subfolders.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Root {
    pub path: PathBuf,
    /// Subfolders (relative to `path`) left out.
    #[serde(default)]
    pub exclude: Vec<PathBuf>,
}

#[derive(Default, Debug)]
pub struct Crawl {
    pub files: Vec<Seen>,
    /// Folders left out because they look like datasets.
    pub datasets: Vec<PathBuf>,
    /// Files left alone because they only live in the cloud.
    pub cloud_only: usize,
    pub folders: usize,
}

fn modified_seconds(metadata: &std::fs::Metadata) -> f64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// What one file is to the index, or None when the rules leave it out (the watcher asks about single files).
pub fn see(path: &Path, roots: &[Root], custom: &Custom) -> Option<Seen> {
    let root = roots.iter().filter(|root| path.starts_with(&root.path)).max_by_key(|root| root.path.as_os_str().len())?;
    let relative = path.strip_prefix(&root.path).ok()?;
    if root.exclude.iter().any(|excluded| relative.starts_with(excluded)) {
        return None;
    }
    // Every folder between the root and the file must pass the rules, and none may be a code project.
    let mut folder = root.path.clone();
    let mut in_project = is_project_folder(&folder);
    if let Some(parent) = relative.parent() {
        for part in parent.components() {
            folder.push(part);
            if rules::skip_folder(&folder, custom) {
                return None;
            }
            in_project = in_project || is_project_folder(&folder);
        }
    }
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() || rules::is_hidden(&metadata) || rules::is_cloud_only(&metadata) {
        return None;
    }
    let kind = rules::accept(path, metadata.len(), in_project, custom)?;
    Some(Seen {
        path: path.to_path_buf(),
        root: root.path.clone(),
        kind,
        size: metadata.len(),
        modified: modified_seconds(&metadata),
    })
}

fn is_project_folder(folder: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(folder) else { return false };
    let names: Vec<String> =
        entries.flatten().take(400).map(|entry| entry.file_name().to_string_lossy().into_owned()).collect();
    rules::is_project(&names)
}

/// Lists every file of `roots` the rules accept. `stop` ends it early.
pub fn crawl(roots: &[Root], custom: &Custom, stop: &AtomicBool) -> Crawl {
    let mut out = Crawl::default();
    for root in roots {
        if root.path.is_dir() {
            walk(&root.path, root, false, custom, stop, &mut out);
        }
    }
    out
}

fn walk(folder: &Path, root: &Root, in_project: bool, custom: &Custom, stop: &AtomicBool, out: &mut Crawl) {
    if stop.load(Ordering::Relaxed) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    out.folders += 1;
    let mut files = Vec::new();
    let mut folders = Vec::new();
    let mut names = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        // DirEntry's metadata doesn't follow links, so a junction or symlink is never walked into.
        let Ok(metadata) = entry.metadata() else { continue };
        names.push(entry.file_name().to_string_lossy().into_owned());
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            folders.push((path, metadata));
        } else if metadata.is_file() {
            files.push((path, metadata));
        }
    }
    let in_project = in_project || rules::is_project(&names);
    let file_names: Vec<String> =
        files.iter().filter_map(|(path, _)| path.file_name().map(|n| n.to_string_lossy().into_owned())).collect();
    if rules::looks_like_dataset(&file_names) {
        out.datasets.push(folder.to_path_buf());
    } else {
        for (path, metadata) in files {
            if rules::is_hidden(&metadata) {
                continue;
            }
            let Some(kind) = rules::accept(&path, metadata.len(), in_project, custom) else { continue };
            if rules::is_cloud_only(&metadata) {
                out.cloud_only += 1;
                continue;
            }
            out.files.push(Seen {
                path,
                root: root.path.clone(),
                kind,
                size: metadata.len(),
                modified: modified_seconds(&metadata),
            });
        }
    }
    for (path, metadata) in folders {
        if rules::is_hidden(&metadata) || rules::skip_folder(&path, custom) {
            continue;
        }
        if let Ok(relative) = path.strip_prefix(&root.path) {
            if root.exclude.iter().any(|excluded| relative == excluded) {
                continue;
            }
        }
        walk(&path, root, in_project, custom, stop, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kinds::FileKind;
    use std::fs;

    fn write(path: &Path, bytes: usize) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![b'x'; bytes]).unwrap();
    }

    #[test]
    fn crawl_follows_the_rules() {
        let folder = tempfile::tempdir().unwrap();
        let base = folder.path();
        write(&base.join("notes.txt"), 100);
        write(&base.join("trip/beach.jpg"), 10_000);
        write(&base.join("trip/.hidden/secret.jpg"), 10_000);
        write(&base.join("node_modules/readme.md"), 100);
        write(&base.join("project/package.json"), 10);
        write(&base.join("project/README.md"), 100);
        write(&base.join("project/docs/Screenshot 1.png"), 10_000);
        write(&base.join("skipme/a.txt"), 100);
        write(&base.join("id_rsa"), 100);
        write(&base.join("app.exe"), 100);
        let roots = vec![Root { path: base.to_path_buf(), exclude: vec!["skipme".into()] }];
        let custom = Custom::default();
        let found = crawl(&roots, &custom, &AtomicBool::new(false));
        let mut names: Vec<(String, FileKind)> = found
            .files
            .iter()
            .map(|f| (f.path.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/"), f.kind))
            .collect();
        names.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(names, vec![
            ("notes.txt".to_string(), FileKind::Doc),
            ("project/docs/Screenshot 1.png".to_string(), FileKind::Screenshot),
            ("trip/beach.jpg".to_string(), FileKind::Image),
        ]);
        // The watcher's single-file check agrees with the walk.
        assert!(see(&base.join("trip/beach.jpg"), &roots, &custom).is_some());
        assert!(see(&base.join("project/README.md"), &roots, &custom).is_none());
        assert!(see(&base.join("project/docs/Screenshot 1.png"), &roots, &custom).is_some());
        assert!(see(&base.join("skipme/a.txt"), &roots, &custom).is_none());
        assert!(see(&base.join("node_modules/readme.md"), &roots, &custom).is_none());
    }
}
