//! A small streaming writer for baseline, uncompressed TIFF files (research D3).
//!
//! A TIFF file is an 8-byte header, the image data, and a "directory" (IFD) of numbered tags
//! describing that data: its size, sample layout, resolution and so on. The header holds the
//! directory's position in the file. Because the data is uncompressed, every strip's position
//! is known in advance, so we can stream pixel data straight from the scanner to disk and
//! write the directory once the data is complete, then go back and fill in the header. Memory
//! use stays small no matter how big the frame is.
//!
//! Supported: 8 or 16 bits per sample; 1 sample (gray, black is zero: the processed B&W
//! positives of feature 003), 3 samples (RGB) or 4 (RGB plus infrared, marked as an
//! "unspecified" extra sample, as VueScan writes RGBI files). Files are limited to 4 GB, the
//! most a classic TIFF can address; the largest preset frame is about 1.1 GB.

use std::io::{self, Seek, SeekFrom, Write};

/// Everything the TIFF tags record about one image.
#[derive(Debug, Clone, PartialEq)]
pub struct TiffLayout {
    pub width: u32,
    pub height: u32,
    /// 1 (gray), 3 (RGB) or 4 (RGB + infrared).
    pub channels: u16,
    /// 8 or 16.
    pub bits: u16,
    /// Resolution recorded in the file, in dots per inch, across and along the frame. Kept
    /// separate so a file with non-square pixels can never be mislabelled.
    pub dpi_x: f64,
    pub dpi_y: f64,
    /// `ImageDescription` tag, e.g. "Roll 2026-11-09, frame 003, preset standard".
    pub description: String,
    /// `Software` tag, e.g. "primefilm 0.1.0".
    pub software: String,
    /// `DateTime` tag, exactly `YYYY:MM:DD HH:MM:SS` as the TIFF standard requires.
    pub datetime: String,
}

impl TiffLayout {
    fn bytes_per_sample(&self) -> u64 {
        u64::from(self.bits / 8)
    }

    fn row_bytes(&self) -> u64 {
        u64::from(self.width) * u64::from(self.channels) * self.bytes_per_sample()
    }

    /// Total size of the pixel data in bytes.
    pub fn image_bytes(&self) -> u64 {
        self.row_bytes() * u64::from(self.height)
    }

    /// Rows per strip: as many whole rows as fit in 64 KiB, at least one. Small strips keep
    /// readers that load one strip at a time light on memory.
    fn rows_per_strip(&self) -> u32 {
        let rows = (64 * 1024 / self.row_bytes().max(1)).max(1);
        // `try_from` checks the conversion instead of silently truncating; a strip can never
        // hold more rows than the image, so fall back to the image height.
        u32::try_from(rows).unwrap_or(self.height).min(self.height)
    }
}

/// Position of the image data in the file: straight after the 8-byte header.
const DATA_OFFSET: u64 = 8;

// TIFF field types used here.
const ASCII: u16 = 2;
const SHORT: u16 = 3;
const LONG: u16 = 4;
const RATIONAL: u16 = 5;

/// One directory entry before it is written: tag number, field type, value count, and the
/// value itself as little-endian bytes.
struct Entry {
    tag: u16,
    field_type: u16,
    count: u32,
    data: Vec<u8>,
}

// Generics again: `W: Write + Seek` accepts anything we can write to and jump around in, such
// as a `File` (or a `BufWriter<File>`) in the program and an in-memory `Cursor<Vec<u8>>` in
// the tests. `Write` and `Seek` are traits from the standard library (plan concept 3).
/// Streams one image into a TIFF file. Create it, [`write`](TiffWriter::write) the sample
/// bytes in any chunk sizes, then [`finish`](TiffWriter::finish).
pub struct TiffWriter<W: Write + Seek> {
    out: W,
    layout: TiffLayout,
    /// Bytes of pixel data written so far.
    written: u64,
    /// With 16-bit samples, a chunk may end halfway through a sample; its first byte waits
    /// here for the next chunk.
    carry: Option<u8>,
    /// Reused buffer for byte-order conversion, so each chunk doesn't allocate.
    scratch: Vec<u8>,
}

