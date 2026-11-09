//! Statistics gathered while a frame streams to disk, used to recognize "no film" and "the
//! film did not move" from the image itself (research D4).
//!
//! The PrimeFilm XA reports neither: with no film it scans the bare light source, and at the
//! end of a strip it silently stays on the last frame (hardware experiments E4, E5).

/// A sample counts as "bright" from this fraction of full scale. The bare light source
/// measured about 0.80 (channel means 205/199/206 of 255, E5); film frames averaged 0.06.
pub const NEAR_FULL: f64 = 0.5;

/// A frame is an empty gate when at least this fraction of every color channel's samples is
/// bright. E5's empty frame had only a few dark dust specks.
pub const EMPTY_FRACTION: f64 = 0.95;

/// Two fingerprints closer than this (mean absolute difference, as a fraction of full scale)
/// are the same physical frame. Rescans of one frame differed by at most 1.7/255 (0.007) per
/// pixel, less after averaging over a cell; neighbouring frames by 8/255 (0.03) or more
/// (E2–E4).
pub const SAME_FRAME_DIFF: f64 = 0.01;

/// The fingerprint divides the frame into GRID × GRID cells.
const GRID: usize = 16;

/// Number of color channels looked at; the infrared channel, if any, is ignored.
const COLORS: usize = 3;

/// A coarse summary of a frame: the average of each color channel in each cell of a 16 × 16
/// grid, as a fraction of full scale. Small enough to keep for the previous frame (768
/// numbers) instead of the frame itself (up to 1.1 GB).
#[derive(Debug, Clone, PartialEq)]
pub struct Fingerprint(Vec<f64>);

impl Fingerprint {
    /// True when `self` and `other` look like the same physical frame.
    pub fn same_frame(&self, other: &Fingerprint) -> bool {
        if self.0.len() != other.0.len() || self.0.is_empty() {
            return false;
        }
        let total: f64 = self
            .0
            .iter()
            .zip(&other.0)
            .map(|(a, b)| (a - b).abs())
            .sum();
        total / (self.0.len() as f64) < SAME_FRAME_DIFF
    }
}

/// Collects statistics from a frame's sample bytes, in any chunk sizes.
pub struct FrameStats {
    width: usize,
    height: usize,
    channels: usize,
    bits: u16,
    /// Full-scale value: 255 or 65535.
    full: f64,
    /// Position of the next sample: channel within the pixel, column, row.
    channel: usize,
    x: usize,
    y: usize,
    /// First byte of a 16-bit sample split across two chunks.
    carry: Option<u8>,
    /// Per color channel: how many samples were bright, out of how many.
    bright: [u64; COLORS],
    samples: [u64; COLORS],
    /// Per cell and color channel: sum of sample values, and how many pixels the cell got.
    cell_sums: Vec<f64>,
    cell_pixels: Vec<u64>,
}

impl FrameStats {
    /// `channels` is 3 or 4 (the 4th, infrared, is skipped); `bits` is 8 or 16.
    pub fn new(width: u32, height: u32, channels: u16, bits: u16) -> FrameStats {
        let full = if bits == 16 { 65535.0 } else { 255.0 };
        FrameStats {
            width: width as usize,
            height: height as usize,
            channels: usize::from(channels),
            bits,
            full,
            channel: 0,
            x: 0,
            y: 0,
            carry: None,
            bright: [0; COLORS],
            samples: [0; COLORS],
            cell_sums: vec![0.0; GRID * GRID * COLORS],
            cell_pixels: vec![0; GRID * GRID],
        }
    }

    /// Take in the next chunk of sample bytes (16-bit samples in native byte order, as SANE
    /// delivers them).
    pub fn update(&mut self, bytes: &[u8]) {
        if self.bits == 8 {
            for &b in bytes {
                self.sample(f64::from(b));
            }
            return;
        }
        let mut rest = bytes;
        if let Some(first) = self.carry.take() {
            match rest.split_first() {
                Some((second, tail)) => {
                    self.sample(f64::from(u16::from_ne_bytes([first, *second])));
                    rest = tail;
                }
                None => {
                    self.carry = Some(first);
                    return;
                }
            }
        }
        let (pairs, remainder) = rest.as_chunks::<2>();
        for pair in pairs {
            self.sample(f64::from(u16::from_ne_bytes(*pair)));
        }
        if let [last] = remainder {
            self.carry = Some(*last);
        }
    }

    /// Record one sample and move to the next position.
    fn sample(&mut self, value: f64) {
        if self.y >= self.height || self.width == 0 {
            return; // more data than the frame holds; the TIFF writer reports that
        }
        let c = self.channel;
        if c < COLORS {
            self.samples[c] += 1;
            if value >= NEAR_FULL * self.full {
                self.bright[c] += 1;
            }
            let cell = self.cell();
            self.cell_sums[cell * COLORS + c] += value;
            if c == 0 {
                self.cell_pixels[cell] += 1;
            }
        }
        self.channel += 1;
        if self.channel == self.channels {
            self.channel = 0;
            self.x += 1;
            if self.x == self.width {
                self.x = 0;
                self.y += 1;
            }
        }
    }

