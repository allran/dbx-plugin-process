use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Cursor;
use std::path::{Path, PathBuf};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use dbx_plugin_sdk::PluginError;
use image::imageops::FilterType;
use image::{ImageFormat, ImageReader};

pub const ICON_EDGE: u32 = 64;

pub fn data_dir() -> PathBuf {
    env::var_os("DBX_PLUGIN_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| env::temp_dir().join("dbx-plugin-data").join("io.dbx.process"))
}

pub fn icon_cache_dir() -> PathBuf {
    data_dir().join("icons")
}

pub fn cache_path_for_key(key: &str) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    icon_cache_dir().join(format!("{:x}.png", hasher.finish()))
}

pub fn read_png_data_url(path: &Path) -> Result<String, PluginError> {
    let bytes = fs::read(path)
        .map_err(|err| PluginError::new(-32000, format!("Failed to read icon: {err}")))?;
    Ok(format!("data:image/png;base64,{}", BASE64.encode(bytes)))
}

#[cfg_attr(not(windows), allow(dead_code))]
pub fn write_rgba_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> bool {
    let expected = (width as usize)
        .saturating_mul(height as usize)
        .saturating_mul(4);
    if rgba.len() < expected || width == 0 || height == 0 {
        return false;
    }
    let Some(img) = image::RgbaImage::from_raw(width, height, rgba[..expected].to_vec()) else {
        return false;
    };
    let img = if width != ICON_EDGE || height != ICON_EDGE {
        image::imageops::resize(&img, ICON_EDGE, ICON_EDGE, FilterType::Lanczos3)
    } else {
        img
    };
    img.save_with_format(path, ImageFormat::Png).is_ok()
}

#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
pub fn resize_image_file_to_png(input: &Path, out_png: &Path) -> bool {
    let reader = match ImageReader::open(input) {
        Ok(reader) => reader,
        Err(_) => return false,
    };
    let reader = match reader.with_guessed_format() {
        Ok(reader) => reader,
        Err(_) => return false,
    };
    let img = match reader.decode() {
        Ok(img) => img.into_rgba8(),
        Err(_) => return false,
    };
    let resized = image::imageops::resize(&img, ICON_EDGE, ICON_EDGE, FilterType::Lanczos3);
    let mut bytes = Cursor::new(Vec::new());
    if image::DynamicImage::ImageRgba8(resized)
        .write_to(&mut bytes, ImageFormat::Png)
        .is_err()
    {
        return false;
    }
    fs::write(out_png, bytes.into_inner()).is_ok()
}
