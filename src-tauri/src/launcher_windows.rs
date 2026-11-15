//! Windows: ShellExecute + shell icon extraction.

use std::path::Path;
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use windows::Win32::UI::Shell::{SHGetFileInfoW, ShellExecuteW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO, SW_SHOWNORMAL};

/// ShellExecute directly (no cmd), so `&` and friends in paths are inert.
pub fn shell_open(target: &str) -> Result<(), String> {
    let file = HSTRING::from(target);
    let r = unsafe { ShellExecuteW(None, w!("open"), &file, PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    if r.0 as usize > 32 { Ok(()) } else { Err(format!("could not open {target} (code {})", r.0 as usize)) }
}

fn icon_png(path: &Path) -> Option<Vec<u8>> {
    unsafe {
        let mut fi = SHFILEINFOW::default();
        let ok = SHGetFileInfoW(
            &HSTRING::from(path.as_os_str()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut fi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if ok == 0 || fi.hIcon.is_invalid() {
            return None;
        }
        let png = icon_to_png(fi.hIcon);
        let _ = DestroyIcon(fi.hIcon);
        png
    }
}

unsafe fn icon_to_png(icon: HICON) -> Option<Vec<u8>> {
    let mut info = ICONINFO::default();
    GetIconInfo(icon, &mut info).ok()?;
    let png = bitmap_to_png(info.hbmColor);
    if !info.hbmColor.is_invalid() { let _ = DeleteObject(info.hbmColor.into()); }
    if !info.hbmMask.is_invalid() { let _ = DeleteObject(info.hbmMask.into()); }
    png
}

unsafe fn bitmap_to_png(bmp: HBITMAP) -> Option<Vec<u8>> {
    if bmp.is_invalid() {
        return None; // monochrome icon
    }
    let mut bm = BITMAP::default();
    if GetObjectW(bmp.into(), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _)) == 0 {
        return None;
    }
    let (w, h) = (bm.bmWidth, bm.bmHeight);
    if w <= 0 || h <= 0 {
        return None;
    }
    let mut bi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: w,
        biHeight: -h, // top-down
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
        },
        ..Default::default()
    };
    let mut px = vec![0u8; (w * h * 4) as usize];
    let dc = GetDC(None);
    let lines = GetDIBits(dc, bmp, 0, h as u32, Some(px.as_mut_ptr() as *mut _), &mut bi, DIB_RGB_COLORS);
    ReleaseDC(None, dc);
    if lines == 0 {
        return None;
    }
    // Icons without an alpha channel come back all-zero alpha: make them opaque.
    let opaque = px.as_chunks::<4>().0.iter().all(|p| p[3] == 0);
    for p in px.as_chunks_mut::<4>().0.iter_mut() {
        p.swap(0, 2); // BGRA -> RGBA
        if opaque { p[3] = 255; }
    }
    let img = image::RgbaImage::from_raw(w as u32, h as u32, px)?;
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// `data:image/png;base64,...` of the file's icon, or None if it can't be extracted.
pub fn icon_png_uri(path: &Path) -> Option<String> {
    let png = icon_png(path)?;
    Some(format!("data:image/png;base64,{}", super::base64(&png)))
}
