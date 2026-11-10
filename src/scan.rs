//! Single-frame, roll, preview and advance sequencing over a scan session (research D1).
//!
//! Everything here is written against the [`ScannerBackend`] and [`ScanSession`] traits, so the
//! same code drives the real scanner, SANE's test backend and the simulated film strip.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use serde::Serialize;

use crate::backend::{
    ColorFormat, DeviceInfo, FrameParams, ScanKind, ScanSession, ScanSettings, ScannerBackend,
};
use crate::capability::Capability;
use crate::error::{ScanError, frame_text};
use crate::film::{DustConstants, DustLevel, FilmStock, Profile};
use crate::frame_stats::{Fingerprint, FrameStats};
use crate::gray::{GrayFrame, to_8bit};
use crate::output::{self, MAX_FRAME, PartialFile, RollFolder};
use crate::preset::{MIN_DPI, Quality, SCAN_AREA_MM};
use crate::process::{self, ProcessSettings, ProcessedFrame};
use crate::report::Logger;
use crate::tiff::{TiffLayout, TiffWriter};
use crate::tone::TonePoints;

/// Bytes read from the scanner at a time. One buffer of this size is reused for every chunk
/// of every frame, so the reading itself does not grow with the frame size.
const READ_CHUNK: usize = 1 << 20;

/// Version of the settings record, embedded as JSON in each TIFF's `ImageDescription` tag.
/// 2: capture plus processing, no sidecar file (feature 003, data-model "Settings Record").
const RECORD_VERSION: u32 = 2;

/// Capture format for full B&W scans. Infrared is useless on silver film; experiment E9
/// (research P3) decides whether RGB 16-bit is faster than RGBI. Until it has run on the
/// scanner, RGBI is kept and its infrared samples are ignored.
const BW_CAPTURE: ColorFormat = ColorFormat::Rgbi16;

/// Resolution of previews, including roll previews. An ordinary 300 dpi scan takes the same
/// 12 s as the scanner's own preview mode (002 experiment E6), and unlike preview mode it can
/// move the film on afterwards.
const PREVIEW_DPI: f64 = 300.0;

/// Choices shared by the scanning commands.
#[derive(Debug, Clone)]
pub struct RunOptions {
    /// Output root folder.
    pub output: PathBuf,
    /// True when the owner gave `--output` (so resume commands repeat it).
    pub output_given: bool,
    /// `--device`, if given.
    pub device: Option<String>,
    /// Resolution for full scans.
    pub quality: Quality,
    /// True when the owner chose the quality (so resume commands repeat it).
    pub quality_given: bool,
    /// How to turn each frame into a positive.
    pub process: ProcessSettings,
    /// The processing options the owner gave, as command-line words, repeated in resume
    /// commands (e.g. `["--film", "hp5"]`).
    pub process_flags: Vec<String>,
    /// Hidden `--keep-capture DIR`: also save each raw capture there (research P12).
    pub keep_capture: Option<PathBuf>,
    /// `roll --preview`: scan the roll at preview speed, saving preview images.
    pub roll_preview: bool,
}

/// Which command produced a [`RunReport`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunKind {
    Scan,
    Roll,
    Preview,
    Advance,
}

/// Whether the film moved after a capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FilmMoved {
    Advanced,
    None,
}

/// One frame saved by a run.
#[derive(Debug, Clone, Serialize)]
pub struct SavedFile {
    /// Absent for previews.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame: Option<u32>,
    /// The positive (16-bit gray; previews 8-bit).
    pub image: PathBuf,
    pub width: u32,
    pub height: u32,
    /// Always "Gray".
    pub channels: &'static str,
    pub bits: u16,
    pub film: FilmStock,
    pub probably_blank: bool,
    pub dust_repaired: u32,
    /// Capture, processing and writing.
    pub seconds: f64,
    pub film_moved: FilmMoved,
}

/// How a run ended once the scanner was open.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Finished as asked; the message is for the summary, e.g. "Film advanced one frame."
    Completed(Option<String>),
    /// A roll reached the end of the film or its frame limit, or `advance` found the film on
    /// its last frame. A success.
    EndOfFilm(String),
    /// Stopped by an error or Ctrl-C; any files saved before it are kept.
    Failed(ScanError),
}

/// Everything the report needs about a run that got as far as opening the scanner.
#[derive(Debug, Clone)]
pub struct RunReport {
    pub kind: RunKind,
    pub roll: Option<String>,
    pub folder: Option<PathBuf>,
    pub quality: Option<Quality>,
    /// The film stock processed (absent for `advance`).
    pub film: Option<FilmStock>,
    pub files: Vec<SavedFile>,
    pub outcome: Outcome,
    /// For rolls: the frame number to resume with.
    pub next_frame: Option<u32>,
    /// Total time from opening the scanner to the end of the run.
    pub seconds: f64,
}

impl RunReport {
    fn new(kind: RunKind) -> RunReport {
        RunReport {
            kind,
            roll: None,
            folder: None,
            quality: None,
            film: None,
            files: Vec::new(),
            outcome: Outcome::Completed(None),
            next_frame: None,
            seconds: 0.0,
        }
    }
}

