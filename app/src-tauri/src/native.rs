//! What Windows does for the app: thumbnails of any file (Explorer's own), opening a result at its page or moment,
//! showing it in Explorer, putting it on the clipboard, and ending the helpers with the app.

use anyhow::{anyhow, bail, Result};
use std::path::{Path, PathBuf};
use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{HANDLE, HWND, SIZE};
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS, HBITMAP, HGDIOBJ,
};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE, GMEM_ZEROINIT};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::UI::Shell::{
    AssocQueryStringW, IShellItemImageFactory, SHCreateItemFromParsingName, ShellExecuteW, ASSOCF_NONE,
    ASSOCSTR_EXECUTABLE, DROPFILES, SIIGBF_BIGGERSIZEOK, SIIGBF_THUMBNAILONLY, SIIGBF_ICONONLY,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

thread_local! {
    static COM: bool = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok() };
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Explorer's thumbnail of a file (its picture, a video's frame, a PDF's first page), or its icon when it has none:
/// RGBA pixels with their width and height.
fn shell_image(path: &Path, side: i32, icon: bool) -> Result<(u32, u32, Vec<u8>)> {
    COM.with(|_| ());
    unsafe {
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()), None)?;
        let flags = if icon { SIIGBF_ICONONLY } else { SIIGBF_THUMBNAILONLY | SIIGBF_BIGGERSIZEOK };
        let bitmap: HBITMAP = factory.GetImage(SIZE { cx: side, cy: side }, flags)?;
        let result = bitmap_pixels(bitmap);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        result
    }
}

unsafe fn bitmap_pixels(bitmap: HBITMAP) -> Result<(u32, u32, Vec<u8>)> {
    let mut info = BITMAP::default();
    if GetObjectW(HGDIOBJ(bitmap.0), std::mem::size_of::<BITMAP>() as i32, Some(&mut info as *mut _ as *mut _)) == 0 {
        bail!("can't read the thumbnail");
    }
    let (width, height) = (info.bmWidth, info.bmHeight.abs());
    if width <= 0 || height <= 0 {
        bail!("an empty thumbnail");
    }
    let mut header = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    let screen = GetDC(HWND::default());
    let lines = GetDIBits(screen, bitmap, 0, height as u32, Some(pixels.as_mut_ptr() as *mut _), &mut header, DIB_RGB_COLORS);
    ReleaseDC(HWND::default(), screen);
    if lines == 0 {
        bail!("can't copy the thumbnail");
    }
    // BGRA, premultiplied when it has alpha; some thumbnails come with the alpha channel all zero.
    let opaque = pixels.chunks_exact(4).all(|pixel| pixel[3] == 0);
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        if opaque {
            pixel[3] = 255;
        } else if pixel[3] > 0 && pixel[3] < 255 {
            for channel in 0..3 {
                pixel[channel] = ((pixel[channel] as u32 * 255) / pixel[3] as u32).min(255) as u8;
            }
        }
    }
    Ok((width as u32, height as u32, pixels))
}

/// A thumbnail file for `path` in `cache`, made once per version of the file. Returns the PNG's path.
pub fn thumbnail(path: &Path, side: u32, cache: &Path) -> Result<PathBuf> {
    let metadata = std::fs::metadata(path)?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let key = blake3::hash(format!("{}|{}|{}|{side}", path.display(), metadata.len(), modified).as_bytes());
    let target = cache.join(format!("{}.png", &key.to_hex()[..24]));
    if target.exists() {
        return Ok(target);
    }
    std::fs::create_dir_all(cache)?;
    let (width, height, rgba) =
        shell_image(path, side as i32, false).or_else(|_| shell_image(path, side.min(256) as i32, true))?;
    let image = image::RgbaImage::from_raw(width, height, rgba).ok_or_else(|| anyhow!("a broken thumbnail"))?;
    let temporary = target.with_extension("tmp");
    image.save_with_format(&temporary, image::ImageFormat::Png)?;
    std::fs::rename(&temporary, &target)?;
    Ok(target)
}

/// The program that opens files with this extension (".pdf"), by its executable's path.
pub fn default_program(extension: &str) -> Option<PathBuf> {
    unsafe {
        let extension = wide(extension);
        let mut length = 0u32;
        let _ = AssocQueryStringW(ASSOCF_NONE, ASSOCSTR_EXECUTABLE, PCWSTR(extension.as_ptr()), None, PWSTR::null(), &mut length);
        if length == 0 {
            return None;
        }
        let mut buffer = vec![0u16; length as usize];
        AssocQueryStringW(ASSOCF_NONE, ASSOCSTR_EXECUTABLE, PCWSTR(extension.as_ptr()), None, PWSTR(buffer.as_mut_ptr()), &mut length)
            .ok()
            .ok()?;
        let end = buffer.iter().position(|&unit| unit == 0).unwrap_or(buffer.len());
        Some(PathBuf::from(String::from_utf16_lossy(&buffer[..end])))
    }
}

