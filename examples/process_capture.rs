//! Replay a raw capture saved with the hidden `--keep-capture DIR` flag through the B&W
//! processing pipeline, with any settings, without rescanning the film (research P12).
//!
//! ```text
//! cargo run --example process_capture -- CAPTURE.tif --film hp5 [--dust normal]
//!     [--brightness N] [--contrast N] [--black D --white D] --out DIR
//! ```
//!
//! Writes `<capture name>-<settings>.tif` (16-bit gray) and `.jpg` into DIR and prints the
//! points, the blank flag, the number of specks repaired and the time taken.
//!
//! An example is a small program that uses the library like any other crate would; it is
//! built with `cargo build --examples` and run with `cargo run --example NAME`. This one may
//! use the `tiff` decoder from `[dev-dependencies]`, since examples are not part of the tool.

use std::fs::File;
use std::io::{BufWriter, Cursor};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use primefilm::backend::FrameParams;
use primefilm::film::{DustLevel, FilmStock, parse_film};
use primefilm::gray::GrayFrame;
use primefilm::jpeg::write_share_copy;
use primefilm::preset::SCAN_AREA_MM;
use primefilm::process::{self, ProcessSettings};
use primefilm::tiff::{TiffLayout, TiffWriter};
use tiff::decoder::{Decoder, DecodingResult};

/// Command-line arguments, parsed by hand: the example is small enough not to need clap.
struct Args {
    capture: PathBuf,
    out: PathBuf,
    settings: ProcessSettings,
}

fn parse_args() -> Result<Args, String> {
    let mut words = std::env::args().skip(1);
    let mut capture = None;
    let mut out = None;
    let mut stock = FilmStock::DEFAULT;
    let mut dust = None;
    let mut brightness = None;
    let mut contrast = None;
    let (mut black, mut white) = (None, None);
    while let Some(word) = words.next() {
        // Every option takes one value.
        let mut value = || words.next().ok_or(format!("{word} needs a value"));
        match word.as_str() {
            "--film" => stock = parse_film(&value()?)?,
            "--dust" => {
                let text = value()?;
                dust = Some(match text.as_str() {
                    "off" => DustLevel::Off,
                    "gentle" => DustLevel::Gentle,
                    "normal" => DustLevel::Normal,
                    "strong" => DustLevel::Strong,
                    _ => return Err(format!("unknown dust level {text}")),
                });
            }
            "--brightness" => brightness = Some(number(&value()?)? as i32),
            "--contrast" => contrast = Some(number(&value()?)? as i32),
            "--black" => black = Some(number(&value()?)?),
            "--white" => white = Some(number(&value()?)?),
            "--out" => out = Some(PathBuf::from(value()?)),
            _ if capture.is_none() && !word.starts_with("--") => {
                capture = Some(PathBuf::from(word))
            }
            _ => return Err(format!("unexpected argument {word}")),
        }
    }
    let defaults = ProcessSettings::from_profile(stock);
    Ok(Args {
        capture: capture.ok_or("give the capture TIFF to process")?,
        out: out.ok_or("give --out DIR")?,
        settings: ProcessSettings {
            brightness: brightness.unwrap_or(defaults.brightness),
            contrast: contrast.unwrap_or(defaults.contrast),
            points: black.zip(white),
            dust: dust.unwrap_or(defaults.dust),
            ..defaults
        },
    })
}

fn number(text: &str) -> Result<f64, String> {
    text.parse().map_err(|_| format!("{text} is not a number"))
}

/// Decode a kept RGB(I) capture into a gray frame, exactly as a live scan would build it.
fn load(path: &Path) -> Result<GrayFrame, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut decoder = Decoder::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let (width, height) = decoder.dimensions().map_err(|e| e.to_string())?;
    let samples = match decoder.read_image().map_err(|e| e.to_string())? {
        DecodingResult::U16(samples) => samples,
        _ => return Err("expected a 16-bit capture".to_string()),
    };
    // The decoder may or may not return the infrared sample; count what it gave.
    let channels = samples.len() / (width as usize * height as usize);
    let bytes_per_line = width as usize * channels * 2;
    let params = FrameParams::from_sane(bytes_per_line as i32, width as i32, height as i32, 16)
        .map_err(|e| e.to_string())?;
    let dpi = (f64::from(width) / (SCAN_AREA_MM.0 / 25.4)).round();
    let mut frame = GrayFrame::new(&params, dpi).map_err(|e| e.to_string())?;
    let raw: Vec<u8> = samples.iter().flat_map(|s| s.to_ne_bytes()).collect();
    frame.push(&raw).map_err(|e| e.to_string())?;
    Ok(frame)
}

fn save(frame: &GrayFrame, dir: &Path, stem: &str) -> Result<(), String> {
    let path = dir.join(format!("{stem}.tif"));
    let file = File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let layout = TiffLayout {
        width: frame.width,
        height: frame.height,
        channels: 1,
        bits: 16,
        dpi_x: frame.dpi,
        dpi_y: frame.dpi,
        description: format!("Tuning output {stem}"),
        software: "primefilm process_capture example".to_string(),
        datetime: "2000:01:01 00:00:00".to_string(),
    };
    let mut tiff = TiffWriter::new(BufWriter::new(file), layout).map_err(|e| e.to_string())?;
    let bytes: Vec<u8> = frame.pixels.iter().flat_map(|v| v.to_ne_bytes()).collect();
    tiff.write(&bytes).map_err(|e| e.to_string())?;
    tiff.finish().map_err(|e| e.to_string())?;
    let (partial, file) = write_share_copy(dir, stem, frame).map_err(|e| e.to_string())?;
    partial.publish(file).map_err(|e| e.to_string())?;
    Ok(())
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let mut frame = load(&args.capture)?;
    let s = &args.settings;
    let result = process::process(&mut frame, s, None).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&args.out).map_err(|e| e.to_string())?;
    let name = args
        .capture
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = format!(
        "{name}-{}-b{}-c{}-{}",
        s.stock.value(),
        s.brightness,
        s.contrast,
        s.dust.name()
    );
    save(&frame, &args.out, &stem)?;
    println!(
        "{stem}: black {:.3} D, white {:.3} D ({:?}){}; {} dust specks repaired; {:.1} s",
        result.points.black_density,
        result.points.white_density,
        result.points.source,
        if result.probably_blank {
            ", probably blank"
        } else {
            ""
        },
        result.dust_repaired,
        result.seconds
    );
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("process_capture: {err}");
            ExitCode::FAILURE
        }
    }
}