/// The settings record embedded in every image's `ImageDescription` tag (research D10,
/// data-model "Settings Record"). There is no sidecar file: the record travels with the image,
/// as constitution Principle I requires. The top-level fields describe the capture.
#[derive(Debug, Serialize)]
pub struct ScanRecord {
    pub record_version: u32,
    pub software: String,
    /// Local time with UTC offset, e.g. `2026-11-09T11:26:15-05:00`.
    pub timestamp: String,
    pub scanner: DeviceInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roll: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame: Option<u32>,
    /// `web`, `standard`, `high`, `custom` or `preview`.
    pub preset: String,
    /// What was asked for (`None` for previews, which use the scanner's own resolution).
    pub requested_resolution_dpi: Option<f64>,
    /// Worked out from the pixel width and the scan area.
    pub resolution_dpi: f64,
    /// Of the capture: `RGBI` or `RGB`.
    pub color_mode: &'static str,
    pub bits_per_sample: u16,
    pub channel_order: &'static str,
    pub width: u32,
    pub height: u32,
    pub seconds: f64,
    /// Whether this capture asked the scanner to move the film on afterwards.
    pub advance_requested: bool,
    /// Every scanner option as read after the capture, including exposure, gain and offset.
    pub options: Vec<Capability>,
    /// The image this record is embedded in.
    pub output: OutputRecord,
    /// How the capture was turned into a positive.
    pub processing: ProcessingRecord,
}

#[derive(Debug, Serialize)]
pub struct OutputRecord {
    /// `positive-master` or `preview`.
    pub kind: &'static str,
    pub channels: &'static str,
    pub bits_per_sample: u16,
}

#[derive(Debug, Serialize)]
pub struct ProcessingRecord {
    pub film_type: &'static str,
    pub film_stock: FilmStock,
    pub profile: Profile,
    pub points: TonePoints,
    pub probably_blank: bool,
    pub dust: DustRecord,
    pub seconds: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kept_capture: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
pub struct DustRecord {
    pub level: DustLevel,
    // `flatten` writes the constants' fields directly inside this object; with `None` (dust
    // removal off) it writes nothing.
    #[serde(flatten)]
    pub constants: Option<DustConstants>,
    pub repaired: u32,
}

/// A frame captured from the scanner, still in memory.
struct Captured {
    params: FrameParams,
    stats: FrameStats,
    seconds: f64,
    /// The gray frame, when one was asked for.
    gray: Option<GrayFrame>,
    /// The raw capture written for `--keep-capture`, not yet published.
    kept: Option<(PartialFile, File)>,
    /// Where that raw capture was published.
    kept_path: Option<PathBuf>,
}

/// What to name and how to tag an image.
struct FrameTarget<'a> {
    dir: &'a Path,
    stem: &'a str,
    description: String,
}

/// Capture one frame: start it, stream every byte into the statistics and (if `gray`) a gray
/// frame, and into a raw TIFF if `keep` is given, then end the frame. On any error a raw
/// `.partial` file is dropped, which deletes it.
fn capture<S: ScanSession>(
    session: &mut S,
    gray: bool,
    keep: Option<&FrameTarget>,
    cancel: &AtomicBool,
) -> Result<Captured, ScanError> {
    let started = Instant::now();
    let params = session.start_frame(cancel)?;
    let result = stream(session, &params, gray, keep);
    // `end_frame` runs whether streaming worked or not.
    session.end_frame();
    let (stats, gray, kept) = result?;
    Ok(Captured {
        params,
        stats,
        seconds: started.elapsed().as_secs_f64(),
        gray,
        kept,
        kept_path: None,
    })
}

/// What [`stream`] collects from one frame.
type Streamed = (FrameStats, Option<GrayFrame>, Option<(PartialFile, File)>);

/// The part of [`capture`] between starting and ending the frame.
fn stream<S: ScanSession>(
    session: &mut S,
    params: &FrameParams,
    gray: bool,
    keep: Option<&FrameTarget>,
) -> Result<Streamed, ScanError> {
    let mut stats = params.new_stats();
    let mut frame = if gray {
        Some(GrayFrame::new(
            params,
            dpi_along(params.pixels_per_line, SCAN_AREA_MM.0),
        )?)
    } else {
        None
    };
    let mut writer = match keep {
        Some(t) => {
            let (partial, file) = PartialFile::create(t.dir, &format!("{}.tif", t.stem))?;
            let layout = TiffLayout {
                width: params.pixels_per_line,
                height: params.lines,
                channels: params.channels,
                bits: params.bits_per_sample,
                // From the pixels actually delivered, along each side, rather than from what
                // was asked for: the file's resolution tags then always match its pixels.
                dpi_x: dpi_along(params.pixels_per_line, SCAN_AREA_MM.0),
                dpi_y: dpi_along(params.lines, SCAN_AREA_MM.1),
                description: t.description.clone(),
                software: software(),
                datetime: tiff_datetime(),
            };
            let out = BufWriter::with_capacity(READ_CHUNK, file);
            let tiff = TiffWriter::new(out, layout).map_err(|e| write_error(&partial, &e))?;
            Some((partial, tiff))
        }
        None => None,
    };

    // A mutable borrow in a loop: `buf` is created once, and each `read` borrows it mutably
    // just long enough to fill it. `&buf[..n]` then borrows the filled part to pass it on.
    let mut buf = vec![0u8; READ_CHUNK];
    let expected = params.image_bytes();
    let mut total: u64 = 0;
    loop {
        let n = session.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > expected {
            return Err(ScanError::ScanFailed {
                frame: None,
                text: format!("the scanner sent more than the {expected} bytes it announced"),
            });
        }
        let chunk = &buf[..n];
        stats.update(chunk);
        if let Some(frame) = frame.as_mut() {
            frame.push(chunk)?;
        }
        if let Some((partial, tiff)) = writer.as_mut() {
            tiff.write(chunk).map_err(|e| write_error(partial, &e))?;
        }
    }
    if total != expected {
        return Err(ScanError::ScanFailed {
            frame: None,
            text: format!("the frame ended after {total} of {expected} bytes"),
        });
    }
    let kept = match writer {
        Some((partial, tiff)) => {
            let out = tiff.finish().map_err(|e| write_error(&partial, &e))?;
            let file = out
                .into_inner()
                .map_err(|e| write_error(&partial, e.error()))?;
            Some((partial, file))
        }
        None => None,
    };
    Ok((stats, frame, kept))
}

