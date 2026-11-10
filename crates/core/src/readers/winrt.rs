//! What Windows itself reads: text in pictures (its OCR), PDF pages as pictures (its PDF reader), and picture
//! formats it has codecs for.

use super::Picture;
use anyhow::{anyhow, bail, Result};
use std::path::Path;
use windows::core::HSTRING;
use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
use windows::Graphics::Imaging::{
    BitmapAlphaMode, BitmapDecoder, BitmapPixelFormat, BitmapTransform, ColorManagementMode, ExifOrientationMode,
    SoftwareBitmap,
};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::{DataWriter, IRandomAccessStream, InMemoryRandomAccessStream};
use windows::Storage::{FileAccessMode, StorageFile};
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

thread_local! {
    // Joining the multithreaded apartment; a thread that's already in one stays as it is.
    static APARTMENT: bool = unsafe { RoInitialize(RO_INIT_MULTITHREADED).is_ok() };
    static ENGINE: Option<OcrEngine> = OcrEngine::TryCreateFromUserProfileLanguages().ok();
}

fn ready() {
    APARTMENT.with(|_| ());
}

fn storage_file(path: &Path) -> Result<StorageFile> {
    ready();
    // The storage API takes backslashes only, and no \\?\ prefix.
    let text = path.to_string_lossy().replace('/', "\\");
    let text = text.trim_start_matches(r"\\?\");
    Ok(StorageFile::GetFileFromPathAsync(&HSTRING::from(text))?.get()?)
}

/// The pixels of a decoder's first frame, no larger than `side` on its longer side, turned as its EXIF says.
fn pixels(decoder: &BitmapDecoder, side: u32) -> Result<Picture> {
    let (width, height) = (decoder.OrientedPixelWidth()?, decoder.OrientedPixelHeight()?);
    if width == 0 || height == 0 {
        bail!("an empty picture");
    }
    let longest = width.max(height);
    let scale = if longest > side { side as f64 / longest as f64 } else { 1.0 };
    let out_width = ((width as f64 * scale).round() as u32).max(1);
    let out_height = ((height as f64 * scale).round() as u32).max(1);
    let transform = BitmapTransform::new()?;
    // The scale applies before the rotation, so it's given in the stored orientation.
    let swapped = decoder.PixelWidth()? != width;
    transform.SetScaledWidth(if swapped { out_height } else { out_width })?;
    transform.SetScaledHeight(if swapped { out_width } else { out_height })?;
    let data = decoder
        .GetPixelDataTransformedAsync(
            BitmapPixelFormat::Bgra8,
            BitmapAlphaMode::Premultiplied,
            &transform,
            ExifOrientationMode::RespectExifOrientation,
            ColorManagementMode::ColorManageToSRgb,
        )?
        .get()?;
    let bgra = data.DetachPixelData()?;
    let bgra = bgra.as_slice();
    let (out_width, out_height) = (out_width as usize, out_height as usize);
    if bgra.len() < out_width * out_height * 4 {
        bail!("the picture came back short");
    }
    let mut rgb = Vec::with_capacity(out_width * out_height * 3);
    for pixel in bgra.chunks_exact(4).take(out_width * out_height) {
        // Premultiplied over white.
        let rest = 255 - pixel[3] as u16;
        rgb.push((pixel[2] as u16 + rest).min(255) as u8);
        rgb.push((pixel[1] as u16 + rest).min(255) as u8);
        rgb.push((pixel[0] as u16 + rest).min(255) as u8);
    }
    Ok(Picture { width: out_width, height: out_height, rgb })
}

/// A picture in any format Windows has a codec for (HEIC and AVIF with their Store extensions, camera RAW…).
pub fn decode(path: &Path, side: u32) -> Result<Picture> {
    let file = storage_file(path)?;
    let stream = file.OpenAsync(FileAccessMode::Read)?.get()?;
    let decoder = BitmapDecoder::CreateAsync(&stream)?.get()?;
    pixels(&decoder, side)
}

/// The languages Windows can read text in on this computer.
pub fn ocr_languages() -> Vec<String> {
    ready();
    OcrEngine::AvailableRecognizerLanguages()
        .map(|languages| languages.into_iter().filter_map(|l| l.LanguageTag().ok()).map(|t| t.to_string()).collect())
        .unwrap_or_default()
}

/// The text Windows reads in a picture, line by line. Empty when it sees none.
pub fn ocr(picture: &Picture) -> Result<String> {
    ready();
    let max = (OcrEngine::MaxImageDimension().unwrap_or(10_000) as usize).min(4000);
    let fitted;
    let picture = if picture.width.max(picture.height) > max {
        fitted = picture.fit(max);
        &fitted
    } else {
        picture
    };
    if picture.width < 40 || picture.height < 40 {
        return Ok(String::new());
    }
    let mut bgra = Vec::with_capacity(picture.width * picture.height * 4);
    for pixel in picture.rgb.chunks_exact(3) {
        bgra.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
    }
    let writer = DataWriter::new()?;
    writer.WriteBytes(&bgra)?;
    let buffer = writer.DetachBuffer()?;
    let bitmap = SoftwareBitmap::CreateCopyFromBuffer(
        &buffer, BitmapPixelFormat::Bgra8, picture.width as i32, picture.height as i32,
    )?;
    ENGINE.with(|engine| {
        let engine = engine.as_ref().ok_or_else(|| anyhow!("Windows has no language installed to read text with"))?;
        let result = engine.RecognizeAsync(&bitmap)?.get()?;
        let mut lines = Vec::new();
        for line in result.Lines()? {
            let text = line.Text()?.to_string();
            if !text.trim().is_empty() {
                lines.push(text);
            }
        }
        Ok(lines.join("\n"))
    })
}

/// A PDF opened by Windows' reader, for turning pages into pictures.
pub struct PdfFile {
    document: PdfDocument,
}

impl PdfFile {
    pub fn open(path: &Path) -> Result<PdfFile> {
        let file = storage_file(path)?;
        let document = PdfDocument::LoadFromFileAsync(&file)?
            .get()
            .map_err(|error| anyhow!("Windows can't open the PDF ({})", error.message()))?;
        Ok(PdfFile { document })
    }

    pub fn pages(&self) -> usize {
        self.document.PageCount().unwrap_or(0) as usize
    }

    /// Page `index` (from 0) as a picture about `side` pixels on its longer side.
    pub fn render(&self, index: usize, side: u32) -> Result<Picture> {
        let page = self.document.GetPage(index as u32)?;
        let size = page.Size()?;
        let options = PdfPageRenderOptions::new()?;
        let scale = side as f32 / size.Width.max(size.Height).max(1.0);
        options.SetDestinationWidth((size.Width * scale).round().max(1.0) as u32)?;
        options.SetDestinationHeight((size.Height * scale).round().max(1.0) as u32)?;
        let stream = InMemoryRandomAccessStream::new()?;
        page.RenderWithOptionsToStreamAsync(&stream, &options)?.get()?;
        let stream: IRandomAccessStream = windows::core::Interface::cast(&stream)?;
        stream.Seek(0)?;
        let decoder = BitmapDecoder::CreateAsync(&stream)?.get()?;
        pixels(&decoder, side)
    }
}
