# Working on Effodio

The build itself is in the README (Rust, Node 20+, Python 3, Visual Studio's C++ tools, llama.cpp under `vendor`).

## Before a change goes in

```powershell
pwsh scripts\check.ps1
```

It runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --release`, the interface's
type check and its build. Everything must pass. For a change to how files are read or ranked, also run
`scripts/eval.py` on an index of your test files (with `EFFODIO_HOME` pointing at a folder of its own) before and
after: the share of searches whose file comes first mustn't drop.

## Where to change what

- **A new kind of file**: its extension in `crates/core/src/kinds.rs`, a reader in `crates/core/src/readers/`, and a
  branch in `Indexer::read` (`crates/core/src/indexer.rs`). The search needs nothing new: every reader ends in
  segments with a place in the file.
- **A new thing the interface asks of the helper**: an op in `crates/helper/src/serve.rs`, a wrapper in
  `app/src/api.ts`. If it takes a path, `commands::ask` checks it (`app/src-tauri/src/paths.rs`).
- **A new command of the app itself**: `app/src-tauri/src/commands.rs`, listed in `generate_handler!` in `main.rs`.
  A path from the page goes through `permitted` before anything touches it.
- **Words in the interface**: both languages in `app/src/i18n.ts`.

## Ground rules

- Nothing about a person's files leaves the computer. The only network calls are the downloads a person asks for and
  the version check, all in `crates/helper/src/web.rs` and the pinned addresses in `kits.rs`, `model.rs` and the
  model crates. A new one needs a reason in the README's Privacy section.
- Readers get files nobody checked: no panics on bad input, no unbounded reads, no shelling out with a file's name
  where an option could go.
- Tests make their files in temporary folders; never point one at a real data folder.
- Rust is formatted by `rustfmt.toml` (120 columns). The TypeScript is strict (`tsconfig.json`).
- Comments say why, in the voice of the ones around them.