impl<W: Write + Seek> TiffWriter<W> {
    /// Check the layout and write the header. `out` should be empty and positioned at 0.
    pub fn new(mut out: W, layout: TiffLayout) -> io::Result<TiffWriter<W>> {
        if !matches!(layout.channels, 1 | 3 | 4) || !matches!(layout.bits, 8 | 16) {
            return Err(invalid(format!(
                "unsupported layout: {} channels, {} bits",
                layout.channels, layout.bits
            )));
        }
        if layout.width == 0 || layout.height == 0 {
            return Err(invalid("image has no pixels".to_string()));
        }
        // Leave a generous 64 KiB for the tags after the data; all offsets must fit in 32 bits.
        if DATA_OFFSET + layout.image_bytes() + 64 * 1024 > u64::from(u32::MAX) {
            return Err(invalid(
                "image too large for a classic TIFF (4 GB)".to_string(),
            ));
        }
        // Byte order: "II" (Intel) means every multi-byte number in the file is little-endian,
        // least significant byte first. 42 identifies the file as TIFF. The last 4 bytes will
        // hold the directory's position; 0 for now, filled in by `finish`.
        out.write_all(b"II")?;
        out.write_all(&42u16.to_le_bytes())?;
        out.write_all(&0u32.to_le_bytes())?;
        Ok(TiffWriter {
            out,
            layout,
            written: 0,
            carry: None,
            scratch: Vec::new(),
        })
    }

    /// Append sample bytes, pixel by pixel (gray, or R, G, B[, I] for each pixel), rows top to
    /// bottom.
    /// 16-bit samples arrive in this computer's native byte order, as SANE delivers them, and
    /// are stored little-endian. Fails if more bytes arrive than the image holds.
    pub fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        let total = self.written + self.carry.map_or(0, |_| 1) + bytes.len() as u64;
        if total > self.layout.image_bytes() {
            return Err(invalid(format!(
                "more data than the image holds ({} bytes)",
                self.layout.image_bytes()
            )));
        }
        if self.layout.bits == 8 {
            self.out.write_all(bytes)?;
            self.written += bytes.len() as u64;
            return Ok(());
        }