/// Resolution worked out from a pixel count and the length it covers in mm.
fn dpi_along(pixels: u32, mm: f64) -> f64 {
    (f64::from(pixels) / (mm / 25.4)).round()
}

fn software() -> String {
    format!("primefilm {}", env!("CARGO_PKG_VERSION"))
}

/// The TIFF `DateTime` tag: exactly `YYYY:MM:DD HH:MM:SS`.
fn tiff_datetime() -> String {
    jiff::Zoned::now().strftime("%Y:%m:%d %H:%M:%S").to_string()
}

fn write_error(partial: &PartialFile, err: &std::io::Error) -> ScanError {
    ScanError::OutputError {
        path: partial.target().display().to_string(),
        reason: err.to_string(),
    }
}

/// Add the frame number to errors raised while scanning it.
fn for_frame(err: ScanError, frame: Option<u32>) -> ScanError {
    match err {
        ScanError::Disconnected { .. } => ScanError::Disconnected {
            during: format!("scanning {}", frame_text(frame)),
        },
        ScanError::ScanFailed { text, .. } => ScanError::ScanFailed { frame, text },
        other => other,
    }
}

/// Details of a capture needed for its settings record.
struct RecordInfo<'a> {
    roll: Option<&'a str>,
    frame: Option<u32>,
    preset: &'a str,
    requested_dpi: Option<f64>,
    advance_requested: bool,
    settings: &'a ProcessSettings,
    /// Previews are 8-bit; everything else is a 16-bit master.
    preview: bool,
}

/// Write one positive (16-bit gray, or 8-bit for previews) with `description` (the settings
/// record) in its `ImageDescription` tag.
fn write_positive(
    dir: &Path,
    name: &str,
    frame: &GrayFrame,
    bits: u16,
    description: String,
) -> Result<(PartialFile, File), ScanError> {
    let (partial, file) = PartialFile::create(dir, name)?;
    let layout = TiffLayout {
        width: frame.width,
        height: frame.height,
        channels: 1,
        bits,
        dpi_x: dpi_along(frame.width, SCAN_AREA_MM.0),
        dpi_y: dpi_along(frame.height, SCAN_AREA_MM.1),
        description,
        software: software(),
        datetime: tiff_datetime(),
    };
    let out = BufWriter::with_capacity(READ_CHUNK, file);
    let mut tiff = TiffWriter::new(out, layout).map_err(|e| write_error(&partial, &e))?;
    // One row at a time, converted into a reused byte buffer.
    let mut bytes = Vec::with_capacity(frame.width as usize * 2);
    for y in 0..frame.height {
        bytes.clear();
        for &v in frame.row(y) {
            if bits == 16 {
                bytes.extend_from_slice(&v.to_ne_bytes());
            } else {
                bytes.push(to_8bit(v));
            }
        }
        tiff.write(&bytes).map_err(|e| write_error(&partial, &e))?;
    }
    let out = tiff.finish().map_err(|e| write_error(&partial, &e))?;
    let file = out
        .into_inner()
        .map_err(|e| write_error(&partial, e.error()))?;
    Ok((partial, file))
}

/// Write the processed positive `frame` with its settings record embedded, then publish it.
/// Returns the saved frame.
// Eight parameters is more than clippy's default limit; grouping them into a struct would only
// move the same list somewhere else.
#[allow(clippy::too_many_arguments)]
fn save<S: ScanSession>(
    session: &mut S,
    captured: &Captured,
    frame: &GrayFrame,
    result: &ProcessedFrame,
    dir: &Path,
    stem: &str,
    info: &RecordInfo,
    log: &Logger,
) -> Result<SavedFile, ScanError> {
    let params = captured.params;
    let (color_mode, channel_order) = if params.channels == 4 {
        ("RGBI", "R,G,B,I")
    } else {
        ("RGB", "R,G,B")
    };
    let options = session.options().unwrap_or_else(|err| {
        log.step(&format!(
            "could not read options for the settings record: {err}"
        ));
        Vec::new()
    });
    let bits = if info.preview { 8 } else { 16 };
    let record = ScanRecord {
        record_version: RECORD_VERSION,
        software: software(),
        timestamp: jiff::Zoned::now()
            .strftime("%Y-%m-%dT%H:%M:%S%:z")
            .to_string(),
        scanner: session.device().clone(),
        roll: info.roll.map(str::to_string),
        frame: info.frame,
        preset: info.preset.to_string(),
        requested_resolution_dpi: info.requested_dpi,
        resolution_dpi: dpi_along(params.pixels_per_line, SCAN_AREA_MM.0),
        color_mode,
        bits_per_sample: params.bits_per_sample,
        channel_order,
        width: params.pixels_per_line,
        height: params.lines,
        seconds: round_tenths(captured.seconds),
        advance_requested: info.advance_requested,
        options,
        output: OutputRecord {
            kind: if info.preview {
                "preview"
            } else {
                "positive-master"
            },
            channels: "Gray",
            bits_per_sample: bits,
        },
        processing: ProcessingRecord {
            film_type: info.settings.stock.film_type().id(),
            film_stock: info.settings.stock,
            profile: result.profile,
            points: result.points,
            probably_blank: result.probably_blank,
            dust: DustRecord {
                level: if result.dust.is_some() {
                    info.settings.dust
                } else {
                    DustLevel::Off
                },
                constants: result.dust,
                repaired: result.dust_repaired,
            },
            seconds: round_tenths(result.seconds),
            kept_capture: captured.kept_path.clone(),
        },
    };
    // Our record holds only strings, numbers and lists, so this cannot fail; an error here
    // would be a bug, reported as a failed write rather than a panic.
    let description = serde_json::to_string(&record).map_err(|e| ScanError::OutputError {
        path: dir.join(stem).display().to_string(),
        reason: format!("could not encode the settings record: {e}"),
    })?;
    let (partial, file) = write_positive(dir, &format!("{stem}.tif"), frame, bits, description)?;
    let image = partial.publish(file)?;

    Ok(SavedFile {
        frame: info.frame,
        image,
        width: frame.width,
        height: frame.height,
        channels: "Gray",
        bits,
        film: info.settings.stock,
        probably_blank: result.probably_blank,
        dust_repaired: result.dust_repaired,
        seconds: 0.0,
        film_moved: if info.advance_requested {
            FilmMoved::Advanced
        } else {
            FilmMoved::None
        },
    })
}

