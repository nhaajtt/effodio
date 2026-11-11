# Glint

**Describe it. Find it.** Glint searches the files on your computer by what's in them: a photo by what it shows, a
PDF by what a page says, a recording or a video by the moment. Everything runs on your computer.

It's for Windows 10 and 11 (64-bit). A Mac version is planned.

```
"a dog on the beach"                  the photo
"zebra in a video"                    the clip, at the second the zebra shows up
"the clause about pets in the lease"  the PDF, at page 3
"payment declined error"              the screenshot, by the text in it
"hoa don tien dien"                   "Hóa đơn tiền điện tháng 10.txt" (accents are optional)
"PAY-7731-DX"                         an exact code, wherever it's written, pictures included
```

## What it does

- **One search for every kind of file.** Pictures, screenshots, PDFs (scans too), Word, Excel, PowerPoint and
  OpenDocument files, text and Markdown, e-books, audio and video. The model puts text, pictures and sound in one
  space, in more than 100 languages, so a search in English can find a note written in Vietnamese.
- **To the page, the passage, the moment.** Long files are read in pieces; a result opens where it matched. A strip
  under a recording or a video shows every place the search matches.
- **Exact words count too.** Names, numbers and codes are matched as written, in file names, document text, and the
  text Windows reads in pictures and scans.
- **Search by example.** Drop or paste a picture, or press Tab on a result, to find what's like it.
- **Filters you type.** `kind:video`, `in:Downloads`, `ext:pdf`, `after:2026-10`, `after:30d`, `"exact words"`, and
  `-word` to steer away from something.
- **Other ways in.** A map of everything by meaning, a list of duplicates and near-duplicates, and saved searches.
- **A local search door for your own tools**, off unless you open it (see below).

## Install

```powershell
powershell -c "irm https://glint.nhaajt.com/install.ps1 | iex"
```

That downloads the installer from [glint.nhaajt.com](https://glint.nhaajt.com), checks its SHA-256 and installs it for
the current user. Run it again to update. The installer can also be downloaded from the site and run by hand.

Press **Ctrl+Shift+Space** in any app for the search panel.

| Key | What it does |
|---|---|
| ↓ ↑ | Move through the results |
| Enter | Open the result: a PDF at its page, a recording or video at its moment |
| Ctrl+Enter | Show it in Explorer |
| Space | A large preview (once you've moved to a result) |
| Tab | More like this |
| Ctrl+C | Copy the file |
| Ctrl+O | The same search in the window |
| Esc | Clear, then close |

## Private

Nothing you index leaves your computer. Glint goes online once, to download the model from Hugging Face. There's
no account and no telemetry. It only reads your files; it never changes, moves or uploads them.

It never reads keys, certificates or password files, hidden and system folders, apps or caches. Inside a code project
it reads only screenshots. It skips folders that look like datasets, and files that only live in the cloud (OneDrive's
online-only files) are left there.

## How it works

```
your folders ─► readers turn files into pictures, text and sound
             ─► EmbeddingGemma 2 on llama.cpp, in a helper process that quits when it's done
             ─► one SQLite file: a vector for every picture, page, passage, video frame and 30 s of sound,
                plus a keyword index of names and text
your words   ─► the model's text part (while you search) ─► nearest vectors + exact words
```

Every piece of a file gets a point with its place in the file. A search scores every point against your words and
puts each on a scale of how far it stands out from the others of its kind, so a photo and a PDF page can be ranked
together; words found in the file add to that. Results that trail far behind the best are folded under "weaker
matches", and a search for something that isn't there says so.

Files are read by Windows itself where it has a reader: its OCR for text in pictures, its PDF renderer for scanned
pages, its codecs for HEIC and RAW. Audio and video go through FFmpeg, running as a separate process.

The index remembers which model and vector version made it, and never mixes them. A test checks that this build's
vectors match stored reference vectors (cosine ≥ 0.999) before anything else is trusted.

## Build

You'll need Rust, Node 20 or later, Python 3, Git and Visual Studio's C++ build tools.

```bat
git clone --depth 1 --branch b11461 https://github.com/ggml-org/llama.cpp vendor\llama.cpp
python -m venv vendor\tools-venv && vendor\tools-venv\Scripts\pip install cmake ninja
scripts\build-llama.cmd cpu
cargo test --release
pwsh scripts\package.ps1
```

The installer lands in `target\release\bundle\nsis`. It installs for the current user, with no administrator
rights. It isn't signed, so Windows asks before running it: More info, then Run anyway.

To work on it: `cargo build -p glint-helper`, then `cd app && npx tauri dev`.

**FFmpeg.** Recordings and videos need `ffmpeg` and `ffprobe` on the PATH or beside `glint.exe`
(`winget install Gyan.FFmpeg`). Without them everything else still works, and Settings says so.

**Graphics card.** The app runs the model on the processor unless a GPU build of llama.cpp is beside it and the speed
check finds it faster with the same vectors. To build one, install the Vulkan SDK (or run
`scripts\build-vulkan-deps.cmd`, which builds what's needed from source), then `scripts\build-llama.cmd vulkan` and
`pwsh scripts\package.ps1 -Flavor vulkan`.

## The local search door

Off by default. Switched on in Settings, it lets other programs on the same computer search the index:

```sh
curl -H "Authorization: Bearer <token>" "http://127.0.0.1:47600/search?q=dog+on+the+beach&limit=5"
```

After that: only this computer can reach it (it listens on 127.0.0.1, never on the network); every request needs the
token from Settings; it answers `GET /search` and `GET /status` and nothing else; it can't change or read your
files, only return what a search returns (paths, excerpts, places).

## Checking it

```sh
cargo test --release                     # the engine, and the vectors against their reference
python scripts/eval.py evals/testbed.json   # how often the right file comes first
python scripts/smoke.py                  # everything the app asks of the helper
```

`GLINT_HOME` points all three (and the app) at another data folder, so a test index never touches yours.

## Where things are

| | |
|---|---|
| `crates/embed` | The model on llama.cpp: text, pictures and sound into vectors |
| `crates/core` | Rules, readers, the index, the search |
| `crates/helper` | The helper process: indexing, searching, the speed check, the model download |
| `app/src-tauri` | The app: tray, shortcut, windows, the folder watcher, the local door |
| `app/src` | The interface |
| `site` | The website and the install script (`pwsh scripts\site.ps1 -Deploy` publishes them with the installer) |

Your data is in `%LOCALAPPDATA%\app.glint.search`: the model, the index, the settings and a cache of thumbnails.
Uninstalling with "Delete the application data" ticked removes it.

## Thanks

Glint learned from [DigUp](https://github.com/ARahim3/DigUp) by Abdur Rahim (MIT), which showed how well
EmbeddingGemma 2 searches a computer: the way pictures are sized for the model, the pinned llama.cpp build and its
one-line patch, and the ranking that puts meaning and words on one scale started there.

It's built on [EmbeddingGemma 2](https://huggingface.co/google/embeddinggemma-2) by Google DeepMind,
[llama.cpp](https://github.com/ggml-org/llama.cpp) (MIT), [Tauri](https://tauri.app) (MIT or Apache 2.0) and
[SQLite](https://sqlite.org). Their notices are in [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

Glint isn't affiliated with Google or Microsoft.

## License

MIT
