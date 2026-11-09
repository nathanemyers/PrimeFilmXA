//! A pretend scanner for tests (constitution Principle IV). Selected with `--simulate`.

use std::sync::atomic::{AtomicBool, Ordering};

use super::{DeviceInfo, ScannerBackend};
use crate::capability::Capability;
use crate::cli::Scenario;
use crate::error::ScanError;

/// Simulated option list: a real `primefilm --json capabilities` recording from the
/// PrimeFilm XA (2026-11-09), so tests see the same 43 options the scanner reports.
const FIXTURE: &str = include_str!("fixtures/primefilm-xa.json");

const FAKE_DEVICE_NAME: &str = "fake:primefilm-xa";

pub struct FakeBackend {
    scenario: Scenario,
}

impl FakeBackend {
    pub fn new(scenario: Scenario) -> FakeBackend {
        FakeBackend { scenario }
    }
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
            Scenario::Ready | Scenario::Disconnect | Scenario::Interrupt => Ok(vec![fake_device()]),
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
            _ => {
                // The fixture is compiled into the binary and parsed by the CLI tests
                // (`--simulate ready capabilities`), so a broken fixture fails `cargo test`
                // rather than reaching a user.
                #[allow(clippy::expect_used)]
                let options = serde_json::from_str(FIXTURE)
                    .expect("fixtures/primefilm-xa.json is a valid Vec<Capability>");
                Ok(options)
            }
        }
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

    #[test]
    fn ready_reads_capabilities() {
        let mut backend = FakeBackend::new(Scenario::Ready);
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
