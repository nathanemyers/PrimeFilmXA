//! Safe wrapper around SANE, and the `SaneBackend` that drives the real scanner.
//!
//! This is the only module allowed to use `unsafe` (constitution Principle VI). Every
//! `unsafe` block says why it is sound in a `// SAFETY:` comment. Code outside this module sees
//! only safe types: [`Sane`], [`Device`] and [`SaneBackend`].

pub mod config;
pub mod ffi;

use std::ffi::{CStr, CString, c_char, c_void};
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

use super::{DeviceInfo, ScannerBackend};
use crate::capability::{Capability, Constraint, Unit, Value, ValueType, fixed_to_f64};
use crate::error::ScanError;
use crate::report::Logger;
use crate::usb::{PRODUCT_ID, UsbDevice, UsbProbe, VENDOR_ID};

/// SANE allows one active context per process. This flag turns a second `Sane::init` into an
/// error instead of undefined behavior (research D5). An atomic is used because a `static`
/// must be safe to touch from any thread.
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// An initialized SANE library. Created by [`Sane::init`]; `sane_exit` runs when it is dropped.
pub struct Sane {
    version_code: ffi::SaneWord,
}

impl Sane {
    pub fn init() -> Result<Sane, ScanError> {
        if INITIALIZED.swap(true, Ordering::SeqCst) {
            return Err(ScanError::Sane {
                status: -1,
                text: "SANE is already initialized in this process".to_string(),
            });
        }
        let mut version_code: ffi::SaneWord = 0;
        // SAFETY: `version_code` is a valid, writable `SANE_Int`, and a null auth callback is
        // allowed by the SANE standard. INITIALIZED guarantees no other context is active.
        let status = unsafe { ffi::sane_init(&mut version_code, None) };
        if status != ffi::STATUS_GOOD {
            INITIALIZED.store(false, Ordering::SeqCst);
            return Err(status_error(status));
        }
        Ok(Sane { version_code })
    }

    /// The SANE version as `major.minor.build`.
    pub fn version(&self) -> String {
        let code = self.version_code;
        format!("{}.{}.{}", code >> 24, (code >> 16) & 0xff, code & 0xffff)
    }

    /// List local scanners, without opening them.
    pub fn list_devices(&self) -> Result<Vec<DeviceInfo>, ScanError> {
        let mut list: *mut *const ffi::SaneDevice = ptr::null_mut();
        // SAFETY: SANE is initialized (we hold `&self`) and `list` is a valid place for SANE
        // to store its array pointer. `local_only = 1` skips network scanners.
        let status = unsafe { ffi::sane_get_devices(&mut list, 1) };
        if status != ffi::STATUS_GOOD {
            return Err(status_error(status));
        }
        let mut devices = Vec::new();
        if list.is_null() {
            return Ok(devices);
        }
        for i in 0.. {
            // SAFETY: SANE returned a NULL-terminated array, and we stop at the first null
            // entry, so `list.add(i)` never goes past its end. The array stays valid until the
            // next `sane_get_devices` or `sane_exit`, neither of which happens in this loop.
            let device = unsafe { *list.add(i) };
            if device.is_null() {
                break;
            }
            // SAFETY: `device` is non-null and points into SANE's array (see above); its
            // fields are null or NUL-terminated strings owned by SANE.
            let info = unsafe {
                let d = &*device;
                DeviceInfo {
                    name: c_str(d.name),
                    vendor: c_str(d.vendor),
                    model: c_str(d.model),
                    kind: c_str(d.type_),
                }
            };
            devices.push(info);
        }
        Ok(devices)
    }

    /// Open a device by its SANE name. The returned [`Device`] borrows `self`, so it cannot
    /// outlive this SANE context.
    pub fn open(&self, name: &str) -> Result<Device<'_>, ScanError> {
        // `CString` is an owned, NUL-terminated string C can read. Building one fails only if
        // `name` contains a NUL byte, which no SANE device name does.
        let c_name = CString::new(name).map_err(|_| ScanError::Sane {
            status: -1,
            text: format!("device name {name:?} contains a NUL byte"),
        })?;
        let mut handle: ffi::SaneHandle = ptr::null_mut();
        // SAFETY: SANE is initialized, `c_name` is a valid C string that lives until the end
        // of this function, and `handle` is a valid place for SANE to store the handle.
        let status = unsafe { ffi::sane_open(c_name.as_ptr(), &mut handle) };
        if status != ffi::STATUS_GOOD {
            return Err(status_error(status));
        }
        Ok(Device {
            handle,
            _sane: self,
        })
    }
}

