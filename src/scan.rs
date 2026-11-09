//! Single-frame, roll, preview and advance sequencing over a scan session (research D1).
//!
//! Everything here is written against the [`ScannerBackend`] and [`ScanSession`] traits, so the
//! same code drives the real scanner, SANE's test backend and the simulated film strip.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use serde::Serialize;

use crate::backend::{
    ColorFormat, DeviceInfo, FrameParams, ScanKind, ScanSession, ScanSettings, ScannerBackend,
};
use crate::capability::Capability;
use crate::error::{ScanError, frame_text};
use crate::frame_stats::{Fingerprint, FrameStats};
use crate::output::{self, MAX_FRAME, PartialFile, RollFolder};
use crate::preset::{MIN_DPI, Quality, SCAN_AREA_MM};
use crate::report::Logger;
use crate::tiff::{TiffLayout, TiffWriter};

/// Bytes read from the scanner at a time. One buffer of this size is reused for every chunk
/// of every frame, so memory use does not grow with the frame size.
const READ_CHUNK: usize = 1 << 20;

/// Version of the settings record (`.json` sidecar) format.
const RECORD_VERSION: u32 = 1;

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

/// One image saved by a run.
#[derive(Debug, Clone, Serialize)]
pub struct SavedFile {
    /// Absent for previews.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame: Option<u32>,
    pub image: PathBuf,
    pub settings: PathBuf,
    pub width: u32,
    pub height: u32,
    /// "RGB" or "RGBI".
    pub channels: &'static str,
    pub bits: u16,
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
            files: Vec::new(),
            outcome: Outcome::Completed(None),
            next_frame: None,
            seconds: 0.0,
        }
    }
}

/// The settings record saved next to every image as `<name>.json` (research D10).
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
    /// `RGBI` or `RGB`.
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
}

/// A frame captured and written to a `.partial` file, not yet published.
struct Captured {
    params: FrameParams,
    stats: FrameStats,
    seconds: f64,
    image: Option<(PartialFile, File)>,
}

/// What to name and how to tag the image of one capture.
struct FrameTarget<'a> {
    dir: &'a Path,
    stem: &'a str,
    description: String,
}

/// Capture one frame: start it, stream every byte into the TIFF writer (if `target` is
/// given) and the statistics, then end the frame. On any error the `.partial` file is
/// dropped, which deletes it, so no incomplete image is ever left looking complete.
fn capture<S: ScanSession>(
    session: &mut S,
    target: Option<&FrameTarget>,
    cancel: &AtomicBool,
) -> Result<Captured, ScanError> {
    let started = Instant::now();
    let params = session.start_frame(cancel)?;
    let result = stream(session, &params, target);
    // `end_frame` runs whether streaming worked or not.
    session.end_frame();
    let (stats, image) = result?;
    Ok(Captured {
        params,
        stats,
        seconds: started.elapsed().as_secs_f64(),
        image,
    })
}

