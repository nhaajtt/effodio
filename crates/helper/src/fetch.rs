//! Where a model or tool file comes from, in this order: a copy already on this computer (the full installer's
//! `models` folder beside the app, a pack someone put in `Models\Pack`, or `$EFFODIO_PACK`), then Effodio's own
//! mirror, then the place its makers publish it. Whatever the source, a file is only kept once its size and SHA-256
//! are the ones written in the app, so no source has to be trusted, and any one of them going away isn't the end.

use anyhow::{bail, Result};
use effodio_core::config;
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Effodio's own copy of every file, on its own site.
pub const MIRROR: &str = "https://effodio.nhaajt.com/models";

pub struct Wanted<'a> {
    pub name: &'a str,
    pub bytes: u64,
    pub sha256: &'a str,
    /// Where its makers publish it.
    pub origin: &'a str,
}

pub enum Step {
    /// This many of the file's bytes are here.
    Have(u64),
    /// All of it is here and its checksum is being worked out.
    Checking,
}

pub fn sha256(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 4 << 20];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

/// The folders a file may already be in, on this computer.
pub fn packs() -> Vec<PathBuf> {
    let mut folders = Vec::new();
    if let Some(chosen) = std::env::var_os("EFFODIO_PACK") {
        folders.push(PathBuf::from(chosen));
    }
    if let Some(beside) = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|folder| folder.join("models")))
    {
        folders.push(beside);
    }
    folders.push(config::models_folder().join("Pack"));
    folders
}

/// The places on the web to get a file from, the first tried first.
pub fn sources(wanted: &Wanted) -> Vec<String> {
    let mut urls = Vec::new();
    // `$EFFODIO_MIRROR` stands in for Effodio's own, for a company that keeps its own copy.
    let mirror = std::env::var("EFFODIO_MIRROR").unwrap_or_else(|_| MIRROR.to_string());
    urls.push(format!("{}/{}", mirror.trim_end_matches('/'), wanted.name));
    urls.push(wanted.origin.to_string());
    urls
}

fn partial(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    target.with_file_name(name)
}

/// Whether the part file is the file wanted; it is removed if it isn't.
fn whole(part: &Path, wanted: &Wanted, step: &mut dyn FnMut(Step)) -> Result<bool> {
    if std::fs::metadata(part).map(|m| m.len()).ok() != Some(wanted.bytes) {
        std::fs::remove_file(part).ok();
        return Ok(false);
    }
    step(Step::Checking);
    if sha256(part)? != wanted.sha256 {
        std::fs::remove_file(part).ok();
        return Ok(false);
    }
    Ok(true)
}

/// The rest of the file from `url` into the part file, continuing what is there. Returns the byte it started from.
fn download(url: &str, wanted: &Wanted, part: &Path, step: &mut dyn FnMut(Step)) -> Result<u64> {
    let mut have = std::fs::metadata(part).map(|m| m.len()).unwrap_or(0);
    if have > wanted.bytes {
        std::fs::remove_file(part).ok();
        have = 0;
    }
    let from = have;
    if have == wanted.bytes {
        return Ok(from);
    }
    let response = crate::web::get(url).set("Range", &format!("bytes={have}-")).call()?;
    // A server that ignores the range sends the whole file again.
    let resumed = response.status() == 206;
    let mut output = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(part)?;
    if resumed {
        output.seek(SeekFrom::End(0))?;
    } else {
        output.set_len(0)?;
        have = 0;
    }
    let from = have;
    // Never more than the file is known to be: a server that keeps sending can't fill the disk.
    let mut reader = response.into_reader().take(wanted.bytes - have);
    let mut buffer = vec![0u8; 1 << 20];
    let mut last = std::time::Instant::now();
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        output.write_all(&buffer[..read])?;
        have += read as u64;
        if last.elapsed().as_millis() >= 250 {
            last = std::time::Instant::now();
            step(Step::Have(have));
        }
    }
    output.flush()?;
    step(Step::Have(have));
    if have != wanted.bytes {
        bail!("stopped at {have} of {} bytes", wanted.bytes);
    }
    Ok(from)
}

/// Puts the file at `target`, from the first source that has it whole.
pub fn obtain(wanted: &Wanted, target: &Path, step: &mut dyn FnMut(Step)) -> Result<()> {
    if target.exists() {
        return Ok(());
    }
    if let Some(folder) = target.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let part = partial(target);
    for folder in packs() {
        let copy = folder.join(wanted.name);
        if std::fs::metadata(&copy).map(|m| m.len()).ok() != Some(wanted.bytes) {
            continue;
        }
        // A second name for the same bytes when it's on the same drive (the full installer's copy then takes no more
        // room), a copy when not. Either way it is what gets checked, under the name it will keep.
        std::fs::remove_file(&part).ok();
        if std::fs::hard_link(&copy, &part).is_err() {
            std::fs::copy(&copy, &part)?;
        }
        if whole(&part, wanted, step)? {
            std::fs::rename(&part, target)?;
            step(Step::Have(wanted.bytes));
            return Ok(());
        }
    }
    let mut failures = Vec::new();
    for url in sources(wanted) {
        // Twice when what was there before came out wrong: once more from the first byte.
        for _ in 0..2 {
            match download(&url, wanted, &part, step) {
                Ok(from) => {
                    if whole(&part, wanted, step)? {
                        std::fs::rename(&part, target)?;
                        return Ok(());
                    }
                    failures.push(format!("{url}: not the file it should be"));
                    if from == 0 {
                        break;
                    }
                }
                Err(error) => {
                    failures.push(format!("{url}: {error:#}"));
                    break;
                }
            }
        }
    }
    bail!("can't get {} (run it again to go on): {}", wanted.name, failures.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wanted<'a>(name: &'a str, bytes: &[u8], sha: &'a mut String) -> Wanted<'a> {
        *sha = Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect();
        Wanted { name, bytes: bytes.len() as u64, sha256: sha, origin: "http://127.0.0.1:9/nowhere" }
    }

    #[test]
    fn a_pack_on_this_computer_is_used_and_a_wrong_one_is_not() {
        let pack = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        // The only test that sets these. Nothing listens on port 9, so the web has nothing either.
        std::env::set_var("EFFODIO_PACK", pack.path());
        std::env::set_var("EFFODIO_MIRROR", "http://127.0.0.1:9");
        let mut sha = String::new();
        let good = wanted("good.bin", b"the right bytes", &mut sha);
        std::fs::write(pack.path().join("good.bin"), b"the right bytes").unwrap();
        let target = out.path().join("good.bin");
        obtain(&good, &target, &mut |_| {}).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"the right bytes");

        let mut sha = String::new();
        let bad = wanted("bad.bin", b"the right bytes", &mut sha);
        std::fs::write(pack.path().join("bad.bin"), b"the wrong bytes").unwrap();
        let target = out.path().join("bad.bin");
        // Same size, other bytes: refused, and with nowhere else to get it from, an error.
        assert!(obtain(&bad, &target, &mut |_| {}).is_err());
        assert!(!target.exists() && !partial(&target).exists());
    }
}