fn round_tenths(seconds: f64) -> f64 {
    (seconds * 10.0).round() / 10.0
}

fn full_settings(quality: Quality, advance_after: bool) -> ScanSettings {
    ScanSettings {
        kind: ScanKind::Full,
        resolution_dpi: quality.dpi(),
        color: BW_CAPTURE,
        advance_after,
    }
}

/// The settings for each frame of a roll: full scans, or for `roll --preview` an ordinary
/// (not preview-mode) scan at preview resolution, which can advance the film.
fn roll_settings(opts: &RunOptions) -> ScanSettings {
    if opts.roll_preview {
        ScanSettings {
            kind: ScanKind::Full,
            resolution_dpi: PREVIEW_DPI,
            color: ColorFormat::Rgb8,
            advance_after: true,
        }
    } else {
        full_settings(opts.quality, true)
    }
}

fn log_leftovers(dir: &Path, log: &Logger) {
    for path in output::leftover_partials(dir) {
        log.step(&format!(
            "leftover partial file from an earlier run: {}",
            path.display()
        ));
    }
}

/// `primefilm scan`: scan the frame in the scanner once; the film does not move (US2).
///
/// `Err` means nothing was scanned (scanner busy, not found, output folder unusable); every
/// outcome after the scanner opened is in the returned report.
pub fn scan_single<B: ScannerBackend>(
    backend: &mut B,
    roll: &str,
    frame: Option<u32>,
    opts: &RunOptions,
    log: &Logger,
    cancel: &AtomicBool,
) -> Result<RunReport, ScanError> {
    let folder = RollFolder::open(&opts.output, roll)?;
    log_leftovers(folder.dir(), log);
    // `if let … else`-style defaults: use `--frame` when given, otherwise the next free number.
    let frame = match frame {
        Some(n) => n,
        None => folder.next_frame()?,
    };
    check_frame_number(frame)?;
    let keep_dir = keep_dir(opts)?;

    let started = Instant::now();
    let mut session = backend.open(opts.device.as_deref())?;
    let mut report = RunReport::new(RunKind::Scan);
    report.roll = Some(roll.to_string());
    report.folder = Some(folder.dir().to_path_buf());
    report.quality = Some(opts.quality);
    report.film = Some(opts.process.stock);

    let result = scan_one(
        &mut session,
        &folder,
        frame,
        opts,
        keep_dir.as_deref(),
        log,
        cancel,
    );
    match result {
        Ok(saved) => report.files.push(saved),
        Err(err) => report.outcome = Outcome::Failed(for_frame(err, Some(frame))),
    }
    report.seconds = started.elapsed().as_secs_f64();
    Ok(report)
}

/// The `--keep-capture` folder, created if needed.
fn keep_dir(opts: &RunOptions) -> Result<Option<PathBuf>, ScanError> {
    match &opts.keep_capture {
        Some(dir) => {
            std::fs::create_dir_all(dir).map_err(|e| ScanError::OutputError {
                path: dir.display().to_string(),
                reason: e.to_string(),
            })?;
            Ok(Some(dir.clone()))
        }
        None => Ok(None),
    }
}

fn scan_one<S: ScanSession>(
    session: &mut S,
    folder: &RollFolder,
    frame: u32,
    opts: &RunOptions,
    keep: Option<&Path>,
    log: &Logger,
    cancel: &AtomicBool,
) -> Result<SavedFile, ScanError> {
    session.configure(&full_settings(opts.quality, false))?;
    let stem = folder.scan_stem(frame);
    eprintln!("Frame {frame:03}: scanning…");
    let captured = capture_full(session, folder, &stem, frame, keep, cancel)?;
    if captured.stats.looks_empty() {
        return Err(ScanError::NoFilm);
    }
    let (saved, result) = finish_frame(session, captured, folder, &stem, frame, opts, false, log)?;
    progress_saved(&saved, &result, "film not moved");
    Ok(saved)
}

