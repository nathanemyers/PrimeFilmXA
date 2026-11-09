//! The share copy: a full-resolution 8-bit gray JPEG of the positive (research P10, P13).

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use jpeg_encoder::{ColorType, Encoder};

use crate::error::ScanError;
use crate::gray::{GrayFrame, to_8bit};
use crate::output::PartialFile;

/// JPEG quality, 1–100. 90 keeps grain and fine detail with no visible artifacts.
const QUALITY: u8 = 90;

/// Write `<stem>.jpg` (as a hidden `.partial` file, published later with the frame's other
/// files) from the processed positive in `frame`.
pub fn write_share_copy(
    dir: &Path,
    stem: &str,
    frame: &GrayFrame,
) -> Result<(PartialFile, File), ScanError> {
    let (partial, file) = PartialFile::create(dir, &format!("{stem}.jpg"))?;
    let error = |reason: String| ScanError::OutputError {
        path: partial.target().display().to_string(),
        reason,
    };
    // JPEG sizes are 16-bit numbers; the largest preset frame (7185 × 4783) fits easily.
    let (Ok(width), Ok(height)) = (u16::try_from(frame.width), u16::try_from(frame.height)) else {
        return Err(error("frame too large for JPEG".to_string()));
    };
    let gray: Vec<u8> = frame.pixels.iter().map(|&v| to_8bit(v)).collect();
    let mut out = BufWriter::new(file);
    // The encoder's builder-style API: create it with the output and quality, then `encode`
    // consumes it. Its own error type is turned into ours with `map_err`.
    Encoder::new(&mut out, QUALITY)
        .encode(&gray, width, height, ColorType::Luma)
        .map_err(|e| error(e.to_string()))?;
    let file = out.into_inner().map_err(|e| error(e.error().to_string()))?;
    Ok((partial, file))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn temp_dir() -> std::path::PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("primefilm-jpeg-test-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Width and height from the SOF0 (baseline frame) header: FF C0, length, precision,
    /// height, width, components.
    fn sof0(bytes: &[u8]) -> (u16, u16, u8) {
        let at = bytes
            .windows(2)
            .position(|w| w == [0xFF, 0xC0])
            .expect("SOF0 marker");
        let h = u16::from_be_bytes([bytes[at + 5], bytes[at + 6]]);
        let w = u16::from_be_bytes([bytes[at + 7], bytes[at + 8]]);
        (w, h, bytes[at + 9])
    }

    #[test]
    fn writes_a_gray_jpeg() {
        let dir = temp_dir();
        let pixels: Vec<u16> = (0..64 * 32).map(|i| (i % 64 * 1000) as u16).collect();
        let frame = GrayFrame::from_pixels(64, 32, 300.0, pixels);
        let (partial, file) = write_share_copy(&dir, "x-001", &frame).unwrap();
        let path = partial.publish(file).unwrap();
        assert!(path.ends_with("x-001.jpg"));
        let bytes = fs::read(&path).unwrap();
        assert_eq!(&bytes[..2], &[0xFF, 0xD8]);
        assert_eq!(&bytes[bytes.len() - 2..], &[0xFF, 0xD9]);
        assert_eq!(sof0(&bytes), (64, 32, 1));
    }
}
