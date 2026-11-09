//! A pretend scanner for tests (constitution Principle IV). Selected with `--simulate`.
//!
//! For scanning it simulates a film strip (research D12): frames are generated images that
//! differ from frame to frame, the film position moves only when a capture asks to advance,
//! and, like the PrimeFilm XA (hardware experiments E4, E5), it stays on the last frame when
//! asked to advance past it and shows the bare light source when there is no film.

use std::sync::atomic::{AtomicBool, Ordering};

use super::{
    ColorFormat, DeviceInfo, FrameParams, ScanKind, ScanSession, ScanSettings, ScannerBackend,
    choose_device,
};
use crate::capability::Capability;
use crate::cli::Scenario;
use crate::error::ScanError;
use crate::preset::frame_pixels;

/// Simulated option list: a real `primefilm --json capabilities` recording from the
/// PrimeFilm XA (2026-11-09), so tests see the same 43 options the scanner reports.
const FIXTURE: &str = include_str!("fixtures/primefilm-xa.json");

const FAKE_DEVICE_NAME: &str = "fake:primefilm-xa";

/// Frames on the simulated strip when `--simulate-frames` is not given.
pub const DEFAULT_FRAMES: u32 = 6;

pub struct FakeBackend {
    scenario: Scenario,
    frames: u32,
}

impl FakeBackend {
    /// A simulated scanner behaving as `scenario`, with `frames` frames of film loaded.
    pub fn new(scenario: Scenario, frames: u32) -> FakeBackend {
        FakeBackend { scenario, frames }
    }
}

fn fixture_options() -> Vec<Capability> {
    // The fixture is compiled into the binary and parsed by the CLI tests
    // (`--simulate ready capabilities`), so a broken fixture fails `cargo test` rather than
    // reaching a user.
    #[allow(clippy::expect_used)]
    serde_json::from_str(FIXTURE).expect("fixtures/primefilm-xa.json is a valid Vec<Capability>")
}

fn fake_device() -> DeviceInfo {
    DeviceInfo {
        name: FAKE_DEVICE_NAME.to_string(),
        vendor: "PIE".to_string(),
        model: "MF Scanner".to_string(),
        kind: "film scanner".to_string(),
    }
}

// `impl Trait for Type` provides the trait's methods for this type.
impl ScannerBackend for FakeBackend {
    fn list_devices(&mut self) -> Result<Vec<DeviceInfo>, ScanError> {
        match self.scenario {
            Scenario::Ready
            | Scenario::Disconnect
            | Scenario::Interrupt
            | Scenario::NoFilm
            | Scenario::Jam
            | Scenario::EmptyGate => Ok(vec![fake_device()]),
            Scenario::NoScanner => Err(ScanError::NotFound),
            Scenario::Busy => Err(ScanError::Busy { device: None }),
            Scenario::PermissionDenied => Err(ScanError::PermissionDenied {
                path: "/dev/bus/usb/001/004".to_string(),
            }),
            Scenario::DriverMissing => Err(ScanError::DriverUnavailable {
                reason: "simulated: driver file missing".to_string(),
            }),
        }
    }

    fn read_capabilities(
        &mut self,
        device: &str,
        cancel: &AtomicBool,
    ) -> Result<Vec<Capability>, ScanError> {
        // Scenarios that fail before a device is found fail here the same way.
        let devices = self.list_devices()?;
        if !devices.iter().any(|d| d.name == device) {
            return Err(ScanError::NotFound);
        }
        if cancel.load(Ordering::SeqCst) {
            return Err(ScanError::Interrupted);
        }
        match self.scenario {
            Scenario::Disconnect => Err(ScanError::Disconnected {
                during: "reading options".to_string(),
            }),
            Scenario::Interrupt => Err(ScanError::Interrupted),
            _ => Ok(fixture_options()),
        }
    }

    type Session = FakeSession;

    fn open(&mut self, device: Option<&str>) -> Result<FakeSession, ScanError> {
        let info = choose_device(&self.list_devices()?, device)?;
        Ok(FakeSession::new(self.scenario, self.frames, info))
    }
}