fn shell_open(target: &str, parameters: Option<&str>) -> Result<()> {
    let target = wide(target);
    let parameters = parameters.map(wide);
    let result = unsafe {
        ShellExecuteW(
            HWND::default(),
            windows::core::w!("open"),
            PCWSTR(target.as_ptr()),
            parameters.as_ref().map_or(PCWSTR::null(), |p| PCWSTR(p.as_ptr())),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if result.0 as usize <= 32 {
        bail!("Windows couldn't open it");
    }
    Ok(())
}

fn quoted(path: &Path) -> String {
    format!("\"{}\"", path.display())
}

fn file_url(path: &Path) -> String {
    let mut url = String::from("file:///");
    for byte in path.to_string_lossy().replace('\\', "/").bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b':' | b'-' | b'_' | b'.' | b'~' => url.push(byte as char),
            other => url.push_str(&format!("%{other:02X}")),
        }
    }
    url
}

/// Opens a file in its own program: a PDF at `page`, a recording or a video at `second`, when the program is one
/// that can be told (the rest open the file from its start).
pub fn open(path: &Path, page: Option<u32>, second: Option<f64>) -> Result<()> {
    if !path.exists() {
        bail!("the file isn't there any more");
    }
    let extension = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let program = default_program(&extension);
    let name = program
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if let (Some(page), Some(program)) = (page.filter(|&page| page > 1), program.as_ref()) {
        let program_text = program.to_string_lossy();
        let arguments = match name.as_str() {
            "msedge.exe" | "chrome.exe" | "firefox.exe" | "brave.exe" | "vivaldi.exe" | "opera.exe" => {
                Some(format!("\"{}#page={page}\"", file_url(path)))
            }
            "acrobat.exe" | "acrord32.exe" => Some(format!("/A \"page={page}\" {}", quoted(path))),
            "sumatrapdf.exe" => Some(format!("-page {page} {}", quoted(path))),
            "foxitpdfreader.exe" | "foxitreader.exe" => Some(format!("{} /A page={page}", quoted(path))),
            "pdfxcview.exe" | "pdfxedit.exe" => Some(format!("/A \"page={page}\" {}", quoted(path))),
            _ => None,
        };
        if let Some(arguments) = arguments {
            return shell_open(&program_text, Some(&arguments));
        }
    }
    if let (Some(second), Some(program)) = (second.filter(|&second| second >= 1.0), program.as_ref()) {
        let program_text = program.to_string_lossy();
        let whole = second.floor() as u64;
        let arguments = match name.as_str() {
            "vlc.exe" => Some(format!("--start-time={whole} {}", quoted(path))),
            "mpv.exe" | "mpvnet.exe" => Some(format!("--start={whole} {}", quoted(path))),
            "mpc-hc64.exe" | "mpc-hc.exe" | "mpc-be64.exe" | "mpc-be.exe" => {
                Some(format!("{} /start {}", quoted(path), whole * 1000))
            }
            "potplayermini64.exe" | "potplayermini.exe" => Some(format!("{} /seek={whole}", quoted(path))),
            _ => None,
        };
        if let Some(arguments) = arguments {
            return shell_open(&program_text, Some(&arguments));
        }
    }
    shell_open(&path.to_string_lossy(), None)
}

/// Shows the file in Explorer, selected.
pub fn reveal(path: &Path) -> Result<()> {
    shell_open("explorer.exe", Some(&format!("/select,{}", quoted(path))))
}

/// Puts files on the clipboard, to paste into a folder, a chat or a mail.
pub fn copy_files(paths: &[PathBuf]) -> Result<()> {
    let mut list: Vec<u16> = Vec::new();
    for path in paths {
        list.extend(path.as_os_str().to_string_lossy().encode_utf16());
        list.push(0);
    }
    list.push(0);
    let header = std::mem::size_of::<DROPFILES>();
    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, header + list.len() * 2)?;
        let pointer = GlobalLock(memory) as *mut u8;
        if pointer.is_null() {
            bail!("no memory for the clipboard");
        }
        let drop = pointer as *mut DROPFILES;
        (*drop).pFiles = header as u32;
        (*drop).fWide = true.into();
        std::ptr::copy_nonoverlapping(list.as_ptr() as *const u8, pointer.add(header), list.len() * 2);
        let _ = GlobalUnlock(memory);
        OpenClipboard(HWND::default())?;
        let _ = EmptyClipboard();
        const CF_HDROP: u32 = 15;
        let set = SetClipboardData(CF_HDROP, HANDLE(memory.0));
        let _ = CloseClipboard();
        set?;
    }
    Ok(())
}

/// A job object that ends every process in it when the app's handle to it closes, however the app ends.
pub struct Job(HANDLE);

unsafe impl Send for Job {}
unsafe impl Sync for Job {}

impl Job {
    pub fn new() -> Option<Job> {
        unsafe {
            let job = CreateJobObjectW(None, PCWSTR::null()).ok()?;
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .ok()?;
            Some(Job(job))
        }
    }

    pub fn add(&self, child: &std::process::Child) {
        use std::os::windows::io::AsRawHandle;
        unsafe {
            let _ = AssignProcessToJobObject(self.0, HANDLE(child.as_raw_handle() as _));
        }
    }
}

/// Whether the computer runs on mains power (true when Windows can't tell).
pub fn on_power() -> bool {
    let mut status = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut status).is_err() || status.ACLineStatus != 0 }
}
