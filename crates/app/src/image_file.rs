//! The file half of `initializeImage`: turns the bytes of a pasted or dropped image into a
//! `files` entry plus the natural size of the element that will show it.

use std::io::Cursor;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::imageops::FilterType;
use image::metadata::Orientation;
use image::{
    DynamicImage, ExtendedColorType, ImageDecoder, ImageEncoder, ImageFormat, ImageReader,
};
use scene::file::FileData;
use scene::image::PreparedImage;
use sha1::{Digest, Sha1};

/// `DEFAULT_MAX_IMAGE_WIDTH_OR_HEIGHT`: the long side images are downsized to.
const MAX_SIDE: u32 = 1440;
/// `MAX_ALLOWED_FILE_BYTES`, checked against the bytes that end up in the data URL.
const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;
/// Quality of re-encoded JPEGs.
const JPEG_QUALITY: u8 = 92;

#[derive(Debug, PartialEq)]
pub enum PrepareError {
    Svg,
    Unsupported,
    TooBig,
    Decode(String),
}

/// `initializeImage`'s file work, off the UI thread: sniffs the format from the bytes
/// (not the claimed MIME type), rejects SVG, takes the SHA-1 of the original bytes as the
/// `fileId`, bakes the EXIF orientation into the pixels (so any image whose orientation is
/// not the identity is re-encoded even when small), downsizes past 1440 px on the long side
/// and re-encodes, rejects anything still over 4 MiB, and builds the data URL. `now_ms`
/// stamps `created`/`lastRetrieved`.
pub fn prepare(bytes: &[u8], now_ms: f64) -> Result<PreparedImage, PrepareError> {
    prepare_with_limit(bytes, now_ms, MAX_FILE_BYTES)
}

/// Lowercase hex SHA-1 (`generateIdFromFile`).
pub fn file_id(bytes: &[u8]) -> String {
    Sha1::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn prepare_with_limit(
    bytes: &[u8],
    now_ms: f64,
    max_bytes: usize,
) -> Result<PreparedImage, PrepareError> {
    let format = match image::guess_format(bytes) {
        Ok(f @ (ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP | ImageFormat::Gif)) => f,
        _ if looks_like_svg(bytes) => return Err(PrepareError::Svg),
        _ => return Err(PrepareError::Unsupported),
    };
    let decode_err = |e: image::ImageError| PrepareError::Decode(e.to_string());

    let mut decoder = ImageReader::with_format(Cursor::new(bytes), format)
        .into_decoder()
        .map_err(decode_err)?;
    // Stored pixels are always upright, so nothing downstream reads EXIF.
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let (mut width, mut height) = decoder.dimensions();
    if matches!(
        orientation,
        Orientation::Rotate90
            | Orientation::Rotate270
            | Orientation::Rotate90FlipH
            | Orientation::Rotate270FlipH
    ) {
        std::mem::swap(&mut width, &mut height);
    }
    let too_large = width.max(height) > MAX_SIDE;

    let (mime, out_bytes, size) = if !too_large && orientation == Orientation::NoTransforms {
        (mime_of(format), None, (width, height))
    } else {
        let mut img = DynamicImage::from_decoder(decoder).map_err(decode_err)?;
        img.apply_orientation(orientation);
        let (w, h) = if too_large {
            let scale = f64::from(MAX_SIDE) / f64::from(width.max(height));
            let scaled = |side: u32| ((f64::from(side) * scale).round() as u32).max(1);
            let dims = if width >= height {
                (MAX_SIDE, scaled(height))
            } else {
                (scaled(width), MAX_SIDE)
            };
            img = img.resize_exact(dims.0, dims.1, FilterType::Lanczos3);
            dims
        } else {
            (width, height)
        };
        let mut out = Vec::new();
        let mime = if format == ImageFormat::Jpeg {
            let rgb = img.to_rgb8();
            JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY)
                .write_image(&rgb, w, h, ExtendedColorType::Rgb8)
                .map_err(decode_err)?;
            "image/jpeg"
        } else {
            // GIF and WebP re-encode as PNG.
            let rgba = img.to_rgba8();
            PngEncoder::new(&mut out)
                .write_image(&rgba, w, h, ExtendedColorType::Rgba8)
                .map_err(decode_err)?;
            "image/png"
        };
        (mime, Some(out), (w, h))
    };

    let final_bytes = out_bytes.as_deref().unwrap_or(bytes);
    if final_bytes.len() > max_bytes {
        return Err(PrepareError::TooBig);
    }
    Ok(PreparedImage {
        file: FileData {
            id: file_id(bytes),
            mime_type: mime.to_string(),
            data_url: format!("data:{mime};base64,{}", STANDARD.encode(final_bytes)),
            created: now_ms,
            last_retrieved: now_ms,
        },
        natural_size: [f64::from(size.0), f64::from(size.1)],
    })
}

