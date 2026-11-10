//! The gray frame held in memory while a B&W frame is processed (research P1, P2).
//!
//! Silver is spectrally neutral, so on B&W film the red, green and blue channels hold the same
//! image; averaging them keeps all three channels' signal and leaves no color cast. The
//! infrared channel, if the capture has one, shows the silver as well and is skipped.

use crate::backend::FrameParams;
use crate::error::ScanError;

/// One frame as 16-bit gray values, built chunk by chunk as the scanner delivers it, then
/// processed in place into the positive.
#[derive(Debug, Clone, PartialEq)]
pub struct GrayFrame {
    pub width: u32,
    pub height: u32,
    /// Resolution across the frame, used to turn mm into pixels.
    pub dpi: f64,
    // A flat `Vec<u16>` used as a 2-D image: the pixel at column `x`, row `y` is at index
    // `y * width + x` ("row-major" order, one row after another). One allocation for the whole
    // frame, and rows are simple slices of it.
    /// Gray values, row-major: raw gray while capturing, the positive after processing.
    pub pixels: Vec<u16>,
    /// Bytes per sample of the capture (1 or 2) and samples per pixel (3 or 4).
    sample_bytes: usize,
    channels: usize,
    /// Bytes of an incomplete pixel left over from the previous chunk.
    leftover: Vec<u8>,
}

impl GrayFrame {
    /// An empty frame for a capture of this layout, with room for every pixel.
    pub fn new(params: &FrameParams, dpi: f64) -> Result<GrayFrame, ScanError> {
        let too_big = || ScanError::ScanFailed {
            frame: None,
            text: format!(
                "frame of {} × {} pixels is too large to process",
                params.pixels_per_line, params.lines
            ),
        };
        // `checked_mul` returns `None` instead of overflowing; `usize::try_from` refuses a
        // value that does not fit, rather than silently cutting it short.
        let count = u64::from(params.pixels_per_line)
            .checked_mul(u64::from(params.lines))
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(too_big)?;
        if count == 0 {
            return Err(ScanError::ScanFailed {
                frame: None,
                text: "the scanner sent an empty frame".to_string(),
            });
        }
        Ok(GrayFrame {
            width: params.pixels_per_line,
            height: params.lines,
            dpi,
            pixels: Vec::with_capacity(count),
            sample_bytes: usize::from(params.bits_per_sample / 8),
            channels: usize::from(params.channels),
            leftover: Vec::new(),
        })
    }

    /// A finished frame from gray values (for tests and the tuning example).
    pub fn from_pixels(width: u32, height: u32, dpi: f64, pixels: Vec<u16>) -> GrayFrame {
        GrayFrame {
            width,
            height,
            dpi,
            pixels,
            sample_bytes: 2,
            channels: 1,
            leftover: Vec::new(),
        }
    }

    fn pixel_count(&self) -> usize {
        self.width as usize * self.height as usize
    }

    /// True once every pixel has arrived.
    pub fn is_complete(&self) -> bool {
        self.pixels.len() == self.pixel_count() && self.leftover.is_empty()
    }

    /// Add capture bytes: 8- or 16-bit samples in this computer's native byte order, 3 or 4
    /// per pixel, as SANE delivers them. A chunk may end partway through a pixel; the rest
    /// waits for the next chunk.
    pub fn push(&mut self, bytes: &[u8]) -> Result<(), ScanError> {
        let pixel_bytes = self.sample_bytes * self.channels;
        let mut rest = bytes;
        if !self.leftover.is_empty() {
            let need = (pixel_bytes - self.leftover.len()).min(rest.len());
            self.leftover.extend_from_slice(&rest[..need]);
            rest = &rest[need..];
            if self.leftover.len() < pixel_bytes {
                return Ok(());
            }
            // `std::mem::take` moves the buffer out, leaving an empty one in its place, so we
            // can read it while calling `&mut self` methods.
            let pixel = std::mem::take(&mut self.leftover);
            self.add_pixel(&pixel)?;
        }
        // `chunks_exact` yields whole pixels only; `remainder` is whatever is left at the end.
        let pixels = rest.chunks_exact(pixel_bytes);
        let remainder = pixels.remainder();
        for pixel in pixels {
            self.add_pixel(pixel)?;
        }
        self.leftover.extend_from_slice(remainder);
        Ok(())
    }