        // 16-bit: rebuild each sample from its two bytes in native order (`from_ne_bytes`),
        // then store it as little-endian (`to_le_bytes`). On x86-64 both orders are the same
        // and this is a plain copy, but spelling it out keeps the file correct on any machine.
        self.scratch.clear();
        let mut rest = bytes;
        if let Some(first) = self.carry.take() {
            match rest.split_first() {
                Some((second, tail)) => {
                    let sample = u16::from_ne_bytes([first, *second]);
                    self.scratch.extend_from_slice(&sample.to_le_bytes());
                    rest = tail;
                }
                None => {
                    self.carry = Some(first);
                    return Ok(());
                }
            }
        }
        // `as_chunks::<2>()` views the bytes as a list of 2-byte arrays, plus whatever is left
        // over (at most one byte).
        let (pairs, remainder) = rest.as_chunks::<2>();
        for pair in pairs {
            let sample = u16::from_ne_bytes(*pair);
            self.scratch.extend_from_slice(&sample.to_le_bytes());
        }
        // An odd byte left at the end waits for the next chunk.
        if let [last] = remainder {
            self.carry = Some(*last);
        }
        self.out.write_all(&self.scratch)?;
        self.written += self.scratch.len() as u64;
        Ok(())
    }

    /// Check that the whole image arrived, write the tag directory, point the header at it,
    /// and hand back the output (so the caller can flush and sync it).
    pub fn finish(mut self) -> io::Result<W> {
        if self.carry.is_some() || self.written != self.layout.image_bytes() {
            return Err(invalid(format!(
                "image incomplete: {} of {} bytes",
                self.written,
                self.layout.image_bytes()
            )));
        }
        let entries = self.entries();

        // Values longer than 4 bytes don't fit in a directory entry, so they go after the
        // image data and the entry holds their position instead. TIFF requires positions to
        // be even, hence the padding byte after odd-length values.
        let mut pos = DATA_OFFSET + self.written;
        if pos % 2 == 1 {
            self.out.write_all(&[0])?;
            pos += 1;
        }
        let mut offsets = Vec::with_capacity(entries.len());
        for entry in &entries {
            if entry.data.len() > 4 {
                offsets.push(Some(pos));
                self.out.write_all(&entry.data)?;
                pos += entry.data.len() as u64;
                if pos % 2 == 1 {
                    self.out.write_all(&[0])?;
                    pos += 1;
                }
            } else {
                offsets.push(None);
            }
        }

        // The directory: entry count, 12 bytes per entry (tags in ascending order, as the
        // standard requires), then 0 meaning "no further images in this file".
        let ifd_offset = to_u32(pos)?;
        let count = u16::try_from(entries.len()).map_err(|_| invalid("too many tags".into()))?;
        self.out.write_all(&count.to_le_bytes())?;
        // `zip` walks two lists side by side, pairing each entry with its offset.
        for (entry, offset) in entries.iter().zip(&offsets) {
            self.out.write_all(&entry.tag.to_le_bytes())?;
            self.out.write_all(&entry.field_type.to_le_bytes())?;
            self.out.write_all(&entry.count.to_le_bytes())?;
            match offset {
                Some(at) => self.out.write_all(&to_u32(*at)?.to_le_bytes())?,
                None => {
                    // Short values are stored in the entry itself, left-aligned.
                    let mut inline = [0u8; 4];
                    inline[..entry.data.len()].copy_from_slice(&entry.data);
                    self.out.write_all(&inline)?;
                }
            }
        }
        self.out.write_all(&0u32.to_le_bytes())?;

        // Go back and fill in the directory position in the header.
        self.out.seek(SeekFrom::Start(4))?;
        self.out.write_all(&ifd_offset.to_le_bytes())?;
        self.out.seek(SeekFrom::End(0))?;
        self.out.flush()?;
        Ok(self.out)
    }

    /// The directory entries for this image, in ascending tag order.
    fn entries(&self) -> Vec<Entry> {
        let l = &self.layout;
        let rows_per_strip = l.rows_per_strip();
        let strip_bytes = l.row_bytes() * u64::from(rows_per_strip);
        let strips = l.height.div_ceil(rows_per_strip);
        let mut strip_offsets = Vec::new();
        let mut strip_counts = Vec::new();
        for i in 0..u64::from(strips) {
            let start = i * strip_bytes;
            let len = strip_bytes.min(l.image_bytes() - start);
            // Offsets and lengths fit in 32 bits: `new` checked the whole file does.
            strip_offsets.push((DATA_OFFSET + start) as u32);
            strip_counts.push(len as u32);
        }
        // Resolution as a fraction: hundredths of a dot per inch over 100.
        let dpi_x = (l.dpi_x * 100.0).round() as u32;
        let dpi_y = (l.dpi_y * 100.0).round() as u32;

        // 1 = BlackIsZero (gray, 0 is black), 2 = RGB.
        let photometric = if l.channels == 1 { 1 } else { 2 };
        let mut entries = vec![
            long(256, &[l.width]),                              // ImageWidth
            long(257, &[l.height]),                             // ImageLength
            short(258, &vec![l.bits; usize::from(l.channels)]), // BitsPerSample
            short(259, &[1]),                                   // Compression: none
            short(262, &[photometric]),                         // PhotometricInterpretation
            ascii(270, &l.description),                         // ImageDescription
            long(273, &strip_offsets),                          // StripOffsets
            short(277, &[l.channels]),                          // SamplesPerPixel
            long(278, &[rows_per_strip]),                       // RowsPerStrip
            long(279, &strip_counts),                           // StripByteCounts
            rational(282, dpi_x, 100),                          // XResolution
            rational(283, dpi_y, 100),                          // YResolution
            short(284, &[1]),                                   // PlanarConfiguration: interleaved
            short(296, &[2]),                                   // ResolutionUnit: inch
            ascii(305, &l.software),                            // Software
            ascii(306, &l.datetime),                            // DateTime
        ];
        if l.channels == 4 {
            // ExtraSamples: the 4th sample is "unspecified" data (infrared), not transparency.
            entries.push(short(338, &[0]));
        }
        entries
    }
}

fn short(tag: u16, values: &[u16]) -> Entry {
    Entry {
        tag,
        field_type: SHORT,
        count: values.len() as u32,
        data: values.iter().flat_map(|v| v.to_le_bytes()).collect(),
    }
}

fn long(tag: u16, values: &[u32]) -> Entry {
    Entry {
        tag,
        field_type: LONG,
        count: values.len() as u32,
        data: values.iter().flat_map(|v| v.to_le_bytes()).collect(),
    }
}

