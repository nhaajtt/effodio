//! EmbeddingGemma 2 on llama.cpp. Text, images and audio land in one 768-dimensional space; every vector is
//! mean-pooled and L2-normalized, so a dot product is a cosine.

pub mod sizing;

use anyhow::{anyhow, bail, Result};
use std::{
    ffi::{c_char, c_int, c_uint, c_void, CString},
    path::{Path, PathBuf},
};

/// The model files: ggml-org's Q8_0 conversion at one pinned revision. Never the F16 files, whose activations
/// overflow.
pub mod files {
    pub const REPOSITORY: &str = "ggml-org/embeddinggemma-2-GGUF";
    pub const REVISION: &str = "bfcd298762cc34d0357ece5ebdd31791a3a374d8";

    pub struct File {
        pub name: &'static str,
        pub bytes: u64,
        pub sha256: &'static str,
    }

    impl File {
        pub fn url(&self) -> String {
            format!("https://huggingface.co/{REPOSITORY}/resolve/{REVISION}/{}", self.name)
        }
    }

    /// The text model: all that queries need.
    pub const TEXT: File = File {
        name: "embeddinggemma-2-Q8_0.gguf",
        bytes: 309_855_456,
        sha256: "2188ac1deca4b77dffefd603c2776a9d76d9d74ec01841392982ebb840b09135",
    };
    /// The image and audio encoders.
    pub const MEDIA: File = File {
        name: "mmproj-embeddinggemma-2-Q8_0.gguf",
        bytes: 554_821_024,
        sha256: "c4a8a52691ecef40618438928bdf9e68379b854e24166f292592353db0aab64f",
    };
    pub const ALL: [&File; 2] = [&TEXT, &MEDIA];
}

/// The prompts the model was trained with.
pub mod prompts {
    pub fn query(text: &str) -> String {
        format!("task: search result | query: {text}")
    }
    pub fn code_query(text: &str) -> String {
        format!("task: code retrieval | query: {text}")
    }
    pub fn document(title: Option<&str>, text: &str) -> String {
        let title = title.filter(|t| !t.is_empty()).unwrap_or("none");
        format!("title: {title} | text: {text}")
    }
}

/// Which vectors this pipeline makes (the model files, llama.cpp, the resizing, the prompts). Indexes record it and
/// never mix versions. It changes only when the reference-vector test says the vectors did.
pub const VECTOR_VERSION: u32 = 1;
pub const DIMENSION: usize = 768;
/// Soft-token budgets an image can be embedded at.
pub const BUDGETS: [usize; 5] = [70, 140, 280, 560, 1120];
pub const AUDIO_RATE: usize = 16_000;
/// The model hears this much per input.
pub const AUDIO_SECONDS: usize = 30;

#[repr(C)]
struct Ctx {
    _private: [u8; 0],
}

extern "C" {
    fn ge_init(dir: *const c_char, cpu_only: c_int) -> c_int;
    fn ge_backends(out: *mut c_char, len: c_int);
    fn ge_open(
        text: *const c_char, media: *const c_char, max_tokens: c_int, threads: c_int, use_gpu: c_int,
        err: *mut c_char, errlen: c_int,
    ) -> *mut Ctx;
    fn ge_close(ctx: *mut Ctx);
    fn ge_dim(ctx: *const Ctx) -> c_int;
    fn ge_embed_texts(
        ctx: *mut Ctx, texts: *const *const c_char, lens: *const c_int, n: c_int, out: *mut f32,
        err: *mut c_char, errlen: c_int,
    ) -> c_int;
    fn ge_embed_image(
        ctx: *mut Ctx, rgb: *const u8, width: c_int, height: c_int, out: *mut f32, err: *mut c_char, errlen: c_int,
    ) -> c_int;
    fn ge_embed_audio(
        ctx: *mut Ctx, samples: *const f32, count: usize, out: *mut f32, err: *mut c_char, errlen: c_int,
    ) -> c_int;
    fn ge_decode(
        ctx: *mut Ctx, bytes: *const u8, len: usize, is_audio: *mut c_int, width: *mut c_uint, height: *mut c_uint,
        data: *mut *const u8, data_len: *mut usize,
    ) -> *mut c_void;
    fn ge_decoded_free(handle: *mut c_void);
}

fn message(buffer: &[u8]) -> String {
    let end = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
    String::from_utf8_lossy(&buffer[..end]).into_owned()
}

/// Where the processor or the graphics card does the work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    Cpu,
    Gpu,
}