// `Drop` is Rust's destructor: `drop` runs automatically when a value goes out of scope,
// whether the function returned normally, returned early with `?`, or is unwinding from a
// panic. Putting `sane_exit` here means we can never forget to call it (this pattern is called
// RAII, "resource acquisition is initialization").
impl Drop for Sane {
    fn drop(&mut self) {
        // SAFETY: this `Sane` was created by a successful `sane_init`, and every `Device`
        // borrowing it has already been dropped (the borrow checker guarantees that), so no
        // handle is used after SANE exits.
        unsafe { ffi::sane_exit() };
        INITIALIZED.store(false, Ordering::SeqCst);
    }
}

// Lifetimes: `'a` names "how long the borrowed `Sane` lives". `Device<'a>` holds a
// `&'a Sane`, so the compiler refuses any code where a `Device` would still exist after its
// `Sane` is dropped. SANE forbids using a handle after `sane_exit`; this turns that rule into a
// compile error. It is the one explicit lifetime in the project (plan.md, Complexity Tracking).
// The field starts with `_` because we never read it; it exists only to hold the borrow.
/// An open scanner. `sane_close` runs when it is dropped.
pub struct Device<'a> {
    handle: ffi::SaneHandle,
    _sane: &'a Sane,
}

impl Device<'_> {
    /// Number of options, including option 0 (which holds the count itself).
    pub fn option_count(&self) -> Result<i32, ScanError> {
        let mut count: ffi::SaneWord = 0;
        // SAFETY: the handle is open; option 0 is always a single `SANE_Int` (SANE standard),
        // and `count` is a writable `SANE_Int`. GET_VALUE never changes the scanner.
        let status = unsafe {
            ffi::sane_control_option(
                self.handle,
                0,
                ffi::ACTION_GET_VALUE,
                (&mut count as *mut ffi::SaneWord).cast::<c_void>(),
                ptr::null_mut(),
            )
        };
        if status != ffi::STATUS_GOOD {
            return Err(status_error(status));
        }
        Ok(count)
    }

    /// Read every option's description and, where possible, its current value. Read-only:
    /// only `ACTION_GET_VALUE` is ever used (FR-006). Checks `cancel` between options.
    pub fn options(
        &self,
        count: i32,
        cancel: &AtomicBool,
        log: &Logger,
    ) -> Result<Vec<Capability>, ScanError> {
        let mut options = Vec::new();
        for index in 1..count {
            if cancel.load(Ordering::SeqCst) {
                return Err(ScanError::Interrupted);
            }
            // SAFETY: the handle is open and `index` is below the option count.
            let desc = unsafe { ffi::sane_get_option_descriptor(self.handle, index) };
            if desc.is_null() {
                return Err(ScanError::Sane {
                    status: -1,
                    text: format!("no descriptor for option {index}"),
                });
            }
            // SAFETY: `desc` is non-null and SANE keeps it valid while the device is open.
            let desc = unsafe { &*desc };
            options.push(self.capability(index, desc, log)?);
        }
        Ok(options)
    }

    fn capability(
        &self,
        index: i32,
        desc: &ffi::SaneOptionDescriptor,
        log: &Logger,
    ) -> Result<Capability, ScanError> {
        // SAFETY: SANE descriptor strings are null or NUL-terminated and valid while the
        // device is open.
        let (name, title, description) =
            unsafe { (c_str(desc.name), c_str(desc.title), c_str(desc.desc)) };

        let value_type = ValueType::from_sane(desc.type_).ok_or_else(|| ScanError::Sane {
            status: -1,
            text: format!("option {index} ({name}) has unknown type {}", desc.type_),
        })?;
        let unit = Unit::from_sane(desc.unit);
        if unit == Unit::None && desc.unit != ffi::UNIT_NONE {
            log.step(&format!(
                "option {name}: unknown unit {}, shown as none",
                desc.unit
            ));
        }
        let active = desc.cap & ffi::CAP_INACTIVE == 0;

        // SAFETY: `desc` is a valid descriptor from SANE (see `options`).
        let constraint = unsafe { read_constraint(desc, value_type) };

        let has_value = !matches!(value_type, ValueType::Button | ValueType::Group);
        let value = if active && has_value {
            self.read_value(index, value_type, desc.size)?
        } else {
            None
        };

        Ok(Capability {
            index,
            name,
            title,
            description,
            value_type,
            unit,
            size: desc.size,
            active,
            settable: desc.cap & ffi::CAP_SOFT_SELECT != 0,
            automatic: desc.cap & ffi::CAP_AUTOMATIC != 0,
            advanced: desc.cap & ffi::CAP_ADVANCED != 0,
            constraint,
            value,
        })
    }

    /// Read an option's current value. `Ok(None)` when the backend says it can't be read.
    fn read_value(
        &self,
        index: i32,
        value_type: ValueType,
        size: i32,
    ) -> Result<Option<Value>, ScanError> {
        let size = usize::try_from(size).unwrap_or(0);
        let value = match value_type {
            ValueType::Bool | ValueType::Int | ValueType::Fixed => {
                // One `i32` per value; at least one so the buffer is never empty. Using
                // `Vec<i32>` (not bytes) gives SANE correctly aligned words.
                let mut words = vec![0 as ffi::SaneWord; (size / 4).max(1)];
                let status = self.get_value(index, words.as_mut_ptr().cast::<c_void>());
                if let Some(result) = unreadable(status) {
                    return result;
                }
                match value_type {
                    ValueType::Bool => Value::Bool(words.iter().map(|w| *w != 0).collect()),
                    ValueType::Fixed => {
                        Value::Fixed(words.iter().map(|w| fixed_to_f64(*w)).collect())
                    }
                    _ => Value::Int(words),
                }
            }
            ValueType::String => {
                let mut bytes = vec![0u8; size.max(1)];
                let status = self.get_value(index, bytes.as_mut_ptr().cast::<c_void>());
                if let Some(result) = unreadable(status) {
                    return result;
                }
                // Stops at the first NUL; if SANE wrote none, use the whole buffer rather than
                // reading past its end.
                let text = match CStr::from_bytes_until_nul(&bytes) {
                    Ok(s) => s.to_string_lossy().into_owned(),
                    Err(_) => String::from_utf8_lossy(&bytes).into_owned(),
                };
                Value::Text(text)
            }
            ValueType::Button | ValueType::Group => return Ok(None),
        };
        Ok(Some(value))
    }

    /// `sane_control_option(GET_VALUE)`. Callers pass a buffer of at least the option's
    /// `size` bytes, which is the only way this module calls it.
    fn get_value(&self, index: i32, buffer: *mut c_void) -> i32 {
        // SAFETY: the handle is open, and `read_value` sizes `buffer` from the descriptor's
        // `size` (at least that many bytes), which is all SANE writes for GET_VALUE.
        unsafe {
            ffi::sane_control_option(
                self.handle,
                index,
                ffi::ACTION_GET_VALUE,
                buffer,
                ptr::null_mut(),
            )
        }
    }
}

