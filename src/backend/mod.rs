//! The single interface all scanner access goes through (constitution Principle III).

pub mod fake;

// Constitution Principle VI: `unsafe` code is allowed only in the module that wraps the SANE
// C library. The crate root denies `unsafe_code`; this attribute lifts that for `sane` alone.
#[allow(unsafe_code)]
pub mod sane;

use std::sync::atomic::AtomicBool;

use serde::{Deserialize, Serialize};

use crate::capability::Capability;
use crate::error::ScanError;
use crate::frame_stats::FrameStats;

/// A scanner found by the backend.
// A `struct` groups named fields into one type. `Clone` allows explicit copies with
// `.clone()`; `PartialEq` allows `==`; `Serialize`/`Deserialize` (serde) convert it to and
// from JSON using the field names below as keys.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// SANE device name, e.g. `pieusb:libusb:001:004`. Non-empty; used with `--device`.
    pub name: String,
    /// As reported by the driver, e.g. `PIE`. May be empty.
    pub vendor: String,
    /// As reported by the driver, e.g. `MF Scanner`. May be empty.
    pub model: String,
    /// SANE device type, e.g. `film scanner`. Named `kind` because `type` is a Rust keyword.
    pub kind: String,
}

// A *trait* is a set of methods a type promises to provide, like an interface in other
// languages. `SaneBackend` (the real scanner) and `FakeBackend` (for tests) both implement
// it, and the command code in `lib.rs` is written against the trait, not either type.
/// Everything the tool needs from a scanner driver.
pub trait ScannerBackend {
    /// List the scanners this backend can see, without opening them.
    fn list_devices(&mut self) -> Result<Vec<DeviceInfo>, ScanError>;

    /// Open `device`, read every option's description and current value, and close it again.
    /// Strictly read-only: nothing on the scanner is changed (FR-006). Stops with
    /// [`ScanError::Interrupted`] when `cancel` becomes true.
    fn read_capabilities(
        &mut self,
        device: &str,
        cancel: &AtomicBool,
    ) -> Result<Vec<Capability>, ScanError>;

    // An *associated type* lets each implementation of a trait name a type of its own:
    // `SaneBackend` opens a `SaneSession`, `FakeBackend` a `FakeSession`. Code written
    // against the trait (such as `scan.rs`) refers to it as `B::Session` and works with
    // either, without knowing which it is.
    /// The open-scanner type this backend hands out.
    type Session: ScanSession;

    /// Open a scanner for scanning: `device` if given, otherwise the first one found. The
    /// scanner is released when the session is dropped.
    fn open(&mut self, device: Option<&str>) -> Result<Self::Session, ScanError>;
}

/// An open scanner that can capture frames (research D1). One frame is captured with
/// `start_frame`, `read` until it returns 0, then `end_frame`.
pub trait ScanSession {
    /// The scanner this session has open.
    fn device(&self) -> &DeviceInfo;

    /// Apply `settings` to the scanner. Nothing is stored on the scanner itself; the values
    /// apply to this session's scans only.
    fn configure(&mut self, settings: &ScanSettings) -> Result<(), ScanError>;

    /// Capture a frame and return its layout. With the PrimeFilm XA this is where the frame
    /// is scanned and, when `advance_after` is set, the film is moved on. The fake scanner
    /// uses `cancel` to simulate Ctrl-C; the real one cannot be interrupted mid-frame.
    fn start_frame(&mut self, cancel: &AtomicBool) -> Result<FrameParams, ScanError>;

    /// Copy the next frame bytes into `buf`; returns how many, 0 at the end of the frame.
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, ScanError>;

    /// Finish the frame (after the last `read`, or to abandon it after an error).
    fn end_frame(&mut self);

    /// Read every option's current value, for the settings record of a scan.
    fn options(&mut self) -> Result<Vec<Capability>, ScanError>;
}

/// What kind of capture to make (data-model.md, "Scan Settings").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanKind {
    /// An archival scan.
    Full,
    /// A quick, low-resolution look; never moves the film.
    Preview,
    /// A tiny scan made only to move the film (`advance`); its data is thrown away.
    Throwaway,
}

/// Color mode and bit depth of a capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorFormat {
    /// Red, green, blue and infrared, 16 bits each: every full scan (Principle I).
    Rgbi16,
    /// Red, green, blue, 8 bits each: previews and throwaway scans.
    Rgb8,
}

/// What one capture asks of the scanner.
#[derive(Debug, Clone, PartialEq)]
pub struct ScanSettings {
    pub kind: ScanKind,
    /// Requested resolution. The PrimeFilm XA ignores it for previews.
    pub resolution_dpi: f64,
    pub color: ColorFormat,
    /// Move the film on by one frame after this capture.
    pub advance_after: bool,
}