/// A simulated open scanner with a film strip in it.
pub struct FakeSession {
    scenario: Scenario,
    frames: u32,
    /// Frame currently in the gate, from 1.
    position: u32,
    info: DeviceInfo,
    settings: Option<ScanSettings>,
    /// The frame being read, and how much of it has been read.
    data: Vec<u8>,
    read_pos: usize,
}

impl FakeSession {
    pub fn new(scenario: Scenario, frames: u32, info: DeviceInfo) -> FakeSession {
        FakeSession {
            scenario,
            frames,
            position: 1,
            info,
            settings: None,
            data: Vec::new(),
            read_pos: 0,
        }
    }

    /// The frame currently in the gate (for tests).
    pub fn position(&self) -> u32 {
        self.position
    }

    /// Put the film at `frame` (for tests).
    pub fn set_position(&mut self, frame: u32) {
        self.position = frame;
    }
}

/// Sample value at a pixel: a pattern that shifts with the frame number, so different frames
/// have different fingerprints, kept below 60% of full scale so it never looks like an empty
/// gate. Returned as a fraction of full scale.
fn pattern(frame: u32, x: u32, y: u32, channel: u32) -> f64 {
    let v = (x * 7 + y * 3 + frame * 37 + channel * 11) % 64;
    0.6 * f64::from(v) / 64.0
}

impl ScanSession for FakeSession {
    fn device(&self) -> &DeviceInfo {
        &self.info
    }

    fn configure(&mut self, settings: &ScanSettings) -> Result<(), ScanError> {
        self.settings = Some(settings.clone());
        Ok(())
    }

    fn start_frame(&mut self, cancel: &AtomicBool) -> Result<FrameParams, ScanError> {
        let settings = self.settings.clone().ok_or_else(|| ScanError::ScanFailed {
            frame: None,
            text: "simulated scanner was not configured".to_string(),
        })?;
        match (self.scenario, self.position) {
            (Scenario::Jam, 3) => {
                return Err(ScanError::ScanFailed {
                    frame: None,
                    text: "simulated jam".to_string(),
                });
            }
            (Scenario::Disconnect, 3) => {
                return Err(ScanError::Disconnected {
                    during: "scanning".to_string(),
                });
            }
            // As if Ctrl-C were pressed while frame 2 is being scanned.
            (Scenario::Interrupt, 2) => cancel.store(true, Ordering::SeqCst),
            _ => {}
        }

        // Frames are 1/50 of the real pixel size, so tests stay fast; previews are 300 dpi.
        let dpi = match settings.kind {
            ScanKind::Preview => 300.0,
            _ => settings.resolution_dpi,
        };
        let (width, height) = frame_pixels((dpi / 50.0).max(12.0));
        let (channels, bits) = match settings.color {
            ColorFormat::Rgbi16 => (4u32, 16u16),
            ColorFormat::Rgb8 => (3, 8),
        };
        let empty = self.scenario == Scenario::NoFilm
            || (self.scenario == Scenario::EmptyGate && self.position >= 4);

        self.data.clear();
        self.read_pos = 0;
        for y in 0..height {
            for x in 0..width {
                for c in 0..channels {
                    let mut fraction = if empty {
                        0.8
                    } else {
                        pattern(self.position, x, y, c)
                    };
                    // The very first sample holds the frame number, so tests can check order.
                    if x == 0 && y == 0 && c == 0 && !empty {
                        fraction = f64::from(self.position) / f64::from(u16::MAX);
                    }
                    if bits == 16 {
                        let v = (fraction * 65535.0).round() as u16;
                        self.data.extend_from_slice(&v.to_ne_bytes());
                    } else {
                        let v = if x == 0 && y == 0 && c == 0 && !empty {
                            // Frame number directly, for 8-bit frames too.
                            self.position.min(255) as u8
                        } else {
                            (fraction * 255.0).round() as u8
                        };
                        self.data.push(v);
                    }
                }
            }
        }

        // Like the XA, the film moves at the end of the capture, and not past the last frame.
        if settings.advance_after && !empty && self.position < self.frames {
            self.position += 1;
        }

        let bytes_per_line = width * channels * u32::from(bits / 8);
        FrameParams::from_sane(
            bytes_per_line as i32,
            width as i32,
            height as i32,
            i32::from(bits),
        )
    }

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, ScanError> {
        let rest = &self.data[self.read_pos..];
        let n = rest.len().min(buf.len());
        buf[..n].copy_from_slice(&rest[..n]);
        self.read_pos += n;
        Ok(n)
    }

