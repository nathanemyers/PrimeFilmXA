//! A pretend scanner for tests (constitution Principle IV). Selected with `--simulate`.

use std::sync::atomic::{AtomicBool, Ordering};

use super::{DeviceInfo, ScannerBackend};
use crate::capability::Capability;
use crate::cli::Scenario;
use crate::error::ScanError;

/// Simulated option list. Hand-authored for now; see research.md R4.
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
        vendor: "Reflecta".to_string(),
        model: "RPS 10M".to_string(),
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