/// Size and sample layout of a captured frame (data-model.md, "Frame Layout").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameParams {
    pub pixels_per_line: u32,
    pub lines: u32,
    /// 8 or 16.
    pub bits_per_sample: u16,
    /// 3 (RGB) or 4 (RGB + infrared).
    pub channels: u16,
    pub bytes_per_line: u32,
}

impl FrameParams {
    /// Build from the numbers SANE reports, checking they describe a frame we can save.
    /// The channel count comes from the line length, not SANE's frame format: pieusb reports
    /// RGBI as "RGB" with 4 samples per pixel (research, pieusb findings).
    pub fn from_sane(
        bytes_per_line: i32,
        pixels_per_line: i32,
        lines: i32,
        depth: i32,
    ) -> Result<FrameParams, ScanError> {
        let bad = |what: String| ScanError::ScanFailed {
            frame: None,
            text: format!("unsupported frame from the scanner: {what}"),
        };
        // `u32::try_from` fails for negative numbers, which SANE should never send.
        let (Ok(bytes_per_line), Ok(pixels), Ok(lines)) = (
            u32::try_from(bytes_per_line),
            u32::try_from(pixels_per_line),
            u32::try_from(lines),
        ) else {
            return Err(bad("negative size".to_string()));
        };
        if pixels == 0 || lines == 0 {
            return Err(bad(format!("{pixels} × {lines} pixels")));
        }
        let bits: u16 = match depth {
            8 => 8,
            16 => 16,
            other => return Err(bad(format!("{other} bits per sample"))),
        };
        let bytes_per_sample = u32::from(bits / 8);
        let channels = bytes_per_line / (pixels * bytes_per_sample);
        if !matches!(channels, 3 | 4) || bytes_per_line != pixels * channels * bytes_per_sample {
            return Err(bad(format!(
                "{bytes_per_line} bytes per line for {pixels} pixels at {bits} bits"
            )));
        }
        Ok(FrameParams {
            pixels_per_line: pixels,
            lines,
            bits_per_sample: bits,
            // At most 4, checked above, so this cannot truncate.
            channels: channels as u16,
            bytes_per_line,
        })
    }

    /// Total bytes in the frame.
    pub fn image_bytes(&self) -> u64 {
        u64::from(self.bytes_per_line) * u64::from(self.lines)
    }

    /// Empty statistics for a frame of this layout.
    pub fn new_stats(&self) -> FrameStats {
        FrameStats::new(
            self.pixels_per_line,
            self.lines,
            self.channels,
            self.bits_per_sample,
        )
    }

    /// "RGBI 16-bit" or "RGB 8-bit", for progress lines.
    pub fn describe(&self) -> String {
        let channels = if self.channels == 4 { "RGBI" } else { "RGB" };
        format!("{channels} {}-bit", self.bits_per_sample)
    }
}

/// Pick the `wanted` scanner if given, otherwise the first one found.
pub fn choose_device(
    devices: &[DeviceInfo],
    wanted: Option<&str>,
) -> Result<DeviceInfo, ScanError> {
    // `.iter().find(|d| ...)` walks the list with a closure; see `capability.rs` for how
    // iterators and closures work. `.cloned()` turns the borrowed `&DeviceInfo` it finds into
    // an owned copy we can return, and `.ok_or(..)` turns `None` into an error.
    match wanted {
        Some(name) => devices
            .iter()
            .find(|d| d.name == name)
            .cloned()
            .ok_or(ScanError::NotFound),
        None => {
            let first = devices.first().cloned().ok_or(ScanError::NotFound)?;
            if devices.len() > 1 {
                eprintln!("using first scanner: {}", first.name);
            }
            Ok(first)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgbi_reported_as_rgb_has_four_channels() {
        // pieusb, RGBI at 16 bits: bytes_per_line = 2 × 4 × pixels.
        let p = FrameParams::from_sane(2 * 4 * 5173, 5173, 3443, 16).unwrap();
        assert_eq!(p.channels, 4);
        assert_eq!(p.bits_per_sample, 16);
        assert_eq!(p.image_bytes(), 5173 * 3443 * 8);
        assert_eq!(p.describe(), "RGBI 16-bit");
    }

    #[test]
    fn rgb_8_bit() {
        let p = FrameParams::from_sane(3 * 431, 431, 287, 8).unwrap();
        assert_eq!(p.channels, 3);
        assert_eq!(p.describe(), "RGB 8-bit");
    }

    #[test]
    fn rejects_bad_layouts() {
        assert!(FrameParams::from_sane(431, 431, 287, 8).is_err()); // gray: 1 channel
        assert!(FrameParams::from_sane(3 * 431 + 1, 431, 287, 8).is_err()); // padded line
        assert!(FrameParams::from_sane(54, 431, 287, 1).is_err()); // line art
        assert!(FrameParams::from_sane(0, 0, 287, 8).is_err());
        assert!(FrameParams::from_sane(-3, 1, 1, 8).is_err());
    }
}