/// Capture a full frame into memory (and, with `--keep-capture`, a raw TIFF).
fn capture_full<S: ScanSession>(
    session: &mut S,
    folder: &RollFolder,
    stem: &str,
    frame: u32,
    keep: Option<&Path>,
    cancel: &AtomicBool,
) -> Result<Captured, ScanError> {
    let keep_stem = format!("{stem}-capture");
    let keep_target = keep.map(|dir| FrameTarget {
        dir,
        stem: &keep_stem,
        description: format!(
            "Raw capture, roll {}, frame {frame:03} (development only)",
            folder.name()
        ),
    });
    capture(session, true, keep_target.as_ref(), cancel)
}

/// Process a captured full frame and save it.
// Eight parameters is more than clippy's default limit; grouping them into a struct would only
// move the same list somewhere else.
#[allow(clippy::too_many_arguments)]
fn finish_frame<S: ScanSession>(
    session: &mut S,
    mut captured: Captured,
    folder: &RollFolder,
    stem: &str,
    frame: u32,
    opts: &RunOptions,
    advance_requested: bool,
    log: &Logger,
) -> Result<(SavedFile, ProcessedFrame), ScanError> {
    let started = Instant::now();
    // The raw capture for `--keep-capture` is published first: it is useful even if
    // processing fails.
    if let Some((partial, file)) = captured.kept.take() {
        let path = partial.publish(file)?;
        log.step(&format!("kept raw capture {}", path.display()));
        // Remember where it went for the settings record.
        captured.kept_path = Some(path);
    }
    let mut gray = captured.gray.take().ok_or_else(|| ScanError::ScanFailed {
        frame: Some(frame),
        text: "no image was captured".to_string(),
    })?;
    let result = process::process(&mut gray, &opts.process)?;
    let (preset, requested_dpi) = if opts.roll_preview {
        ("preview", PREVIEW_DPI)
    } else {
        (opts.quality.name(), opts.quality.dpi())
    };
    let info = RecordInfo {
        roll: Some(folder.name()),
        frame: Some(frame),
        preset,
        requested_dpi: Some(requested_dpi),
        advance_requested,
        settings: &opts.process,
        preview: opts.roll_preview,
    };
    let mut saved = save(
        session,
        &captured,
        &gray,
        &result,
        folder.dir(),
        stem,
        &info,
        log,
    )?;
    saved.seconds = round_tenths(captured.seconds + started.elapsed().as_secs_f64());
    Ok((saved, result))
}

/// `primefilm roll`: scan, advance, repeat until the end of the film (US1).
///
/// Stops when a frame shows an empty gate or repeats the previous frame (the PrimeFilm XA
/// stays on its last frame instead of reporting the end, research D4), after `limit` frames,
/// after Ctrl-C (once the frame in progress is saved, research D6), or at the first error,
/// without retrying (FR-017). Frames saved before the stop are always kept.
///
/// With `opts.roll_preview`, every frame is scanned at preview resolution and saved as an 8-bit
/// preview in `<output>/previews/<roll>/`, apart from archival scans (002 FR-012).
pub fn scan_roll<B: ScannerBackend>(
    backend: &mut B,
    roll: &str,
    start_frame: Option<u32>,
    limit: Option<u32>,
    opts: &RunOptions,
    log: &Logger,
    cancel: &AtomicBool,
) -> Result<RunReport, ScanError> {
    let root = if opts.roll_preview {
        opts.output.join("previews")
    } else {
        opts.output.clone()
    };
    let folder = RollFolder::open(&root, roll)?;
    log_leftovers(folder.dir(), log);
    let first = match start_frame {
        Some(n) => n,
        None => folder.next_frame()?,
    };
    check_frame_number(first)?;
    let keep_dir = keep_dir(opts)?;

    let started = Instant::now();
    let mut session = backend.open(opts.device.as_deref())?;
    let mut report = RunReport::new(RunKind::Roll);
    report.roll = Some(roll.to_string());
    report.folder = Some(folder.dir().to_path_buf());
    // A roll preview has no quality preset.
    report.quality = (!opts.roll_preview).then_some(opts.quality);
    report.film = Some(opts.process.stock);
    report.next_frame = Some(first);
    let quality = if opts.roll_preview {
        format!("preview, {PREVIEW_DPI} dpi")
    } else {
        format!("preset {}, {} dpi", opts.quality.name(), opts.quality.dpi())
    };
    eprintln!(
        "Roll {roll} → {} ({quality}, {}, dust {})",
        folder.dir().display(),
        opts.process.stock.name(),
        opts.process.dust.name(),
    );

    let run = RollRun {
        folder: &folder,
        opts,
        keep: keep_dir.as_deref(),
        log,
        cancel,
    };
    report.outcome = roll_loop(&mut session, &run, first, limit, &mut report);
    report.seconds = started.elapsed().as_secs_f64();
    Ok(report)
}

/// The fixed parts of a roll run.
struct RollRun<'a> {
    folder: &'a RollFolder,
    opts: &'a RunOptions,
    keep: Option<&'a Path>,
    log: &'a Logger,
    cancel: &'a AtomicBool,
}