    fn add_pixel(&mut self, pixel: &[u8]) -> Result<(), ScanError> {
        if self.pixels.len() == self.pixel_count() {
            return Err(ScanError::ScanFailed {
                frame: None,
                text: "the scanner sent more pixels than the frame holds".to_string(),
            });
        }
        let sample = |i: usize| -> u32 {
            if self.sample_bytes == 2 {
                u32::from(u16::from_ne_bytes([pixel[2 * i], pixel[2 * i + 1]]))
            } else {
                // 255 × 257 = 65535: an 8-bit value scaled to the same range as 16-bit ones.
                u32::from(pixel[i]) * 257
            }
        };
        // Red, green and blue only; a 4th (infrared) sample is skipped. The sum of three
        // 16-bit values fits easily in a `u32`, and the average fits back in a `u16`.
        let gray = (sample(0) + sample(1) + sample(2)) / 3;
        self.pixels.push(gray as u16);
        Ok(())
    }

    /// Row `y` of the frame.
    pub fn row(&self, y: u32) -> &[u16] {
        let start = y as usize * self.width as usize;
        &self.pixels[start..start + self.width as usize]
    }
}

/// A 16-bit value rounded to 8 bits (for previews and the share copy).
pub fn to_8bit(value: u16) -> u8 {
    // 65535 / 257 = 255 exactly; adding half of 257 first rounds to the nearest value.
    ((u32::from(value) + 128) / 257) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(width: u32, height: u32, channels: u16, bits: u16) -> FrameParams {
        let bytes_per_line = width * u32::from(channels) * u32::from(bits / 8);
        FrameParams::from_sane(
            bytes_per_line as i32,
            width as i32,
            height as i32,
            i32::from(bits),
        )
        .unwrap()
    }

    fn rgbi16_bytes(pixels: &[[u16; 4]]) -> Vec<u8> {
        pixels
            .iter()
            .flat_map(|px| px.iter().flat_map(|s| s.to_ne_bytes()))
            .collect()
    }

    #[test]
    fn averages_rgb_and_ignores_infrared() {
        let mut frame = GrayFrame::new(&params(2, 1, 4, 16), 3600.0).unwrap();
        frame
            .push(&rgbi16_bytes(&[[300, 600, 900, 65535], [0, 0, 3, 0]]))
            .unwrap();
        assert!(frame.is_complete());
        assert_eq!(frame.pixels, vec![600, 1]);
    }

    #[test]
    fn rgb16_and_rgb8() {
        let mut frame = GrayFrame::new(&params(1, 1, 3, 16), 3600.0).unwrap();
        let bytes: Vec<u8> = [10u16, 20, 30]
            .iter()
            .flat_map(|s| s.to_ne_bytes())
            .collect();
        frame.push(&bytes).unwrap();
        assert_eq!(frame.pixels, vec![20]);

        let mut frame = GrayFrame::new(&params(2, 1, 3, 8), 300.0).unwrap();
        frame.push(&[255, 255, 255, 0, 3, 6]).unwrap();
        assert_eq!(frame.pixels, vec![65535, 3 * 257]);
    }

    #[test]
    fn chunks_split_anywhere_give_the_same_frame() {
        let pixels: Vec<[u16; 4]> = (0..35u16)
            .map(|i| [i * 100, i * 200 + 7, i * 300 + 1, 999])
            .collect();
        let bytes = rgbi16_bytes(&pixels);
        let mut whole = GrayFrame::new(&params(7, 5, 4, 16), 3600.0).unwrap();
        whole.push(&bytes).unwrap();
        for size in [1, 3, 7, 8, 13] {
            let mut split = GrayFrame::new(&params(7, 5, 4, 16), 3600.0).unwrap();
            for chunk in bytes.chunks(size) {
                split.push(chunk).unwrap();
            }
            assert!(split.is_complete(), "chunk size {size}");
            assert_eq!(split.pixels, whole.pixels, "chunk size {size}");
        }
    }

    #[test]
    fn incomplete_and_overfull_frames() {
        let mut frame = GrayFrame::new(&params(2, 1, 3, 8), 300.0).unwrap();
        frame.push(&[1, 2, 3, 4]).unwrap();
        assert!(!frame.is_complete());
        frame.push(&[5, 6]).unwrap();
        assert!(frame.is_complete());
        assert!(frame.push(&[7, 8, 9]).is_err());
    }

    #[test]
    fn rounding_to_8_bits() {
        assert_eq!(to_8bit(0), 0);
        assert_eq!(to_8bit(65535), 255);
        assert_eq!(to_8bit(257 * 100), 100);
        assert_eq!(to_8bit(257 * 100 + 129), 101);
    }

    #[test]
    fn rows() {
        let frame = GrayFrame::from_pixels(3, 2, 300.0, vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(frame.row(1), &[4, 5, 6]);
    }
}