/// Decide what a failed value read means: `None` if it succeeded; `Some(Ok(None))` if the
/// backend just doesn't support reading this option (the value is shown as null);
/// `Some(Err(..))` for real failures such as I/O errors.
fn unreadable(status: i32) -> Option<Result<Option<Value>, ScanError>> {
    match status {
        ffi::STATUS_GOOD => None,
        ffi::STATUS_UNSUPPORTED | ffi::STATUS_INVAL => Some(Ok(None)),
        other => Some(Err(status_error(other))),
    }
}

impl Drop for Device<'_> {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful `sane_open` and is closed exactly once,
        // here. The `'a` borrow guarantees SANE has not exited yet.
        unsafe { ffi::sane_close(self.handle) };
    }
}

/// Convert a descriptor's C constraint union into a Rust [`Constraint`].
///
/// # Safety
/// `desc` must be a descriptor returned by SANE for a device that is still open.
unsafe fn read_constraint(desc: &ffi::SaneOptionDescriptor, value_type: ValueType) -> Constraint {
    if desc.constraint.is_null() {
        return Constraint::None;
    }
    let number = |word: ffi::SaneWord| {
        if value_type == ValueType::Fixed {
            fixed_to_f64(word)
        } else {
            word as f64
        }
    };
    match desc.constraint_type {
        ffi::CONSTRAINT_RANGE => {
            // SAFETY: for CONSTRAINT_RANGE the union holds a non-null `SANE_Range*`.
            let range = unsafe { &*desc.constraint.cast::<ffi::SaneRange>() };
            Constraint::Range {
                min: number(range.min),
                max: number(range.max),
                step: number(range.quant),
            }
        }
        ffi::CONSTRAINT_WORD_LIST => {
            let words = desc.constraint.cast::<ffi::SaneWord>();
            // SAFETY: for CONSTRAINT_WORD_LIST the union points to a word array whose first
            // element is the number of words that follow.
            let values = unsafe {
                let count = usize::try_from(*words).unwrap_or(0);
                (1..=count).map(|i| number(*words.add(i))).collect()
            };
            Constraint::NumberList { values }
        }
        ffi::CONSTRAINT_STRING_LIST => {
            let list = desc.constraint.cast::<*const c_char>();
            let mut values = Vec::new();
            for i in 0.. {
                // SAFETY: for CONSTRAINT_STRING_LIST the union points to a NULL-terminated
                // array of C strings; we stop at the null.
                let item = unsafe { *list.add(i) };
                if item.is_null() {
                    break;
                }
                // SAFETY: `item` is a non-null, NUL-terminated string owned by SANE.
                values.push(unsafe { c_str(item) });
            }
            Constraint::StringList { values }
        }
        _ => Constraint::None,
    }
}