fn roll_loop<S: ScanSession>(
    session: &mut S,
    run: &RollRun,
    first: u32,
    limit: Option<u32>,
    report: &mut RunReport,
) -> Outcome {
    let (folder, opts) = (run.folder, run.opts);
    if let Err(err) = session.configure(&roll_settings(opts)) {
        return Outcome::Failed(err);
    }
    let mut previous: Option<Fingerprint> = None;
    let mut frame = first;
    loop {
        if run.cancel.load(Ordering::SeqCst) {
            return Outcome::Failed(ScanError::Interrupted);
        }
        if let Err(err) = check_frame_number(frame) {
            return Outcome::Failed(err);
        }
        let stem = folder.scan_stem(frame);
        eprintln!("Frame {frame:03}: scanning…");
        let captured = match capture_full(session, folder, &stem, frame, run.keep, run.cancel) {
            Ok(c) => c,
            Err(err) => return Outcome::Failed(for_frame(err, Some(frame))),
        };

        let fingerprint = captured.stats.fingerprint();
        let empty = captured.stats.looks_empty();
        // `Option::is_some_and` asks "is there a previous fingerprint, and does it match?"
        let repeated = previous
            .as_ref()
            .is_some_and(|p| p.same_frame(&fingerprint));
        if empty || repeated {
            // The capture is dropped here, with any raw partial file.
            if report.files.is_empty() && empty {
                return Outcome::Failed(ScanError::NoFilm);
            }
            if let Some(last) = report.files.last_mut() {
                // The film could not move past the last saved frame.
                last.film_moved = FilmMoved::None;
            }
            let why = if repeated {
                "same as the previous frame"
            } else {
                "no film in the gate"
            };
            eprintln!("Frame {frame:03}: {why}; end of film.");
            return Outcome::EndOfFilm(end_message(report));
        }

        let result = finish_frame(session, captured, folder, &stem, frame, opts, true, run.log);
        match result {
            Ok((saved, processed)) => {
                progress_saved(&saved, &processed, "advancing film");
                run.log
                    .step(&format!("film advanced after frame {frame:03}"));
                report.files.push(saved);
            }
            Err(err) => return Outcome::Failed(for_frame(err, Some(frame))),
        }
        previous = Some(fingerprint);
        frame += 1;
        report.next_frame = Some(frame);

        if limit.is_some_and(|n| report.files.len() as u32 >= n) {
            return Outcome::EndOfFilm(format!(
                "Scanned the {} frames asked for.",
                report.files.len()
            ));
        }
    }
}

fn end_message(report: &RunReport) -> String {
    match report.files.last().and_then(|f| f.frame) {
        Some(n) => format!("Reached the end of the film after frame {n:03}."),
        None => "Reached the end of the film.".to_string(),
    }
}

fn check_frame_number(frame: u32) -> Result<(), ScanError> {
    if (1..=MAX_FRAME).contains(&frame) {
        Ok(())
    } else {
        Err(ScanError::OutputError {
            path: format!("frame {frame}"),
            reason: format!("frame numbers go from 1 to {MAX_FRAME}"),
        })
    }
}

/// `primefilm preview`: a quick, non-archival positive of the frame in the scanner (US3,
/// feature 003 US6).
pub fn preview<B: ScannerBackend>(
    backend: &mut B,
    opts: &RunOptions,
    log: &Logger,
    cancel: &AtomicBool,
) -> Result<RunReport, ScanError> {
    let (dir, stem) = output::preview_stem(&opts.output)?;
    log_leftovers(&dir, log);
    let started = Instant::now();
    let mut session = backend.open(opts.device.as_deref())?;
    let mut report = RunReport::new(RunKind::Preview);
    report.folder = Some(dir.clone());
    report.film = Some(opts.process.stock);

    let result = preview_one(&mut session, &dir, &stem, opts, log, cancel);
    match result {
        Ok(saved) => report.files.push(saved),
        Err(err) => report.outcome = Outcome::Failed(for_frame(err, None)),
    }
    report.seconds = started.elapsed().as_secs_f64();
    Ok(report)
}

fn preview_one<S: ScanSession>(
    session: &mut S,
    dir: &Path,
    stem: &str,
    opts: &RunOptions,
    log: &Logger,
    cancel: &AtomicBool,
) -> Result<SavedFile, ScanError> {
    session.configure(&ScanSettings {
        kind: ScanKind::Preview,
        resolution_dpi: PREVIEW_DPI,
        color: ColorFormat::Rgb8,
        advance_after: false,
    })?;
    eprintln!("Preview: scanning…");
    let mut captured = capture(session, true, None, cancel)?;
    if captured.stats.looks_empty() {
        return Err(ScanError::NoFilm);
    }
    let started = Instant::now();
    // Previews are not archival: no dust removal, to keep them quick (research P11).
    let settings = ProcessSettings {
        dust: DustLevel::Off,
        ..opts.process
    };
    let mut gray = captured.gray.take().ok_or_else(|| ScanError::ScanFailed {
        frame: None,
        text: "no image was captured".to_string(),
    })?;
    let result = process::process(&mut gray, &settings)?;
    let info = RecordInfo {
        roll: None,
        frame: None,
        preset: "preview",
        requested_dpi: None,
        advance_requested: false,
        settings: &settings,
        preview: true,
    };
    let mut saved = save(session, &captured, &gray, &result, dir, stem, &info, log)?;
    saved.seconds = round_tenths(captured.seconds + started.elapsed().as_secs_f64());
    progress_saved(&saved, &result, "film not moved");
    Ok(saved)
}

