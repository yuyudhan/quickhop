//! File and application icons as PNG, the way Finder shows them.
//!
//! `NSWorkspace iconForFile:` resolves every kind of icon macOS knows: `.icns` files named by
//! `CFBundleIconFile`, asset catalogs (`Assets.car`) of apps that ship no `.icns`, document
//! and folder icons. A path parser alone cannot read asset catalogs, which is why this lives in
//! the platform crate (APP-02).

use std::path::Path;

use crate::Unsupported;

/// Why an icon could not be rendered.
#[derive(Debug, thiserror::Error)]
pub enum IconError {
    /// Nothing exists at the path: macOS would answer with a generic document icon.
    #[error("no file at {0}")]
    NotFound(String),
    /// The path is not valid UTF-8, which `NSString` needs.
    #[error("path is not UTF-8: {0}")]
    NotUtf8(String),
    /// `AppKit` returned no bitmap or no PNG data.
    #[error("could not render the icon of {0}")]
    Render(String),
    /// Not running on macOS.
    #[error(transparent)]
    Unsupported(#[from] Unsupported),
}

/// Renders the icons of files and applications. The UI depends on this trait, so it can be
/// tested with a fake on every OS.
pub trait IconRenderer: Send + Sync {
    /// The icon of the file, folder or bundle at `path`, as PNG data at least `size` pixels
    /// square (larger when the closest representation is larger).
    ///
    /// # Errors
    /// When nothing exists at `path`, or `AppKit` cannot render it.
    fn render(&self, path: &Path, size: u32) -> Result<Vec<u8>, IconError>;
}

/// The system's icons, through `NSWorkspace`; on other systems every call fails with
/// [`Unsupported`].
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemIcons;

impl IconRenderer for SystemIcons {
    fn render(&self, path: &Path, size: u32) -> Result<Vec<u8>, IconError> {
        let display = path.display().to_string();
        let text = path
            .to_str()
            .ok_or_else(|| IconError::NotUtf8(display.clone()))?;
        if !path.exists() {
            return Err(IconError::NotFound(display));
        }
        if cfg!(not(target_os = "macos")) {
            return Err(Unsupported.into());
        }
        render_png(text, size).ok_or(IconError::Render(display))
    }
}

#[cfg(target_os = "macos")]
fn render_png(path: &str, size: u32) -> Option<Vec<u8>> {
    use objc2::AnyThread as _;
    use objc2::rc::autoreleasepool;
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSWorkspace};
    use objc2_foundation::{NSDictionary, NSPoint, NSRect, NSSize, NSString};

    // AppKit autoreleases intermediate images; the pool frees them on any thread.
    autoreleasepool(|_| {
        let image = NSWorkspace::sharedWorkspace().iconForFile(&NSString::from_str(path));
        let points = f64::from(size);
        let mut rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(points, points));
        // SAFETY: `rect` is a live, writable `NSRect` for the whole call (AppKit may adjust it);
        // no graphics context and no hints dictionary are passed.
        let image =
            unsafe { image.CGImageForProposedRect_context_hints(&raw mut rect, None, None) }?;
        let bitmap = NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), &image);
        let properties = NSDictionary::new();
        // SAFETY: an empty dictionary is a valid `NSDictionary<NSBitmapImageRepPropertyKey,
        // AnyObject>`: there are no values whose types could be wrong.
        let png = unsafe {
            bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &properties)
        }?;
        Some(png.to_vec())
    })
}

#[cfg(not(target_os = "macos"))]
fn render_png(_path: &str, _size: u32) -> Option<Vec<u8>> {
    None
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use std::collections::HashSet;
    use std::error::Error;

    use super::*;

    type TestResult = Result<(), Box<dyn Error>>;

    /// A decoded PNG.
    struct Bitmap {
        width: u32,
        height: u32,
        color: png::ColorType,
        pixels: Vec<u8>,
    }

    fn decode(data: &[u8]) -> Result<Bitmap, Box<dyn Error>> {
        let mut reader = png::Decoder::new(std::io::Cursor::new(data)).read_info()?;
        let mut pixels = vec![0; reader.output_buffer_size().ok_or("unbounded image")?];
        let info = reader.next_frame(&mut pixels)?;
        pixels.truncate(info.buffer_size());
        Ok(Bitmap {
            width: info.width,
            height: info.height,
            color: info.color_type,
            pixels,
        })
    }

    /// A system app present on every Mac.
    const APP: &str = "/System/Applications/Calculator.app";

    #[test]
    fn renders_an_application_icon_as_a_non_blank_png() -> TestResult {
        let icon = decode(&SystemIcons.render(Path::new(APP), 64)?)?;
        assert!(
            icon.width >= 64 && icon.width == icon.height,
            "a square icon of at least 64 px, got {}x{}",
            icon.width,
            icon.height
        );
        assert_eq!(icon.color, png::ColorType::Rgba);
        let (pixels, _) = icon.pixels.as_chunks::<4>();
        let opaque = pixels.iter().filter(|p| p[3] > 0).count();
        let total = pixels.len();
        assert!(
            opaque * 4 > total,
            "most of an app icon is drawn, got {opaque} of {total} pixels"
        );
        let distinct: HashSet<&[u8; 4]> = pixels.iter().collect();
        assert!(
            distinct.len() > 16,
            "an icon, not a flat fill: {} colours",
            distinct.len()
        );
        Ok(())
    }

    #[test]
    fn larger_sizes_give_larger_bitmaps() -> TestResult {
        let small = decode(&SystemIcons.render(Path::new(APP), 32)?)?.width;
        let large = decode(&SystemIcons.render(Path::new(APP), 512)?)?.width;
        assert!(
            large > small,
            "512 px request gave {large} px, 32 px request gave {small} px"
        );
        Ok(())
    }

    #[test]
    fn folders_have_icons_too() -> TestResult {
        let folder = std::env::temp_dir();
        assert!(decode(&SystemIcons.render(&folder, 32)?)?.width >= 32);
        Ok(())
    }

    #[test]
    fn missing_paths_are_errors_not_generic_icons() {
        let missing = Path::new("/nonexistent/quickhop/Nothing.app");
        assert!(matches!(
            SystemIcons.render(missing, 32),
            Err(IconError::NotFound(_))
        ));
    }

    #[test]
    fn rendering_works_off_the_main_thread() -> TestResult {
        let rendered =
            std::thread::spawn(|| SystemIcons.render(Path::new(APP), 32).map(|png| png.len()))
                .join()
                .map_err(|_| "the renderer thread panicked")?;
        assert!(rendered? > 0);
        Ok(())
    }
}
