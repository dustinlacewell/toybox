//! Prepare a texture's bytes for embedding in a GLB. GLB images must be PNG or
//! JPEG; the library has exactly one TGA, which we transcode to PNG. PNG/JPEG
//! pass through untouched (byte-preserving).

use std::io::Cursor;

use crate::error::{AppError, AppResult};

pub struct PreparedImage {
    pub mime: String,
    pub bytes: Vec<u8>,
}

/// Given a texture's file name (for extension) and raw bytes, return GLB-legal
/// image bytes + MIME. PNG/JPEG pass through; TGA is decoded and re-encoded PNG.
pub fn prepare_for_glb(file_name: &str, bytes: Vec<u8>) -> AppResult<PreparedImage> {
    let ext = file_name.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "png" => Ok(PreparedImage { mime: "image/png".into(), bytes }),
        "jpg" | "jpeg" => Ok(PreparedImage { mime: "image/jpeg".into(), bytes }),
        "tga" => transcode_to_png(&bytes),
        other => Err(AppError::msg(format!("unsupported texture type: .{other}"))),
    }
}

fn transcode_to_png(tga: &[u8]) -> AppResult<PreparedImage> {
    let img = image::load_from_memory_with_format(tga, image::ImageFormat::Tga)
        .map_err(|e| AppError::msg(format!("decode tga: {e}")))?;
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png)
        .map_err(|e| AppError::msg(format!("encode png: {e}")))?;
    Ok(PreparedImage { mime: "image/png".into(), bytes: out.into_inner() })
}

/// Decode any supported image and re-encode it as PNG scaled down to fit
/// `max_size` × `max_size` (aspect kept). An image already within the box is
/// not enlarged. `format` names the encoding when the caller knows it (a TGA has
/// no signature to detect); `None` detects it from the bytes.
pub fn thumb_png(
    bytes: &[u8],
    format: Option<image::ImageFormat>,
    max_size: u32,
) -> AppResult<Vec<u8>> {
    let decoded = match format {
        Some(format) => image::load_from_memory_with_format(bytes, format),
        None => image::load_from_memory(bytes),
    };
    let img = decoded.map_err(|e| AppError::msg(format!("decode image: {e}")))?;
    let img = if img.width() > max_size || img.height() > max_size {
        img.thumbnail(max_size, max_size)
    } else {
        img
    };
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png)
        .map_err(|e| AppError::msg(format!("encode png: {e}")))?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::DynamicImage::new_rgb8(w, h);
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn size_of(bytes: &[u8]) -> (u32, u32) {
        let img = image::load_from_memory(bytes).unwrap();
        (img.width(), img.height())
    }

    #[test]
    fn thumb_scales_down_keeping_aspect() {
        let out = thumb_png(&png(64, 32), None, 16).unwrap();
        assert_eq!(size_of(&out), (16, 8));
    }

    #[test]
    fn thumb_does_not_enlarge() {
        let out = thumb_png(&png(8, 4), None, 16).unwrap();
        assert_eq!(size_of(&out), (8, 4));
    }

    #[test]
    fn thumb_rejects_garbage() {
        assert!(thumb_png(b"not an image", None, 16).is_err());
    }
}
