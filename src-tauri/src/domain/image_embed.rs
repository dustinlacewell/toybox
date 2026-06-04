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