/// The part of [`capture`] between starting and ending the frame.
fn stream<S: ScanSession>(
    session: &mut S,
    params: &FrameParams,
    target: Option<&FrameTarget>,
) -> Result<(FrameStats, Option<(PartialFile, File)>), ScanError> {
    let mut stats = params.new_stats();
    let mut writer = match target {
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
                datetime: jiff::Zoned::now().strftime("%Y:%m:%d %H:%M:%S").to_string(),
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

    let image = match writer {
        Some((partial, tiff)) => {
            let out = tiff.finish().map_err(|e| write_error(&partial, &e))?;
            let file = out
                .into_inner()
                .map_err(|e| write_error(&partial, e.error()))?;
            Some((partial, file))
        }
        None => None,
    };
    Ok((stats, image))
}

/// Resolution worked out from a pixel count and the length it covers in mm.
fn dpi_along(pixels: u32, mm: f64) -> f64 {
    (f64::from(pixels) / (mm / 25.4)).round()
}

fn software() -> String {
    format!("primefilm {}", env!("CARGO_PKG_VERSION"))
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
}

/// Write the settings record and publish it, then the image. Returns the saved file.
fn save<S: ScanSession>(
    session: &mut S,
    captured: Captured,
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
    };

    let (sidecar, mut file) = PartialFile::create(dir, &format!("{stem}.json"))?;
    serde_json::to_writer_pretty(&mut file, &record)
        .map_err(std::io::Error::from)
        .and_then(|()| file.write_all(b"\n"))
        .map_err(|e| write_error(&sidecar, &e))?;
    let image = captured.image.ok_or_else(|| ScanError::OutputError {
        path: dir.join(format!("{stem}.tif")).display().to_string(),
        reason: "no image was written".to_string(),
    })?;
    let (image_path, settings_path) = output::publish_scan((sidecar, file), image)?;

    Ok(SavedFile {
        frame: info.frame,
        image: image_path,
        settings: settings_path,
        width: params.pixels_per_line,
        height: params.lines,
        channels: color_mode,
        bits: params.bits_per_sample,
        seconds: round_tenths(captured.seconds),
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
        color: ColorFormat::Rgbi16,
        advance_after,
    }
}

fn description(roll: &str, frame: u32, quality: Quality) -> String {
    format!(
        "Roll {roll}, frame {frame:03}, preset {}, {} dpi",
        quality.name(),
        quality.dpi()
    )
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

    let started = Instant::now();
    let mut session = backend.open(opts.device.as_deref())?;
    let mut report = RunReport::new(RunKind::Scan);
    report.roll = Some(roll.to_string());
    report.folder = Some(folder.dir().to_path_buf());
    report.quality = Some(opts.quality);

    let result = scan_one(&mut session, &folder, frame, opts.quality, log, cancel);
    match result {
        Ok(saved) => report.files.push(saved),
        Err(err) => report.outcome = Outcome::Failed(for_frame(err, Some(frame))),
    }
    report.seconds = started.elapsed().as_secs_f64();
    Ok(report)
}

fn scan_one<S: ScanSession>(
    session: &mut S,
    folder: &RollFolder,
    frame: u32,
    quality: Quality,
    log: &Logger,
    cancel: &AtomicBool,
) -> Result<SavedFile, ScanError> {
    session.configure(&full_settings(quality, false))?;
    let stem = folder.scan_stem(frame);
    let target = FrameTarget {
        dir: folder.dir(),
        stem: &stem,
        description: description(folder.name(), frame, quality),
    };
    eprintln!("Frame {frame:03}: scanning…");
    let captured = capture(session, Some(&target), cancel)?;
    if captured.stats.looks_empty() {
        return Err(ScanError::NoFilm);
    }
    let info = RecordInfo {
        roll: Some(folder.name()),
        frame: Some(frame),
        preset: quality.name(),
        requested_dpi: Some(quality.dpi()),
        advance_requested: false,
    };
    let saved = save(session, captured, folder.dir(), &stem, &info, log)?;
    progress_saved(&saved, "film not moved");
    Ok(saved)
}

/// `primefilm roll`: scan, advance, repeat until the end of the film (US1).
///
/// Stops when a frame shows an empty gate or repeats the previous frame (the PrimeFilm XA
/// stays on its last frame instead of reporting the end, research D4), after `limit` frames,
/// after Ctrl-C (once the frame in progress is saved, research D6), or at the first error,
/// without retrying (FR-017). Frames saved before the stop are always kept.
pub fn scan_roll<B: ScannerBackend>(
    backend: &mut B,
    roll: &str,
    start_frame: Option<u32>,
    limit: Option<u32>,
    opts: &RunOptions,
    log: &Logger,
    cancel: &AtomicBool,
) -> Result<RunReport, ScanError> {
    let folder = RollFolder::open(&opts.output, roll)?;
    log_leftovers(folder.dir(), log);
    let first = match start_frame {
        Some(n) => n,
        None => folder.next_frame()?,
    };
    check_frame_number(first)?;

    let started = Instant::now();
    let mut session = backend.open(opts.device.as_deref())?;
    let mut report = RunReport::new(RunKind::Roll);
    report.roll = Some(roll.to_string());
    report.folder = Some(folder.dir().to_path_buf());
    report.quality = Some(opts.quality);
    report.next_frame = Some(first);
    eprintln!(
        "Roll {roll} → {} (preset {}, {} dpi)",
        folder.dir().display(),
        opts.quality.name(),
        opts.quality.dpi()
    );

    report.outcome = roll_loop(
        &mut session,
        &folder,
        first,
        limit,
        opts.quality,
        log,
        cancel,
        &mut report,
    );
    report.seconds = started.elapsed().as_secs_f64();
    Ok(report)
}

// Eight parameters is more than clippy's default limit; grouping them into a struct would
// only move the same list somewhere else.
#[allow(clippy::too_many_arguments)]
fn roll_loop<S: ScanSession>(
    session: &mut S,
    folder: &RollFolder,
    first: u32,
    limit: Option<u32>,
    quality: Quality,
    log: &Logger,
    cancel: &AtomicBool,
    report: &mut RunReport,
) -> Outcome {
    if let Err(err) = session.configure(&full_settings(quality, true)) {
        return Outcome::Failed(err);
    }
    let mut previous: Option<Fingerprint> = None;
    let mut frame = first;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Outcome::Failed(ScanError::Interrupted);
        }
        if let Err(err) = check_frame_number(frame) {
            return Outcome::Failed(err);
        }
        let stem = folder.scan_stem(frame);
        let target = FrameTarget {
            dir: folder.dir(),
            stem: &stem,
            description: description(folder.name(), frame, quality),
        };
        eprintln!("Frame {frame:03}: scanning…");
        let captured = match capture(session, Some(&target), cancel) {
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
            // The capture is dropped here, deleting its partial file.
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

        let info = RecordInfo {
            roll: Some(folder.name()),
            frame: Some(frame),
            preset: quality.name(),
            requested_dpi: Some(quality.dpi()),
            advance_requested: true,
        };
        match save(session, captured, folder.dir(), &stem, &info, log) {
            Ok(saved) => {
                progress_saved(&saved, "advancing film");
                log.step(&format!("film advanced after frame {frame:03}"));
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

/// `primefilm preview`: a quick, non-archival look at the frame in the scanner (US3).
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

    let result = preview_one(&mut session, &dir, &stem, log, cancel);
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
    log: &Logger,
    cancel: &AtomicBool,
) -> Result<SavedFile, ScanError> {
    session.configure(&ScanSettings {
        kind: ScanKind::Preview,
        resolution_dpi: 300.0,
        color: ColorFormat::Rgb8,
        advance_after: false,
    })?;
    let target = FrameTarget {
        dir,
        stem,
        description: "Preview (not an archival scan)".to_string(),
    };
    eprintln!("Preview: scanning…");
    let captured = capture(session, Some(&target), cancel)?;
    if captured.stats.looks_empty() {
        return Err(ScanError::NoFilm);
    }
    let info = RecordInfo {
        roll: None,
        frame: None,
        preset: "preview",
        requested_dpi: None,
        advance_requested: false,
    };
    let saved = save(session, captured, dir, stem, &info, log)?;
    progress_saved(&saved, "film not moved");
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
    let before = capture(session, None, cancel)?;
    if before.stats.looks_empty() {
        return Err(ScanError::NoFilm);
    }
    session.configure(&throwaway(false))?;
    let after = capture(session, None, cancel)?;
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

/// The progress line for a saved frame (stderr).
fn progress_saved(saved: &SavedFile, film: &str) {
    let name = saved
        .image
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let label = match saved.frame {
        Some(n) => format!("Frame {n:03}"),
        None => "Preview".to_string(),
    };
    eprintln!(
        "{label}: saved {name} ({} × {}, {} {}-bit) in {:.0} s; {film}",
        saved.width, saved.height, saved.channels, saved.bits, saved.seconds
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

    #[test]
    fn single_scan_does_not_move_film() {
        let root = temp_dir("single");
        let folder = RollFolder::open(&root, "t").unwrap();
        let mut s = session(Scenario::Ready, 6);
        let cancel = AtomicBool::new(false);
        let quality = Quality::Preset(Preset::Web);
        let saved = scan_one(&mut s, &folder, 1, quality, &quiet(), &cancel).unwrap();
        assert_eq!(s.position(), 1);
        assert_eq!(saved.film_moved, FilmMoved::None);
        assert_eq!(saved.channels, "RGBI");
        assert_eq!(visible_files(folder.dir()), vec!["t-001.json", "t-001.tif"]);
    }

    #[test]
    fn single_scan_failures_leave_no_files() {
        let cancel = AtomicBool::new(false);
        let quality = Quality::Preset(Preset::Web);
        for (scenario, expected) in [
            (Scenario::Jam, "scan-failed"),
            (Scenario::Disconnect, "disconnected"),
        ] {
            let root = temp_dir("fail");
            let folder = RollFolder::open(&root, "t").unwrap();
            let mut s = session(scenario, 6);
            s.set_position(3);
            let err = scan_one(&mut s, &folder, 3, quality, &quiet(), &cancel).unwrap_err();
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
        let err = scan_one(
            &mut s,
            &folder,
            1,
            Quality::Preset(Preset::Web),
            &quiet(),
            &cancel,
        )
        .unwrap_err();
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
