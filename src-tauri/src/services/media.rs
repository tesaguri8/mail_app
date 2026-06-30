//! 画像レンディション生成。
//!
//! WebView（Windows の WebView2 等）は HEIC/HEIF を表示できないため、
//! Rust 側でデコードして web 互換形式（JPEG）に変換し、data URL で渡す。
//! HEIC デコードは Photosky と同じく libheif-rs を使う（ネイティブ libheif）。

use base64::Engine;
use image::ImageFormat;
use std::io::Cursor;
use std::sync::Mutex;

/// 表示用レンディションの最大辺（px）。4K ディスプレイに収まれば体感品質は十分で、
/// HEIC/大型画像のフルデコード後の巨大エンコードを避けて軽快にする。
pub const VIEW_MAX: u32 = 2048;
/// 一覧サムネイル用の最大辺（px）。
pub const THUMB_MAX: u32 = 480;

/// libheif-rs は並行呼び出しでデッドロックし得るため、デコードを直列化する。
static HEIC_LOCK: std::sync::LazyLock<Mutex<()>> = std::sync::LazyLock::new(|| Mutex::new(()));

/// 拡張子（小文字）を取り出す。
fn ext_lower(filename: &str) -> Option<String> {
    filename
        .rsplit_once('.')
        .map(|(_, e)| e.to_lowercase())
        .filter(|e| !e.is_empty())
}

/// HEIC/HEIF かどうか（content-type 優先、無ければ拡張子）。
pub fn is_heic(content_type: Option<&str>, filename: &str) -> bool {
    if let Some(ct) = content_type {
        let ct = ct.to_lowercase();
        if ct.contains("heic") || ct.contains("heif") {
            return true;
        }
    }
    matches!(ext_lower(filename).as_deref(), Some("heic" | "heif"))
}

/// web ブラウザ/WebView がそのまま表示できる画像形式か（変換不要）。
fn is_web_native_image(content_type: Option<&str>, filename: &str) -> bool {
    let by_ct = content_type
        .map(|ct| {
            let ct = ct.to_lowercase();
            ct.contains("jpeg")
                || ct.contains("jpg")
                || ct.contains("png")
                || ct.contains("gif")
                || ct.contains("webp")
                || ct.contains("bmp")
        })
        .unwrap_or(false);
    by_ct
        || matches!(
            ext_lower(filename).as_deref(),
            Some("jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp")
        )
}

/// 画像（変換すれば表示できるものを含む）かどうか。
pub fn is_image(content_type: Option<&str>, filename: &str) -> bool {
    if let Some(ct) = content_type {
        if ct.to_lowercase().starts_with("image/") {
            return true;
        }
    }
    is_web_native_image(content_type, filename)
        || is_heic(content_type, filename)
        || matches!(
            ext_lower(filename).as_deref(),
            Some("tiff" | "tif" | "avif")
        )
}

/// HEIC バイト列を libheif でデコードして DynamicImage にする（直列化）。
fn decode_heic(bytes: &[u8]) -> Result<image::DynamicImage, String> {
    let _guard = HEIC_LOCK
        .lock()
        .map_err(|e| format!("HEICロック失敗: {e}"))?;
    let lib = libheif_rs::LibHeif::new();
    let ctx = libheif_rs::HeifContext::read_from_bytes(bytes)
        .map_err(|e| format!("HEIC読み込み失敗: {e}"))?;
    let handle = ctx
        .primary_image_handle()
        .map_err(|e| format!("HEICハンドル取得失敗: {e}"))?;
    let img = lib
        .decode(
            &handle,
            libheif_rs::ColorSpace::Rgb(libheif_rs::RgbChroma::Rgba),
            None,
        )
        .map_err(|e| format!("HEICデコード失敗: {e}"))?;

    let plane = img
        .planes()
        .interleaved
        .ok_or_else(|| "HEICプレーン取得失敗".to_string())?;
    let (w, h, stride) = (plane.width, plane.height, plane.stride);
    let row_bytes = (w * 4) as usize;

    // stride（行バイト数）が width*4 と異なる場合は行ごとに詰め直す。
    let rgba = if stride == row_bytes {
        plane.data.to_vec()
    } else {
        let mut buf = Vec::with_capacity(row_bytes * h as usize);
        for y in 0..h as usize {
            let start = y * stride;
            buf.extend_from_slice(&plane.data[start..start + row_bytes]);
        }
        buf
    };

    let rgba_img =
        image::RgbaImage::from_raw(w, h, rgba).ok_or_else(|| "RGBA画像生成失敗".to_string())?;
    Ok(image::DynamicImage::ImageRgba8(rgba_img))
}

/// 画像バイト列を読み込む（HEIC は libheif、それ以外は image crate）。
fn load_image(
    bytes: &[u8],
    content_type: Option<&str>,
    filename: &str,
) -> Result<image::DynamicImage, String> {
    if is_heic(content_type, filename) {
        decode_heic(bytes)
    } else {
        image::load_from_memory(bytes).map_err(|e| format!("画像読み込み失敗: {e}"))
    }
}

/// 画像を web 表示用の JPEG レンディションにして data URL を返す。
/// HEIC は JPEG へ変換し、大きすぎる画像は max_size に収まるよう縮小する。
pub fn to_web_data_url(
    bytes: &[u8],
    content_type: Option<&str>,
    filename: &str,
    max_size: u32,
) -> Result<String, String> {
    let img = load_image(bytes, content_type, filename)?;
    let resized = if img.width() > max_size || img.height() > max_size {
        img.resize(max_size, max_size, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };

    let mut buf = Cursor::new(Vec::new());
    resized
        .write_to(&mut buf, ImageFormat::Jpeg)
        .map_err(|e| format!("JPEG変換失敗: {e}"))?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(buf.into_inner());
    Ok(format!("data:image/jpeg;base64,{b64}"))
}

/// HEIC をデコードして JPEG バイト列にする（OSで開く用にディスク保存する素材）。
pub fn heic_to_jpeg_bytes(bytes: &[u8], max_size: u32) -> Result<Vec<u8>, String> {
    let img = decode_heic(bytes)?;
    let resized = if img.width() > max_size || img.height() > max_size {
        img.resize(max_size, max_size, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    let mut buf = Cursor::new(Vec::new());
    resized
        .write_to(&mut buf, ImageFormat::Jpeg)
        .map_err(|e| format!("JPEG変換失敗: {e}"))?;
    Ok(buf.into_inner())
}