fn mime_of(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Jpeg => "image/jpeg",
        ImageFormat::WebP => "image/webp",
        ImageFormat::Gif => "image/gif",
        _ => "image/png",
    }
}

fn looks_like_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(1024)];
    let text = String::from_utf8_lossy(head);
    let text = text.trim_start_matches('\u{feff}').trim_start();
    text.starts_with("<svg")
        || (text.starts_with("<?xml") || text.starts_with("<!")) && text.contains("<svg")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_png(width: u32, height: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_fn(width, height, |x, y| {
            image::Rgba([(x % 256) as u8, (y % 256) as u8, 90, 255])
        });
        let mut out = Vec::new();
        PngEncoder::new(&mut out)
            .write_image(img.as_raw(), width, height, ExtendedColorType::Rgba8)
            .unwrap();
        out
    }

    fn base64_encode(bytes: &[u8]) -> String {
        STANDARD.encode(bytes)
    }

    #[test]
    fn file_ids_are_sha1_hex() {
        assert_eq!(file_id(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
    }

    #[test]
    fn a_small_png_keeps_its_bytes_and_size() {
        let bytes = encode_png(300, 200);
        let prepared = prepare(&bytes, 5.0).unwrap();
        assert_eq!(prepared.natural_size, [300.0, 200.0]);
        assert_eq!(prepared.file.id, file_id(&bytes));
        assert_eq!(prepared.file.mime_type, "image/png");
        assert_eq!(
            prepared.file.data_url,
            format!("data:image/png;base64,{}", base64_encode(&bytes))
        );
        assert_eq!(prepared.file.created, 5.0);
        assert_eq!(prepared.file.last_retrieved, 5.0);
    }

    #[test]
    fn a_large_image_is_downsized_to_1440_on_its_long_side() {
        let bytes = encode_png(3000, 1500);
        let prepared = prepare(&bytes, 0.0).unwrap();
        assert_eq!(prepared.natural_size, [1440.0, 720.0]);
        assert_eq!(
            prepared.file.id,
            file_id(&bytes),
            "the id hashes the original bytes"
        );
        assert!(prepared.file.data_url.starts_with("data:image/png;base64,"));
    }

    #[test]
    fn a_tall_image_is_downsized_on_its_height() {
        let prepared = prepare(&encode_png(1000, 2880), 0.0).unwrap();
        assert_eq!(prepared.natural_size, [500.0, 1440.0]);
    }

    #[test]
    fn a_large_jpeg_stays_a_jpeg() {
        let img = image::RgbImage::from_pixel(2000, 1000, image::Rgb([10, 200, 30]));
        let mut bytes = Vec::new();
        JpegEncoder::new(&mut bytes)
            .write_image(img.as_raw(), 2000, 1000, ExtendedColorType::Rgb8)
            .unwrap();
        let prepared = prepare(&bytes, 0.0).unwrap();
        assert_eq!(prepared.natural_size, [1440.0, 720.0]);
        assert_eq!(prepared.file.mime_type, "image/jpeg");
    }

    #[test]
    fn the_format_comes_from_the_bytes_not_a_name() {
        let prepared = prepare(&encode_png(4, 4), 0.0).unwrap();
        assert_eq!(prepared.file.mime_type, "image/png");
    }

    #[test]
    fn svg_and_garbage_are_rejected() {
        assert_eq!(
            prepare(b"<svg xmlns='http://www.w3.org/2000/svg'></svg>", 0.0),
            Err(PrepareError::Svg)
        );
        assert_eq!(
            prepare(b"<?xml version='1.0'?>\n<svg></svg>", 0.0),
            Err(PrepareError::Svg)
        );
        assert_eq!(
            prepare(b"not an image", 0.0),
            Err(PrepareError::Unsupported)
        );
    }

    #[test]
    fn a_truncated_png_is_a_decode_error() {
        let bytes = encode_png(3000, 1500);
        assert!(matches!(
            prepare(&bytes[..200], 0.0),
            Err(PrepareError::Decode(_))
        ));
    }

    #[test]
    fn output_over_the_byte_limit_is_too_big() {
        let bytes = encode_png(300, 200);
        assert_eq!(
            prepare_with_limit(&bytes, 0.0, bytes.len() - 1),
            Err(PrepareError::TooBig)
        );
        assert!(prepare_with_limit(&bytes, 0.0, bytes.len()).is_ok());
    }

    /// A JPEG whose APP1 Exif segment says Orientation = 6 (rotate 90 CW to display).
    fn jpeg_with_orientation_6(width: u32, height: u32) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(width, height, image::Rgb([200, 40, 40]));
        let mut jpeg = Vec::new();
        JpegEncoder::new(&mut jpeg)
            .write_image(img.as_raw(), width, height, ExtendedColorType::Rgb8)
            .unwrap();
        let mut tiff = b"MM\0\x2a\0\0\0\x08".to_vec();
        tiff.extend([0, 1, 0x01, 0x12, 0, 3, 0, 0, 0, 1, 0, 6, 0, 0, 0, 0, 0, 0]);
        let mut payload = b"Exif\0\0".to_vec();
        payload.extend(tiff);
        let mut out = vec![0xff, 0xd8, 0xff, 0xe1];
        out.extend(((payload.len() + 2) as u16).to_be_bytes());
        out.extend(payload);
        out.extend(&jpeg[2..]);
        out
    }

    fn decoded_size(data_url: &str) -> (u32, u32) {
        let b64 = data_url.split_once(',').unwrap().1;
        let bytes = STANDARD.decode(b64).unwrap();
        let img = image::load_from_memory(&bytes).unwrap();
        (img.width(), img.height())
    }

    #[test]
    fn exif_orientation_is_baked_into_the_stored_pixels() {
        let bytes = jpeg_with_orientation_6(120, 80);
        let prepared = prepare(&bytes, 0.0).unwrap();
        assert_eq!(prepared.natural_size, [80.0, 120.0]);
        assert_eq!(prepared.file.mime_type, "image/jpeg");
        assert_eq!(prepared.file.id, file_id(&bytes));
        assert_eq!(decoded_size(&prepared.file.data_url), (80, 120));
    }

    #[test]
    fn a_large_rotated_jpeg_is_upright_and_downsized() {
        let prepared = prepare(&jpeg_with_orientation_6(3000, 1500), 0.0).unwrap();
        assert_eq!(prepared.natural_size, [720.0, 1440.0]);
        assert_eq!(decoded_size(&prepared.file.data_url), (720, 1440));
    }

    #[test]
    fn a_large_gif_becomes_a_png() {
        let img = image::RgbaImage::from_pixel(2000, 1000, image::Rgba([1, 2, 3, 255]));
        let mut bytes = Vec::new();
        image::codecs::gif::GifEncoder::new(&mut bytes)
            .encode(img.as_raw(), 2000, 1000, ExtendedColorType::Rgba8)
            .unwrap();
        let prepared = prepare(&bytes, 0.0).unwrap();
        assert_eq!(prepared.natural_size, [1440.0, 720.0]);
        assert_eq!(prepared.file.mime_type, "image/png");
        assert!(prepared.file.data_url.starts_with("data:image/png;base64,"));
    }

    #[test]
    fn a_large_webp_becomes_a_png() {
        let img = image::RgbaImage::from_pixel(1000, 2000, image::Rgba([1, 2, 3, 255]));
        let mut bytes = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(&mut bytes)
            .write_image(img.as_raw(), 1000, 2000, ExtendedColorType::Rgba8)
            .unwrap();
        let prepared = prepare(&bytes, 0.0).unwrap();
        assert_eq!(prepared.natural_size, [720.0, 1440.0]);
        assert_eq!(prepared.file.mime_type, "image/png");
    }
}
