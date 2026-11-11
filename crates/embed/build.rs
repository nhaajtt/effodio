use std::{env, fs, path::PathBuf};

/// Compiles the shim against llama.cpp's headers and links its import libraries. The DLLs are copied next to the
/// built executables so `cargo run` and `cargo test` find them.
fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let llama = env::var("MAGPIE_LLAMA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| manifest.join("../../vendor/llama-cpu"));
    let llama = llama.canonicalize().unwrap_or_else(|_| {
        panic!("llama.cpp isn't built at {}: run scripts\\build-llama.cmd", llama.display())
    });
    // cl.exe doesn't take the \\?\ form canonicalize gives on Windows.
    let llama = PathBuf::from(llama.to_string_lossy().trim_start_matches(r"\\?\"));

    println!("cargo:rerun-if-changed=shim/shim.cpp");
    println!("cargo:rerun-if-env-changed=MAGPIE_LLAMA_DIR");

    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .file("shim/shim.cpp")
        .include(llama.join("include"))
        .warnings(false)
        .compile("magpie_shim");

    println!("cargo:rustc-link-search=native={}", llama.join("lib").display());
    for library in ["llama", "mtmd", "ggml", "ggml-base"] {
        println!("cargo:rustc-link-lib=dylib={library}");
    }

    // OUT_DIR is target/<profile>/build/<crate>-<hash>/out
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let profile = out.ancestors().nth(3).unwrap().to_path_buf();
    for folder in [profile.clone(), profile.join("deps")] {
        let _ = fs::create_dir_all(&folder);
        if let Ok(entries) = fs::read_dir(llama.join("bin")) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "dll") {
                    let target = folder.join(path.file_name().unwrap());
                    let stale = match (fs::metadata(&path), fs::metadata(&target)) {
                        (Ok(a), Ok(b)) => a.len() != b.len() || a.modified().ok() > b.modified().ok(),
                        _ => true,
                    };
                    if stale {
                        let _ = fs::copy(&path, &target);
                    }
                }
            }
        }
    }
    println!("cargo:bin-dir={}", llama.join("bin").display());
}