    /// Index of the grid cell holding the current pixel.
    fn cell(&self) -> usize {
        let row = self.y * GRID / self.height;
        let col = self.x * GRID / self.width;
        row * GRID + col
    }

    /// True when the frame shows the bare light source, not film.
    pub fn looks_empty(&self) -> bool {
        (0..COLORS).all(|c| {
            self.samples[c] > 0
                && (self.bright[c] as f64) / (self.samples[c] as f64) >= EMPTY_FRACTION
        })
    }

    pub fn fingerprint(&self) -> Fingerprint {
        let values = self
            .cell_sums
            .iter()
            .enumerate()
            .map(|(i, sum)| match self.cell_pixels[i / COLORS] {
                // A frame narrower or shorter than the grid leaves some cells empty.
                0 => 0.0,
                n => sum / (n as f64) / self.full,
            })
            .collect();
        Fingerprint(values)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: u32 = 40;
    const H: u32 = 30;

    /// An RGB 8-bit frame whose pixel at (x, y) is `color(x, y)`.
    fn frame8(color: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for y in 0..H {
            for x in 0..W {
                bytes.extend_from_slice(&color(x, y));
            }
        }
        bytes
    }

    fn stats8(bytes: &[u8]) -> FrameStats {
        let mut stats = FrameStats::new(W, H, 3, 8);
        stats.update(bytes);
        stats
    }

    #[test]
    fn light_source_is_empty() {
        // As measured in E5: about 205/199/206 of 255, with a few dark dust specks.
        let bytes = frame8(|x, y| {
            if x == 3 && y < 5 {
                [30, 25, 30]
            } else {
                [205, 199, 206]
            }
        });
        assert!(stats8(&bytes).looks_empty());
    }

    #[test]
    fn film_is_not_empty() {
        let black = frame8(|_, _| [10, 10, 10]);
        let orange_base = frame8(|_, _| [200, 120, 60]);
        let gradient = frame8(|x, y| [(x * 6) as u8, (y * 8) as u8, 128]);
        for bytes in [black, orange_base, gradient] {
            assert!(!stats8(&bytes).looks_empty());
        }
    }

    #[test]
    fn chunk_size_does_not_matter() {
        let l = (W * H * 4) as usize;
        let samples: Vec<u16> = (0..l).map(|i| (i * 97 % 65536) as u16).collect();
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_ne_bytes()).collect();

        let mut whole = FrameStats::new(W, H, 4, 16);
        whole.update(&bytes);
        let mut pieces = FrameStats::new(W, H, 4, 16);
        for chunk in bytes.chunks(13) {
            pieces.update(chunk);
        }
        assert_eq!(whole.fingerprint(), pieces.fingerprint());
        assert_eq!(whole.looks_empty(), pieces.looks_empty());
    }

    #[test]
    fn noise_is_the_same_frame() {
        let base = |x: u32, y: u32| [(x * 6) as u8, (y * 8) as u8, 100];
        let a = stats8(&frame8(base)).fingerprint();
        // About ±1% noise, alternating, like two scans of one frame.
        let b = stats8(&frame8(|x, y| {
            let [r, g, bl] = base(x, y);
            if (x + y) % 2 == 0 {
                [r.saturating_add(2), g.saturating_add(2), bl + 2]
            } else {
                [r.saturating_sub(2), g.saturating_sub(2), bl - 2]
            }
        }))
        .fingerprint();
        assert!(a.same_frame(&b));
    }

    #[test]
    fn different_frames_differ() {
        let a = stats8(&frame8(|x, y| [(x * 6) as u8, (y * 8) as u8, 100])).fingerprint();
        let b = stats8(&frame8(|x, y| [(y * 8) as u8, (x * 6) as u8, 60])).fingerprint();
        assert!(!a.same_frame(&b));
        // Even two plain frames 8/255 apart (E3's closest neighbours) are told apart.
        let c = stats8(&frame8(|_, _| [20, 20, 20])).fingerprint();
        let d = stats8(&frame8(|_, _| [28, 28, 28])).fingerprint();
        assert!(!c.same_frame(&d));
    }

    #[test]
    fn tiny_frames_work() {
        // Smaller than the 16 × 16 grid, like a 25 dpi `advance` scan's neighbours.
        let mut stats = FrameStats::new(5, 4, 3, 8);
        stats.update(&[100; 5 * 4 * 3]);
        assert!(stats.fingerprint().same_frame(&stats.fingerprint()));
    }
}