/// Copy a C string into an owned Rust `String`. Null becomes an empty string; invalid UTF-8 is
/// replaced with `�` rather than failing.
///
/// # Safety
/// `ptr` must be null or point to a NUL-terminated string that stays valid during the call.
unsafe fn c_str(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: guaranteed by this function's contract (see above).
    unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

/// Turn a SANE status code into a [`ScanError::Sane`] with SANE's own description.
fn status_error(status: i32) -> ScanError {
    // SAFETY: `sane_strstatus` accepts any status and returns a pointer to a static string
    // (or null for numbers it doesn't know, which `c_str` handles).
    let text = unsafe { c_str(ffi::sane_strstatus(status)) };
    ScanError::Sane { status, text }
}

/// Where Ubuntu and source builds install SANE backends.
const DRIVER_DIRS: [&str; 3] = [
    "/usr/lib/x86_64-linux-gnu/sane",
    "/usr/lib/sane",
    "/usr/local/lib/sane",
];

/// Talks to a real scanner through SANE. Only devices from `backend` are listed (FR-002).
pub struct SaneBackend {
    backend: String,
    log: Logger,
    usb: UsbProbe,
}

impl SaneBackend {
    pub fn new(backend: &str, log: Logger) -> SaneBackend {
        SaneBackend {
            backend: backend.to_string(),
            log,
            usb: UsbProbe::system(),
        }
    }

    /// The `/sys` checks only make sense for the PrimeFilm XA's driver, not e.g. `test`.
    fn is_pieusb(&self) -> bool {
        self.backend == "pieusb"
    }

    /// Checks before SANE is loaded, in the order of contracts/cli.md. Returns the scanner's
    /// USB location when it is present.
    fn diagnose_before_init(&self) -> Result<Option<UsbDevice>, ScanError> {
        if !self.is_pieusb() {
            return Ok(None);
        }
        let driver_file = format!("libsane-{}.so.1", self.backend);
        if !DRIVER_DIRS
            .iter()
            .any(|dir| Path::new(dir).join(&driver_file).exists())
        {
            return Err(ScanError::DriverUnavailable {
                reason: "driver file missing".to_string(),
            });
        }

        let usb = self
            .usb
            .find(VENDOR_ID, PRODUCT_ID)
            .ok_or(ScanError::NotFound)?;
        let claimed = self.usb.is_claimed(&usb);
        self.log.step(&format!(
            "USB {VENDOR_ID}:{PRODUCT_ID} found at bus {:03} device {:03}, interface {}",
            usb.bus,
            usb.device,
            if claimed { "claimed" } else { "not claimed" }
        ));
        if claimed {
            return Err(ScanError::Busy { device: None });
        }
        // Other open errors (e.g. the device vanished) are left for SANE to report.
        if let Err(err) = usb.check_node_access()
            && err.kind() == std::io::ErrorKind::PermissionDenied
        {
            return Err(ScanError::PermissionDenied {
                path: usb.node_path().display().to_string(),
            });
        }
        Ok(Some(usb))
    }

    /// Diagnose, start SANE and list this backend's devices. Returns the `Sane` context so the
    /// caller decides how long it stays open.
    fn init_and_list(&self) -> Result<(Sane, Vec<DeviceInfo>), ScanError> {
        let usb = self.diagnose_before_init()?;
        let sane = Sane::init()?;
        self.log
            .step(&format!("sane_init ok (SANE {})", sane.version()));

        let prefix = format!("{}:", self.backend);
        // `into_iter()` consumes the list (takes ownership of each `DeviceInfo`), so the kept
        // ones are moved, not copied.
        let devices: Vec<DeviceInfo> = sane
            .list_devices()?
            .into_iter()
            .filter(|d| d.name.starts_with(&prefix))
            .collect();

        if devices.is_empty() {
            return Err(match usb {
                Some(_) => ScanError::DriverUnavailable {
                    reason: "scanner on USB but pieusb did not recognize it (check the model \
                             number in sane.d/pieusb.conf; run with -vv)"
                        .to_string(),
                },
                None => ScanError::NotFound,
            });
        }
        Ok((sane, devices))
    }

    /// Give a SANE failure a precise meaning. Never retries (Principle II).
    fn explain(&self, err: ScanError, device: &str, during: &str) -> ScanError {
        // A `ScanError::Sane` while the scanner has vanished from USB means it disconnected.
        let gone = self.is_pieusb() && self.usb.find(VENDOR_ID, PRODUCT_ID).is_none();
        match err {
            ScanError::Sane { .. } if gone => ScanError::Disconnected {
                during: during.to_string(),
            },
            ScanError::Sane {
                status: ffi::STATUS_DEVICE_BUSY,
                ..
            } => ScanError::Busy {
                device: Some(device.to_string()),
            },
            ScanError::Sane {
                status: ffi::STATUS_ACCESS_DENIED,
                ..
            } => ScanError::PermissionDenied {
                path: self
                    .usb
                    .find(VENDOR_ID, PRODUCT_ID)
                    .map(|u| u.node_path().display().to_string())
                    .unwrap_or_else(|| device.to_string()),
            },
            other => other,
        }
    }
}

impl ScannerBackend for SaneBackend {
    fn list_devices(&mut self) -> Result<Vec<DeviceInfo>, ScanError> {
        // `_sane` is dropped (SANE exits) at the end of this function.
        let (_sane, devices) = self.init_and_list()?;
        Ok(devices)
    }

    fn read_capabilities(
        &mut self,
        device: &str,
        cancel: &AtomicBool,
    ) -> Result<Vec<Capability>, ScanError> {
        let (sane, devices) = self.init_and_list()?;
        if !devices.iter().any(|d| d.name == device) {
            return Err(ScanError::NotFound);
        }
        // Rust drops local variables in reverse order of declaration, so `open` (declared
        // after `sane`) is closed before SANE exits, on success and on every error path.
        let open = sane
            .open(device)
            .map_err(|e| self.explain(e, device, "opening the scanner"))?;
        let count = open
            .option_count()
            .map_err(|e| self.explain(e, device, "reading options"))?;
        self.log.step(&format!(
            "opened {device}; reading {} options",
            count.saturating_sub(1)
        ));
        open.options(count, cancel, &self.log)
            .map_err(|e| self.explain(e, device, "reading options"))
    }
}
