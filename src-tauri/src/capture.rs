//! Screenshot (specs/phase-2.1/spec.md, constitution Amendment 1): one
//! local still frame on user action — BitBlt the screen, flip BGRA→RGBA,
//! save a PNG into Pictures\Jini, copy the image to the clipboard.

use std::fs;
use std::path::PathBuf;

use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateCompatibleBitmap, DeleteDC, DeleteObject, GetDC, GetDIBits,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

pub struct Capture {
    pub width: u32,
    pub height: u32,
    /// RGBA, row-major, top-down.
    pub rgba: Vec<u8>,
}

pub fn capture_display() -> Result<Capture, String> {
    unsafe {
        let width = GetSystemMetrics(SM_CXSCREEN);
        let height = GetSystemMetrics(SM_CYSCREEN);
        if width <= 0 || height <= 0 {
            return Err("Could not read the screen size.".into());
        }
        let (width, height) = (width as u32, height as u32);

        let screen_dc = GetDC(None);
        if screen_dc.is_invalid() {
            return Err("Could not access the screen.".into());
        }
        let mem_dc = CreateCompatibleDC(screen_dc);
        let bitmap = CreateCompatibleBitmap(screen_dc, width as i32, height as i32);
        let old = windows::Win32::Graphics::Gdi::SelectObject(mem_dc, HGDIOBJ(bitmap.0));

        let copied = BitBlt(mem_dc, 0, 0, width as i32, height as i32, screen_dc, 0, 0, SRCCOPY);

        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32), // top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
        let read = GetDIBits(
            mem_dc,
            bitmap,
            0,
            height,
            Some(pixels.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        );

        windows::Win32::Graphics::Gdi::SelectObject(mem_dc, old);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(mem_dc);
        let _ = windows::Win32::Graphics::Gdi::ReleaseDC(None, screen_dc);

        if copied.is_err() || read == 0 {
            return Err("Screen copy failed. The display may be protected.".into());
        }

        for px in pixels.chunks_exact_mut(4) {
            px.swap(0, 2); // BGRA → RGBA
        }
        Ok(Capture {
            width,
            height,
            rgba: pixels,
        })
    }
}

fn png_bytes(capture: &Capture) -> Result<Vec<u8>, String> {
    let rgba = image::RgbaImage::from_raw(capture.width, capture.height, capture.rgba.clone())
        .ok_or("Capture buffer size mismatch.")?;
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(rgba)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .map_err(|e| format!("Could not encode PNG: {e}"))?;
    Ok(out)
}

/// Pure crop: keep the sub-rectangle (x, y, w, h), clamped to the capture.
/// x/y clamp to width/height (not width-1) so a zero-size crop at the far
/// edge stays empty instead of producing a spurious 1-pixel strip.
pub fn crop(capture: &Capture, x: u32, y: u32, w: u32, h: u32) -> Capture {
    let x = x.min(capture.width);
    let y = y.min(capture.height);
    let w = w.min(capture.width - x);
    let h = h.min(capture.height - y);
    let mut rgba = Vec::with_capacity((w as usize) * (h as usize) * 4);
    for row in y..(y + h) {
        let start = (row as usize) * (capture.width as usize) * 4 + (x as usize) * 4;
        rgba.extend_from_slice(&capture.rgba[start..start + (w as usize) * 4]);
    }
    Capture {
        width: w,
        height: h,
        rgba,
    }
}

/// PNG as base64 — the form the chat client sends to the provider.
pub fn base64_png(capture: &Capture) -> Result<String, String> {
    let bytes = png_bytes(capture)?;
    // Base64 via a tiny local implementation? No — the standard crate-less
    // approach is overkill; reuse the `image`-adjacent helper below.
    Ok(base64_encode(&bytes))
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

/// `Pictures\Jini\screenshot-YYYYMMDD-HHMMSS.png` under the user profile.
pub fn save_png(capture: &Capture) -> Result<PathBuf, String> {
    let dir = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .map(|home| home.join("Pictures").join("Jini"))
        .ok_or_else(|| "Could not resolve the user profile folder.".to_string())?;
    fs::create_dir_all(&dir).map_err(|e| format!("Could not create the folder: {e}"))?;
    let path = dir.join(format!("screenshot-{}.png", timestamp()));
    fs::write(&path, png_bytes(capture)?).map_err(|e| format!("Could not save the file: {e}"))?;
    Ok(path)
}

pub fn copy_to_clipboard(capture: &Capture) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard
        .set_image(arboard::ImageData {
            width: capture.width as usize,
            height: capture.height as usize,
            bytes: std::borrow::Cow::Borrowed(&capture.rgba),
        })
        .map_err(|e| e.to_string())
}

/// `YYYYMMDD-HHMMSS` from the system clock, no chrono dependency.
fn timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (y, mo, d) = civil_from_days(days);
    format!("{y:04}{mo:02}{d:02}-{h:02}{m:02}{s:02}")
}

/// Howard Hinnant's days-to-civil algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::{base64_encode, civil_from_days, crop, Capture};

    fn solid(w: u32, h: u32) -> Capture {
        Capture {
            width: w,
            height: h,
            rgba: vec![200u8; (w * h * 4) as usize],
        }
    }

    #[test]
    fn civil_epoch_is_1970_01_01() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }

    #[test]
    fn civil_known_date() {
        // 2026-09-18 is day 20714 since the epoch.
        assert_eq!(civil_from_days(20_714), (2026, 9, 18));
    }

    #[test]
    fn crop_keeps_subrectangle() {
        let full = solid(100, 100);
        let part = crop(&full, 10, 20, 30, 40);
        assert_eq!((part.width, part.height), (30, 40));
        assert_eq!(part.rgba.len(), 30 * 40 * 4);
    }

    #[test]
    fn crop_clamps_overflow() {
        let full = solid(100, 100);
        let part = crop(&full, 90, 90, 500, 500);
        assert_eq!((part.width, part.height), (10, 10));
    }

    #[test]
    fn crop_at_far_edge_is_empty_not_one_pixel() {
        let full = solid(100, 100);
        let part = crop(&full, 100, 100, 0, 0);
        assert_eq!((part.width, part.height), (0, 0));
        assert!(part.rgba.is_empty());
    }

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }
}