fn rational(tag: u16, numerator: u32, denominator: u32) -> Entry {
    let mut data = numerator.to_le_bytes().to_vec();
    data.extend_from_slice(&denominator.to_le_bytes());
    Entry {
        tag,
        field_type: RATIONAL,
        count: 1,
        data,
    }
}

/// A NUL-terminated ASCII string; any non-ASCII character becomes `?`.
fn ascii(tag: u16, text: &str) -> Entry {
    let mut data: Vec<u8> = text
        .chars()
        .map(|c| if c.is_ascii() { c as u8 } else { b'?' })
        .collect();
    data.push(0);
    Entry {
        tag,
        field_type: ASCII,
        count: data.len() as u32,
        data,
    }
}

fn to_u32(pos: u64) -> io::Result<u32> {
    u32::try_from(pos).map_err(|_| invalid("file larger than 4 GB".to_string()))
}

fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tiff::decoder::{Decoder, DecodingResult};
    use tiff::tags::Tag;

    fn layout(width: u32, height: u32, channels: u16, bits: u16) -> TiffLayout {
        TiffLayout {
            width,
            height,
            channels,
            bits,
            dpi_x: 3600.0,
            dpi_y: 3600.0,
            description: "Roll test, frame 001, preset standard".to_string(),
            software: "primefilm test".to_string(),
            datetime: "2026:11:09 12:00:00".to_string(),
        }
    }

    /// Write `samples` (native-endian u16) in deliberately awkward chunk sizes.
    fn write_rgbi16(l: &TiffLayout, samples: &[u16]) -> Vec<u8> {
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_ne_bytes()).collect();
        let mut writer = TiffWriter::new(Cursor::new(Vec::new()), l.clone()).unwrap();
        for chunk in bytes.chunks(7) {
            writer.write(chunk).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn rgbi_16_bit_round_trip() {
        // Tall enough (200 rows of 300 × 8 bytes) to need several strips.
        let l = layout(300, 200, 4, 16);
        let samples: Vec<u16> = (0..300 * 200 * 4).map(|i| (i * 7 % 65536) as u16).collect();
        let file = write_rgbi16(&l, &samples);

        let mut decoder = Decoder::new(Cursor::new(file.clone())).unwrap();
        assert_eq!(decoder.dimensions().unwrap(), (300, 200));
        assert_eq!(decoder.get_tag_u32_vec(Tag::ExtraSamples).unwrap(), vec![0]);
        assert_eq!(decoder.get_tag_u32(Tag::SamplesPerPixel).unwrap(), 4);

        // The decoder treats the "unspecified" 4th sample as an extra it doesn't return (as
        // image viewers do), so check every sample, infrared included, by following its strip
        // offsets and byte counts: the data must be our samples, little-endian, in order.
        let offsets = decoder.get_tag_u32_vec(Tag::StripOffsets).unwrap();
        let counts = decoder.get_tag_u32_vec(Tag::StripByteCounts).unwrap();
        assert!(offsets.len() > 1, "expected several strips");
        let mut raw = Vec::new();
        for (offset, count) in offsets.iter().zip(&counts) {
            raw.extend_from_slice(&file[*offset as usize..(*offset + *count) as usize]);
        }
        let expected: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        assert_eq!(raw, expected);
        assert_eq!(
            decoder.get_tag_u32_vec(Tag::BitsPerSample).unwrap(),
            vec![16; 4]
        );
        assert_eq!(
            decoder.get_tag_ascii_string(Tag::Software).unwrap(),
            "primefilm test"
        );
        let x_res = decoder.get_tag_u32_vec(Tag::XResolution).unwrap();
        assert_eq!(x_res, vec![360000, 100]);
        assert_eq!(decoder.get_tag_u32(Tag::ResolutionUnit).unwrap(), 2);
        // What a viewer sees: the R, G, B samples of every pixel.
        let rgb: Vec<u16> = samples.chunks(4).flat_map(|px| px[..3].to_vec()).collect();
        match decoder.read_image().unwrap() {
            DecodingResult::U16(pixels) => {
                assert!(pixels == rgb || pixels == samples, "pixel data differs")
            }
            _ => panic!("expected 16-bit samples"),
        }
    }

    #[test]
    fn rgb_8_bit_round_trip() {
        let l = layout(31, 17, 3, 8);
        let samples: Vec<u8> = (0..31 * 17 * 3).map(|i| (i % 251) as u8).collect();
        let mut writer = TiffWriter::new(Cursor::new(Vec::new()), l).unwrap();
        writer.write(&samples).unwrap();
        let file = writer.finish().unwrap().into_inner();

        let mut decoder = Decoder::new(Cursor::new(file)).unwrap();
        assert_eq!(decoder.dimensions().unwrap(), (31, 17));
        assert!(decoder.get_tag(Tag::ExtraSamples).is_err());
        match decoder.read_image().unwrap() {
            DecodingResult::U8(pixels) => assert_eq!(pixels, samples),
            other => panic!("expected 8-bit samples, got {other:?}"),
        }
    }

    #[test]
    fn gray_round_trips() {
        let l = layout(53, 41, 1, 16);
        let samples: Vec<u16> = (0..53 * 41).map(|i| (i * 31 % 65536) as u16).collect();
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_ne_bytes()).collect();
        let mut writer = TiffWriter::new(Cursor::new(Vec::new()), l).unwrap();
        for chunk in bytes.chunks(5) {
            writer.write(chunk).unwrap();
        }
        let file = writer.finish().unwrap().into_inner();
        let mut decoder = Decoder::new(Cursor::new(file)).unwrap();
        assert_eq!(
            decoder.get_tag_u32(Tag::PhotometricInterpretation).unwrap(),
            1
        );
        assert_eq!(decoder.get_tag_u32(Tag::SamplesPerPixel).unwrap(), 1);
        match decoder.read_image().unwrap() {
            DecodingResult::U16(pixels) => assert_eq!(pixels, samples),
            other => panic!("expected 16-bit gray, got {other:?}"),
        }

        let l = layout(9, 7, 1, 8);
        let samples: Vec<u8> = (0..63).collect();
        let mut writer = TiffWriter::new(Cursor::new(Vec::new()), l).unwrap();
        writer.write(&samples).unwrap();
        let file = writer.finish().unwrap().into_inner();
        let mut decoder = Decoder::new(Cursor::new(file)).unwrap();
        match decoder.read_image().unwrap() {
            DecodingResult::U8(pixels) => assert_eq!(pixels, samples),
            other => panic!("expected 8-bit gray, got {other:?}"),
        }
    }

    #[test]
    fn resolution_is_recorded_per_side() {
        let mut l = layout(4, 4, 3, 8);
        l.dpi_y = 7200.0;
        let mut writer = TiffWriter::new(Cursor::new(Vec::new()), l).unwrap();
        writer.write(&[0; 4 * 4 * 3]).unwrap();
        let file = writer.finish().unwrap().into_inner();
        let mut decoder = Decoder::new(Cursor::new(file)).unwrap();
        assert_eq!(
            decoder.get_tag_u32_vec(Tag::XResolution).unwrap(),
            vec![360000, 100]
        );
        assert_eq!(
            decoder.get_tag_u32_vec(Tag::YResolution).unwrap(),
            vec![720000, 100]
        );
    }

    #[test]
    fn finish_rejects_short_image() {
        let mut writer = TiffWriter::new(Cursor::new(Vec::new()), layout(4, 4, 3, 8)).unwrap();
        writer.write(&[0; 10]).unwrap();
        assert!(writer.finish().is_err());
    }

    #[test]
    fn write_rejects_too_much_data() {
        let mut writer = TiffWriter::new(Cursor::new(Vec::new()), layout(2, 2, 3, 8)).unwrap();
        assert!(writer.write(&[0; 13]).is_err());
    }

    #[test]
    fn rejects_unsupported_layouts() {
        assert!(TiffWriter::new(Cursor::new(Vec::new()), layout(2, 2, 2, 8)).is_err());
        assert!(TiffWriter::new(Cursor::new(Vec::new()), layout(2, 2, 5, 8)).is_err());
        assert!(TiffWriter::new(Cursor::new(Vec::new()), layout(2, 2, 3, 12)).is_err());
        assert!(TiffWriter::new(Cursor::new(Vec::new()), layout(0, 2, 3, 8)).is_err());
    }
}
