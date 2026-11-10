//! Pictures as packed RGB, turned the way their EXIF says, plus what the camera wrote about them.

use super::Picture;
use anyhow::{anyhow, Result};
use image::{DynamicImage, ImageDecoder, ImageReader};
use std::path::Path;

/// Larger pictures are scaled down as they're read: the model looks at far fewer pixels anyway.
const MAX_SIDE: u32 = 4096;

pub fn load(path: &Path) -> Result<Picture> {
    match load_with_crate(path) {
        Ok(picture) => Ok(picture),
        Err(error) => {
            // HEIC, AVIF, RAW and whatever else Windows has a codec for.
            #[cfg(windows)]
            if let Ok(picture) = super::winrt::decode(path, MAX_SIDE) {
                return Ok(picture);
            }
            Err(error)
        }
    }
}

fn load_with_crate(path: &Path) -> Result<Picture> {
    let mut reader = ImageReader::open(path)?.with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(1 << 30);
    reader.limits(limits);
    let mut decoder = reader.into_decoder()?;
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut picture = DynamicImage::from_decoder(decoder)?;
    if picture.width() > MAX_SIDE || picture.height() > MAX_SIDE {
        picture = picture.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Triangle);
    }
    picture.apply_orientation(orientation);
    Ok(from_dynamic(picture))
}

/// Transparent pixels over white, as a viewer shows them.
pub fn from_dynamic(picture: DynamicImage) -> Picture {
    let (width, height) = (picture.width() as usize, picture.height() as usize);
    let rgb = if picture.color().has_alpha() {
        let rgba = picture.into_rgba8();
        let mut out = Vec::with_capacity(width * height * 3);
        for pixel in rgba.pixels() {
            let alpha = pixel[3] as u32;
            for channel in 0..3 {
                out.push(((pixel[channel] as u32 * alpha + 255 * (255 - alpha)) / 255) as u8);
            }
        }
        out
    } else {
        picture.into_rgb8().into_raw()
    };
    Picture { width, height, rgb }
}

pub fn decode_bytes(bytes: &[u8]) -> Result<Picture> {
    let picture = image::load_from_memory(bytes).map_err(|error| anyhow!("can't read the picture: {error}"))?;
    Ok(from_dynamic(picture))
}

/// A picture as JPEG bytes (thumbnails, and frames handed to the interface).
pub fn jpeg(picture: &Picture, quality: u8) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
    image::ImageEncoder::write_image(
        encoder, &picture.rgb, picture.width as u32, picture.height as u32, image::ExtendedColorType::Rgb8,
    )?;
    Ok(out)
}

/// What the camera recorded: when the picture was taken, with what, and where.
pub fn exif(path: &Path) -> serde_json::Map<String, serde_json::Value> {
    let mut out = serde_json::Map::new();
    let Ok(file) = std::fs::File::open(path) else { return out };
    let Ok(data) = exif::Reader::new().read_from_container(&mut std::io::BufReader::new(file)) else { return out };
    let text = |tag: exif::Tag| -> Option<String> {
        data.get_field(tag, exif::In::PRIMARY)
            .map(|field| field.display_value().to_string().trim_matches('"').trim().to_string())
            .filter(|value| !value.is_empty())
    };
    if let Some(taken) = text(exif::Tag::DateTimeOriginal).or_else(|| text(exif::Tag::DateTime)) {
        out.insert("taken".into(), taken.into());
    }
    let camera = [text(exif::Tag::Make), text(exif::Tag::Model)].into_iter().flatten().collect::<Vec<_>>().join(" ");
    if !camera.is_empty() {
        out.insert("camera".into(), camera.into());
    }
    let degrees = |tag: exif::Tag, reference: exif::Tag| -> Option<f64> {
        let field = data.get_field(tag, exif::In::PRIMARY)?;
        let exif::Value::Rational(parts) = &field.value else { return None };
        if parts.len() < 3 {
            return None;
        }
        let value = parts[0].to_f64() + parts[1].to_f64() / 60.0 + parts[2].to_f64() / 3600.0;
        let negative = text(reference).is_some_and(|r| r.starts_with('S') || r.starts_with('W'));
        Some(if negative { -value } else { value })
    };
    if let (Some(latitude), Some(longitude)) = (
        degrees(exif::Tag::GPSLatitude, exif::Tag::GPSLatitudeRef),
        degrees(exif::Tag::GPSLongitude, exif::Tag::GPSLongitudeRef),
    ) {
        if latitude.is_finite() && longitude.is_finite() && (latitude != 0.0 || longitude != 0.0) {
            out.insert("lat".into(), ((latitude * 1e5).round() / 1e5).into());
            out.insert("lon".into(), ((longitude * 1e5).round() / 1e5).into());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transparent_pixels_sit_on_white() {
        let mut image = image::RgbaImage::new(2, 1);
        image.put_pixel(0, 0, image::Rgba([0, 0, 0, 0]));
        image.put_pixel(1, 0, image::Rgba([10, 20, 30, 255]));
        let picture = from_dynamic(DynamicImage::ImageRgba8(image));
        assert_eq!(picture.rgb, vec![255, 255, 255, 10, 20, 30]);
    }

    #[test]
    fn pictures_round_trip_through_files() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("a.png");
        image::RgbImage::from_fn(40, 20, |x, _| image::Rgb([(x * 6) as u8, 100, 50])).save(&path).unwrap();
        let picture = load(&path).unwrap();
        assert_eq!((picture.width, picture.height), (40, 20));
        assert_eq!(&picture.rgb[..3], &[0, 100, 50]);
        let small = picture.fit(10);
        assert_eq!((small.width, small.height), (10, 5));
        assert!(jpeg(&picture, 80).unwrap().starts_with(&[0xFF, 0xD8]));
    }
}
