//! The single interface all scanner access goes through (constitution Principle III).

pub mod fake;

// Constitution Principle VI: `unsafe` code is allowed only in the module that wraps the SANE
// C library. The crate root denies `unsafe_code`; this attribute lifts that for `sane` alone.
#[allow(unsafe_code)]
pub mod sane;

use std::sync::atomic::AtomicBool;

use serde::{Deserialize, Serialize};

use crate::capability::Capability;
use crate::error::ScanError;

/// A scanner found by the backend.
// A `struct` groups named fields into one type. `Clone` allows explicit copies with
// `.clone()`; `PartialEq` allows `==`; `Serialize`/`Deserialize` (serde) convert it to and
// from JSON using the field names below as keys.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// SANE device name, e.g. `pieusb:libusb:001:004`. Non-empty; used with `--device`.
    pub name: String,
    /// As reported by the driver, e.g. `Reflecta`. May be empty.
    pub vendor: String,
    /// As reported by the driver, e.g. `RPS 10M`. May be empty.
    pub model: String,
    /// SANE device type, e.g. `film scanner`. Named `kind` because `type` is a Rust keyword.
    pub kind: String,
}

// A *trait* is a set of methods a type promises to provide, like an interface in other
// languages. `SaneBackend` (the real scanner) and `FakeBackend` (for tests) both implement
// it, and the command code in `lib.rs` is written against the trait, not either type.
/// Everything the tool needs from a scanner driver.
pub trait ScannerBackend {
    /// List the scanners this backend can see, without opening them.
    fn list_devices(&mut self) -> Result<Vec<DeviceInfo>, ScanError>;

    /// Open `device`, read every option's description and current value, and close it again.
    /// Strictly read-only: nothing on the scanner is changed (FR-006). Stops with
    /// [`ScanError::Interrupted`] when `cancel` becomes true.
    fn read_capabilities(
        &mut self,
        device: &str,
        cancel: &AtomicBool,
    ) -> Result<Vec<Capability>, ScanError>;
}