    fn end_frame(&mut self) {
        self.data.clear();
        self.read_pos = 0;
    }

    fn options(&mut self) -> Result<Vec<Capability>, ScanError> {
        Ok(fixture_options())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_parses() {
        let options: Vec<Capability> = serde_json::from_str(FIXTURE).unwrap();
        assert!(options.iter().any(|o| o.name == "resolution"));
        assert!(options.iter().any(|o| !o.active && o.value.is_none()));
    }

    /// The summary rules (research D8) applied to the real recording from the XA.
    #[test]
    fn summary_of_real_recording() {
        use crate::capability::{CapabilitySummary, Constraint, Infrared};

        let options: Vec<Capability> = serde_json::from_str(FIXTURE).unwrap();
        assert_eq!(options.len(), 43);
        let summary = CapabilitySummary::from_options(&options);
        assert_eq!(
            summary.resolutions_dpi,
            Some(Constraint::Range {
                min: 25.0,
                max: 10000.0,
                step: 1.0
            })
        );
        assert!(summary.color_modes.unwrap().contains(&"RGBI".to_string()));
        assert_eq!(summary.bit_depths, Some(vec![1, 8, 16]));
        let area = summary.scan_area_mm.unwrap();
        assert!((area.width - 36.5).abs() < 0.01 && (area.height - 24.3).abs() < 0.01);
        assert_eq!(summary.infrared, Infrared::Available);
    }

    fn full(advance_after: bool) -> ScanSettings {
        ScanSettings {
            kind: ScanKind::Full,
            resolution_dpi: 3600.0,
            color: ColorFormat::Rgbi16,
            advance_after,
        }
    }

    /// Capture one frame and return its first 16-bit sample (the frame number).
    fn capture(session: &mut FakeSession) -> u16 {
        let cancel = AtomicBool::new(false);
        let params = session.start_frame(&cancel).unwrap();
        let mut data = vec![0; params.image_bytes() as usize];
        let mut filled = 0;
        while filled < data.len() {
            filled += session.read(&mut data[filled..]).unwrap();
        }
        session.end_frame();
        u16::from_ne_bytes([data[0], data[1]])
    }

    #[test]
    fn film_moves_only_when_asked_and_stops_at_the_end() {
        let mut backend = FakeBackend::new(Scenario::Ready, 3);
        let mut session = backend.open(None).unwrap();
        session.configure(&full(false)).unwrap();
        assert_eq!(capture(&mut session), 1);
        assert_eq!(capture(&mut session), 1);
        session.configure(&full(true)).unwrap();
        assert_eq!(capture(&mut session), 1);
        assert_eq!(capture(&mut session), 2);
        assert_eq!(capture(&mut session), 3);
        // Past the last frame, the film stays put.
        assert_eq!(capture(&mut session), 3);
        assert_eq!(session.position(), 3);
    }

    #[test]
    fn jam_at_frame_3() {
        let mut backend = FakeBackend::new(Scenario::Jam, 6);
        let mut session = backend.open(None).unwrap();
        session.configure(&full(true)).unwrap();
        session.set_position(3);
        let cancel = AtomicBool::new(false);
        assert!(matches!(
            session.start_frame(&cancel),
            Err(ScanError::ScanFailed { .. })
        ));
    }

    #[test]
    fn ready_reads_capabilities() {
        let mut backend = FakeBackend::new(Scenario::Ready, DEFAULT_FRAMES);
        let cancel = AtomicBool::new(false);
        assert!(
            !backend
                .read_capabilities(FAKE_DEVICE_NAME, &cancel)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            backend.read_capabilities("nope", &cancel),
            Err(ScanError::NotFound)
        );
        cancel.store(true, Ordering::SeqCst);
        assert_eq!(
            backend.read_capabilities(FAKE_DEVICE_NAME, &cancel),
            Err(ScanError::Interrupted)
        );
    }
}