#[derive(Clone, Debug)]
pub struct Options {
    pub text: PathBuf,
    /// None: text only (queries), about a quarter of the memory.
    pub media: Option<PathBuf>,
    pub device: Device,
    /// Where ggml's backend DLLs are; None: beside this executable.
    pub backends: Option<PathBuf>,
    pub threads: Option<usize>,
}

impl Options {
    /// Both files in `folder`, or just the text part when `text_only`.
    pub fn in_folder(folder: &Path, text_only: bool, device: Device) -> Options {
        Options {
            text: folder.join(files::TEXT.name),
            media: (!text_only).then(|| folder.join(files::MEDIA.name)),
            device,
            backends: None,
            threads: None,
        }
    }
}

/// What a decoded file turned out to be (llama.cpp's own readers: stb_image, miniaudio).
pub enum Decoded {
    Image { width: usize, height: usize, rgb: Vec<u8> },
    Audio { samples: Vec<f32> },
}

pub struct Embedder {
    ctx: *mut Ctx,
    dim: usize,
    pub text_only: bool,
    pub gpu: bool,
}

// One thread at a time uses it, but it may move between threads.
unsafe impl Send for Embedder {}

impl Embedder {
    /// Every logical processor up to 8: more makes the slow cores of a hybrid processor hold the fast ones back.
    pub fn default_threads() -> usize {
        std::env::var("EFFODIO_THREADS").ok().and_then(|v| v.parse().ok()).unwrap_or_else(|| {
            std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8)
        })
    }

    /// Loads ggml's backends once for the process. Returns whether a GPU backend found a device.
    pub fn init_backends(folder: Option<&Path>, cpu_only: bool) -> bool {
        let folder = folder.map(Path::to_path_buf).or_else(|| {
            std::env::var_os("EFFODIO_BACKENDS").map(PathBuf::from).or_else(|| {
                std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf))
            })
        });
        let dir = CString::new(folder.map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()).unwrap();
        unsafe { ge_init(dir.as_ptr(), cpu_only as c_int) != 0 }
    }

    pub fn backends() -> String {
        let mut buffer = [0u8; 512];
        unsafe { ge_backends(buffer.as_mut_ptr() as *mut c_char, buffer.len() as c_int) };
        message(&buffer)
    }

    pub fn open(options: &Options) -> Result<Embedder> {
        let cpu_only = options.device == Device::Cpu;
        let gpu_found = Self::init_backends(options.backends.as_deref(), cpu_only);
        if !options.text.exists() {
            bail!("the model isn't downloaded yet: no {}", options.text.display());
        }
        if let Some(media) = &options.media {
            if !media.exists() {
                bail!("the image and audio encoders aren't downloaded yet: no {}", media.display());
            }
        }
        let text = CString::new(options.text.to_string_lossy().as_bytes())?;
        let media = match &options.media {
            Some(path) => Some(CString::new(path.to_string_lossy().as_bytes())?),
            None => None,
        };
        // One input must fit one micro-batch: 2048 tokens (the reference's cut), which also holds a 1120-token image
        // or a 30 s clip. Queries are short, so the query encoder reserves less.
        let max_tokens = if media.is_some() { 2048 } else { 512 };
        let threads = options.threads.unwrap_or_else(Self::default_threads);
        let use_gpu = !cpu_only && gpu_found;
        let mut err = [0u8; 512];
        let ctx = unsafe {
            ge_open(
                text.as_ptr(),
                media.as_ref().map_or(std::ptr::null(), |m| m.as_ptr()),
                max_tokens,
                threads as c_int,
                use_gpu as c_int,
                err.as_mut_ptr() as *mut c_char,
                err.len() as c_int,
            )
        };
        if ctx.is_null() {
            bail!("{}", message(&err));
        }
        let dim = unsafe { ge_dim(ctx) } as usize;
        let mut embedder = Embedder { ctx, dim, text_only: media.is_none(), gpu: use_gpu };
        // The first calls set the backend up: pay that now.
        embedder.embed_texts(&[prompts::query("warm up")])?;
        Ok(embedder)
    }

    pub fn dim(&self) -> usize {
        self.dim
    }

    /// One vector per text, in order. Texts go in as they are: add a prompt from `prompts` first.
    pub fn embed_texts<S: AsRef<str>>(&mut self, texts: &[S]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let pointers: Vec<*const c_char> = texts.iter().map(|t| t.as_ref().as_ptr() as *const c_char).collect();
        let lengths: Vec<c_int> = texts.iter().map(|t| t.as_ref().len() as c_int).collect();
        let mut out = vec![0f32; texts.len() * self.dim];
        let mut err = [0u8; 512];
        let status = unsafe {
            ge_embed_texts(
                self.ctx, pointers.as_ptr(), lengths.as_ptr(), texts.len() as c_int, out.as_mut_ptr(),
                err.as_mut_ptr() as *mut c_char, err.len() as c_int,
            )
        };
        if status != 0 {
            bail!("{}", message(&err));
        }
        Ok(out.chunks(self.dim).map(<[f32]>::to_vec).collect())
    }

    pub fn embed_text(&mut self, text: &str) -> Result<Vec<f32>> {
        Ok(self.embed_texts(&[text])?.remove(0))
    }

    /// Packed RGB of any size: resized here to fill `budget` soft tokens, as the reference does.
    pub fn embed_image(&mut self, rgb: &[u8], width: usize, height: usize, budget: usize) -> Result<Vec<f32>> {
        if !BUDGETS.contains(&budget) {
            bail!("budget must be one of {BUDGETS:?}");
        }
        if rgb.len() < width * height * 3 {
            bail!("the pixels don't fill {width}x{height}");
        }
        let (target_height, target_width) =
            sizing::target_size(height, width, budget).ok_or_else(|| anyhow!("image too thin to embed"))?;
        let resized;
        let pixels = if (target_height, target_width) == (height, width) {
            rgb
        } else {
            resized = sizing::resize_bicubic(rgb, width, height, target_width, target_height);
            &resized
        };
        let mut out = vec![0f32; self.dim];
        let mut err = [0u8; 512];
        let status = unsafe {
            ge_embed_image(
                self.ctx, pixels.as_ptr(), target_width as c_int, target_height as c_int, out.as_mut_ptr(),
                err.as_mut_ptr() as *mut c_char, err.len() as c_int,
            )
        };
        if status != 0 {
            bail!("{}", message(&err));
        }
        Ok(out)
    }

    /// Mono samples at 16 kHz; anything past 30 s is dropped.
    pub fn embed_audio(&mut self, samples: &[f32]) -> Result<Vec<f32>> {
        if samples.is_empty() {
            bail!("no sound to embed");
        }
        let mut out = vec![0f32; self.dim];
        let mut err = [0u8; 512];
        let status = unsafe {
            ge_embed_audio(
                self.ctx, samples.as_ptr(), samples.len(), out.as_mut_ptr(), err.as_mut_ptr() as *mut c_char,
                err.len() as c_int,
            )
        };
        if status != 0 {
            bail!("{}", message(&err));
        }
        Ok(out)
    }

    /// Reads an image (JPEG, PNG, BMP, GIF) or audio file (WAV, MP3, FLAC) held in memory with llama.cpp's readers.
    pub fn decode(&mut self, bytes: &[u8]) -> Result<Decoded> {
        let (mut is_audio, mut width, mut height) = (0 as c_int, 0 as c_uint, 0 as c_uint);
        let (mut data, mut len) = (std::ptr::null::<u8>(), 0usize);
        let handle = unsafe {
            ge_decode(self.ctx, bytes.as_ptr(), bytes.len(), &mut is_audio, &mut width, &mut height, &mut data, &mut len)
        };
        if handle.is_null() {
            bail!("can't read the file as an image or as audio");
        }
        let raw = unsafe { std::slice::from_raw_parts(data, len) };
        let decoded = if is_audio != 0 {
            let samples = raw.chunks_exact(4).map(|c| f32::from_ne_bytes([c[0], c[1], c[2], c[3]])).collect();
            Decoded::Audio { samples }
        } else {
            Decoded::Image { width: width as usize, height: height as usize, rgb: raw.to_vec() }
        };
        unsafe { ge_decoded_free(handle) };
        Ok(decoded)
    }
}

impl Drop for Embedder {
    fn drop(&mut self) {
        unsafe { ge_close(self.ctx) };
    }
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Where the app keeps the model: `%LOCALAPPDATA%\app.effodio.search\Models`, or `$EFFODIO_MODELS`.
pub fn models_folder() -> PathBuf {
    if let Some(chosen) = std::env::var_os("EFFODIO_MODELS") {
        return PathBuf::from(chosen);
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share"));
    match std::env::var_os("EFFODIO_HOME") {
        Some(home) => PathBuf::from(home).join("Models"),
        None => base.join("app.effodio.search").join("Models"),
    }
}
