//! Downloads the model files, once (see `fetch` for where from): each to a `.part` file that a later run continues,
//! checked against its SHA-256 before it gets its final name. Being there under its final name means it's whole.

use crate::emit;
use crate::fetch::{self, Step, Wanted};
use anyhow::Result;
use effodio_embed::files;
use serde_json::json;
use std::path::Path;

pub fn download(folder: &Path) -> Result<()> {
    std::fs::create_dir_all(folder)?;
    let total: u64 = files::ALL.iter().map(|file| file.bytes).sum();
    let mut before = 0u64;
    for file in files::ALL {
        let origin = file.url();
        let wanted = Wanted { name: file.name, bytes: file.bytes, sha256: file.sha256, origin: &origin };
        fetch::obtain(&wanted, &folder.join(file.name), &mut |step| match step {
            Step::Have(have) => {
                emit(&json!({"event": "progress", "file": file.name, "done": before + have, "total": total}))
            }
            Step::Checking => emit(&json!({"event": "checking", "file": file.name})),
        })?;
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
