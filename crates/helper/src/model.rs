//! Downloads the model files, once: each to a `.part` file that a later run continues, checked against its SHA-256
//! before it gets its final name. Being there under its final name means it's whole.

use crate::emit;
use anyhow::{bail, Context, Result};
use magpie_embed::files;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

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

pub fn download(folder: &Path) -> Result<()> {
    std::fs::create_dir_all(folder)?;
    let total: u64 = files::ALL.iter().map(|file| file.bytes).sum();
    let mut before = 0u64;
    for file in files::ALL {
        let target = folder.join(file.name);
        if target.exists() {
            before += file.bytes;
            continue;
        }
        let part = folder.join(format!("{}.part", file.name));
        let mut have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        if have > file.bytes {
            std::fs::remove_file(&part).ok();
            have = 0;
        }
        if have < file.bytes {
            let request = ureq::get(&file.url()).set("User-Agent", "magpie").set("Range", &format!("bytes={have}-"));
            let response = request.call().with_context(|| format!("can't download {}", file.name))?;
            // A server that ignores the range sends the whole file again.
            let resumed = response.status() == 206;
            let mut output = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&part)?;
            if resumed {
                output.seek(SeekFrom::End(0))?;
            } else {
                output.set_len(0)?;
                have = 0;
            }
            let mut reader = response.into_reader();
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
                    emit(&json!({"event": "progress", "file": file.name, "done": before + have, "total": total}));
                }
            }
            output.flush()?;
        }
        if have != file.bytes {
            bail!("{} stopped at {have} of {} bytes: run it again to go on", file.name, file.bytes);
        }
        emit(&json!({"event": "checking", "file": file.name}));
        if sha256(&part)? != file.sha256 {
            std::fs::remove_file(&part).ok();
            bail!("{} didn't arrive whole (its checksum is wrong): it will be downloaded again", file.name);
        }
        std::fs::rename(&part, &target)?;
        before += file.bytes;
        emit(&json!({"event": "progress", "file": file.name, "done": before, "total": total}));
    }
    // Videos and recordings need FFmpeg: fetched now if this computer has none.
    emit(&json!({"event": "checking", "file": "FFmpeg"}));
    if let Err(error) = crate::kits::get_media() {
        emit(&json!({"event": "note", "note": format!("FFmpeg: {error:#}")}));
    }
    emit(&json!({"event": "finished", "total": total}));
    Ok(())
}