/// `primefilm advance`: move the film one frame without saving anything (US5, research D5).
///
/// Two tiny throwaway scans: the first fingerprints the frame in the gate and asks the scanner
/// to advance; the second fingerprints the frame now in the gate. If they match, the film was
/// already on its last frame and did not move.
pub fn advance<B: ScannerBackend>(
    backend: &mut B,
    device: Option<&str>,
    log: &Logger,
    cancel: &AtomicBool,
) -> Result<RunReport, ScanError> {
    let started = Instant::now();
    let mut session = backend.open(device)?;
    let mut report = RunReport::new(RunKind::Advance);
    report.outcome = match advance_once(&mut session, log, cancel) {
        Ok(outcome) => outcome,
        Err(err) => Outcome::Failed(for_frame(err, None)),
    };
    report.seconds = started.elapsed().as_secs_f64();
    Ok(report)
}

fn advance_once<S: ScanSession>(
    session: &mut S,
    log: &Logger,
    cancel: &AtomicBool,
) -> Result<Outcome, ScanError> {
    let throwaway = |advance_after| ScanSettings {
        kind: ScanKind::Throwaway,
        resolution_dpi: MIN_DPI,
        color: ColorFormat::Rgb8,
        advance_after,
    };
    session.configure(&throwaway(true))?;
    eprintln!("Advancing the film…");
    let before = capture(session, false, None, cancel)?;
    if before.stats.looks_empty() {
        return Err(ScanError::NoFilm);
    }
    session.configure(&throwaway(false))?;
    let after = capture(session, false, None, cancel)?;
    if before
        .stats
        .fingerprint()
        .same_frame(&after.stats.fingerprint())
    {
        return Ok(Outcome::EndOfFilm(
            "Already at the last frame; the film did not move.".to_string(),
        ));
    }
    log.step("film advanced one frame");
    Ok(Outcome::Completed(Some(
        "Film advanced one frame.".to_string(),
    )))
}

/// The progress line for a saved frame (stderr), e.g. `Frame 002: probably blank; saved
/// r-002.tif (5173 × 3443, gray 16-bit) in 93 s; advancing film`.
fn progress_saved(saved: &SavedFile, result: &ProcessedFrame, film: &str) {
    let name = saved
        .image
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let label = match saved.frame {
        Some(n) => format!("Frame {n:03}"),
        None => "Preview".to_string(),
    };
    let mut notes = String::new();
    if result.probably_blank {
        notes.push_str("probably blank; ");
    }
    if result.dust_repaired > 0 {
        notes.push_str(&format!("{} dust specks removed; ", result.dust_repaired));
    }
    eprintln!(
        "{label}: {notes}saved {name} ({} × {}, gray {}-bit) in {:.0} s; {film}",
        saved.width, saved.height, saved.bits, saved.seconds
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::fake::{FakeBackend, FakeSession};
    use crate::cli::Scenario;
    use crate::preset::Preset;
    use std::fs;
    use std::sync::atomic::AtomicU32;

    fn temp_dir(label: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "primefilm-scan-test-{}-{label}-{n}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn opts(output: &Path) -> RunOptions {
        RunOptions {
            output: output.to_path_buf(),
            output_given: true,
            device: None,
            quality: Quality::Preset(Preset::Web),
            quality_given: false,
            process: ProcessSettings::from_profile(FilmStock::Hp5),
            process_flags: Vec::new(),
            keep_capture: None,
            roll_preview: false,
        }
    }

    fn session(scenario: Scenario, frames: u32) -> FakeSession {
        FakeBackend::new(scenario, frames).open(None).unwrap()
    }

    fn quiet() -> Logger {
        Logger::new(0)
    }

    fn visible_files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// A roll preview uses ordinary scans at preview resolution, because the scanner's preview
    /// mode never moves the film.
    #[test]
    fn roll_preview_uses_ordinary_low_resolution_scans_that_advance() {
        let mut o = opts(Path::new("/x"));
        assert_eq!(roll_settings(&o), full_settings(o.quality, true));
        o.roll_preview = true;
        assert_eq!(
            roll_settings(&o),
            ScanSettings {
                kind: ScanKind::Full,
                resolution_dpi: PREVIEW_DPI,
                color: ColorFormat::Rgb8,
                advance_after: true,
            }
        );
    }

    #[test]
    fn single_scan_does_not_move_film() {
        let root = temp_dir("single");
        let folder = RollFolder::open(&root, "t").unwrap();
        let mut s = session(Scenario::Ready, 6);
        let cancel = AtomicBool::new(false);
        let o = opts(&root);
        let saved = scan_one(&mut s, &folder, 1, &o, None, &quiet(), &cancel).unwrap();
        assert_eq!(s.position(), 1);
        assert_eq!(saved.film_moved, FilmMoved::None);
        assert_eq!(saved.channels, "Gray");
        assert_eq!(saved.bits, 16);
        assert_eq!(saved.film, FilmStock::Hp5);
        // The positive only: no raw capture, no sidecar, no share copy.
        assert_eq!(visible_files(folder.dir()), vec!["t-001.tif"]);
    }

    /// The film order is checked on the captured gray frames: the simulated scanner marks
    /// each frame's first pixel with its number (counted down from full scale).
    #[test]
    fn captures_follow_the_film_order() {
        let mut s = session(Scenario::Ready, 3);
        let cancel = AtomicBool::new(false);
        s.configure(&full_settings(Quality::Preset(Preset::Web), true))
            .unwrap();
        let numbers: Vec<u16> = (0..4)
            .map(|_| {
                let captured = capture(&mut s, true, None, &cancel).unwrap();
                u16::MAX - captured.gray.unwrap().pixels[0]
            })
            .collect();
        // The film stays on its last frame.
        assert_eq!(numbers, vec![1, 2, 3, 3]);
    }

    #[test]
    fn failure_after_processing_stops_the_roll_and_keeps_earlier_frames() {
        let root = temp_dir("procfail");
        let dir = root.join("p");
        // A folder where frame 3's positive would be written makes saving it fail after the
        // frame was captured and processed.
        fs::create_dir_all(dir.join(".p-003.tif.partial")).unwrap();
        let mut backend = FakeBackend::new(Scenario::Ready, 6);
        let cancel = AtomicBool::new(false);
        let report = scan_roll(
            &mut backend,
            "p",
            None,
            None,
            &opts(&root),
            &quiet(),
            &cancel,
        )
        .unwrap();
        match &report.outcome {
            Outcome::Failed(err) => {
                assert_eq!(err.outcome(), "output-error");
                assert!(err.to_string().contains("p-003.tif"), "{err}");
            }
            other => panic!("expected a failure, got {other:?}"),
        }
        assert_eq!(report.files.len(), 2);
        assert_eq!(report.next_frame, Some(3));
        let mut names = visible_files(&dir);
        names.retain(|n| !n.starts_with('.'));
        assert_eq!(names, vec!["p-001.tif", "p-002.tif"]);
    }

    #[test]
    fn single_scan_failures_leave_no_files() {
        let cancel = AtomicBool::new(false);
        for (scenario, expected) in [
            (Scenario::Jam, "scan-failed"),
            (Scenario::Disconnect, "disconnected"),
        ] {
            let root = temp_dir("fail");
            let folder = RollFolder::open(&root, "t").unwrap();
            let mut s = session(scenario, 6);
            s.set_position(3);
            let err =
                scan_one(&mut s, &folder, 3, &opts(&root), None, &quiet(), &cancel).unwrap_err();
            assert_eq!(for_frame(err, Some(3)).outcome(), expected);
            assert!(visible_files(folder.dir()).is_empty(), "{scenario:?}");
        }
    }

    #[test]
    fn single_scan_of_empty_gate_is_no_film() {
        let root = temp_dir("nofilm");
        let folder = RollFolder::open(&root, "t").unwrap();
        let mut s = session(Scenario::NoFilm, 6);
        let cancel = AtomicBool::new(false);
        let err = scan_one(&mut s, &folder, 1, &opts(&root), None, &quiet(), &cancel).unwrap_err();
        assert_eq!(err, ScanError::NoFilm);
        assert!(visible_files(folder.dir()).is_empty());
    }

    #[test]
    fn roll_advances_once_per_saved_frame() {
        let root = temp_dir("roll");
        let mut backend = FakeBackend::new(Scenario::Ready, 4);
        let cancel = AtomicBool::new(false);
        let report = scan_roll(
            &mut backend,
            "r",
            None,
            None,
            &opts(&root),
            &quiet(),
            &cancel,
        )
        .unwrap();
        assert_eq!(
            report.outcome,
            Outcome::EndOfFilm("Reached the end of the film after frame 004.".into())
        );
        let frames: Vec<u32> = report.files.iter().filter_map(|f| f.frame).collect();
        assert_eq!(frames, vec![1, 2, 3, 4]);
        assert_eq!(report.files[2].film_moved, FilmMoved::Advanced);
        assert_eq!(report.files[3].film_moved, FilmMoved::None);
        assert_eq!(report.next_frame, Some(5));
    }

    #[test]
    fn roll_never_replaces_existing_frames() {
        let root = temp_dir("existing");
        let dir = root.join("r");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("r-001.tif"), "original").unwrap();
        let mut backend = FakeBackend::new(Scenario::Ready, 2);
        let cancel = AtomicBool::new(false);
        let report = scan_roll(
            &mut backend,
            "r",
            Some(1),
            None,
            &opts(&root),
            &quiet(),
            &cancel,
        )
        .unwrap();
        assert!(report.files[0].image.ends_with("r-001-2.tif"));
        assert_eq!(
            fs::read_to_string(dir.join("r-001.tif")).unwrap(),
            "original"
        );
    }

    #[test]
    fn roll_stops_after_interrupted_frame_is_saved() {
        let root = temp_dir("interrupt");
        let mut backend = FakeBackend::new(Scenario::Interrupt, 6);
        let cancel = AtomicBool::new(false);
        let report = scan_roll(
            &mut backend,
            "r",
            None,
            None,
            &opts(&root),
            &quiet(),
            &cancel,
        )
        .unwrap();
        assert_eq!(report.outcome, Outcome::Failed(ScanError::Interrupted));
        assert_eq!(report.files.len(), 2);
        assert_eq!(report.next_frame, Some(3));
    }

    #[test]
    fn advance_moves_one_frame_and_detects_the_end() {
        let mut s = session(Scenario::Ready, 3);
        let cancel = AtomicBool::new(false);
        let outcome = advance_once(&mut s, &quiet(), &cancel).unwrap();
        assert_eq!(
            outcome,
            Outcome::Completed(Some("Film advanced one frame.".into()))
        );
        assert_eq!(s.position(), 2);
        s.set_position(3);
        let outcome = advance_once(&mut s, &quiet(), &cancel).unwrap();
        assert!(matches!(outcome, Outcome::EndOfFilm(_)));
        assert_eq!(s.position(), 3);
    }

    #[test]
    fn advance_with_no_film() {
        let mut s = session(Scenario::NoFilm, 3);
        let cancel = AtomicBool::new(false);
        assert_eq!(
            advance_once(&mut s, &quiet(), &cancel).unwrap_err(),
            ScanError::NoFilm
        );
    }
}
