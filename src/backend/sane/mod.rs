//! Safe wrapper around SANE, and the `SaneBackend` that drives the real scanner.
//!
//! This is the only module allowed to use `unsafe` (constitution Principle VI). Every
//! `unsafe` block says why it is sound in a `// SAFETY:` comment. Code outside this module sees
//! only safe types: [`Sane`], [`Device`], [`SaneBackend`] and [`SaneSession`].

pub mod config;
pub mod ffi;

use std::ffi::{CStr, CString, c_char, c_void};
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

use super::{
    ColorFormat, DeviceInfo, FrameParams, ScanKind, ScanSession, ScanSettings, ScannerBackend,
    choose_device,
};
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

    // `self` (not `&self`): opening a device *moves* the SANE context into it. The caller can
    // no longer use or drop the `Sane` value; the `Device` now owns it and exits SANE when it
    // is itself dropped. If opening fails, `self` is dropped here, which exits SANE too.
    /// Open a device by its SANE name. The returned [`Device`] takes ownership of this SANE
    /// context, so SANE stays initialized exactly as long as the device is open.
    pub fn open(self, name: &str) -> Result<Device, ScanError> {
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
        // SAFETY: this `Sane` was created by a successful `sane_init`. If a `Device` was
        // opened, it owns this `Sane`, and its own `drop` (which closes the handle) runs before
        // its fields are dropped, so no handle is used after SANE exits.
        unsafe { ffi::sane_exit() };
        INITIALIZED.store(false, Ordering::SeqCst);
    }
}

// Drop order: when a `Device` goes out of scope, Rust first runs `Device`'s own `drop`
// (below, `sane_close`), then drops its fields in declaration order, which ends with `_sane`
// (`sane_exit`). So the handle is always closed before SANE exits, as SANE requires, and the
// compiler makes it impossible to use the `Sane` context separately while a device is open
// (feature 001 used a borrowed `&Sane` with a lifetime for this; owning it is simpler and lets
// a session hold the device, research D1). The field starts with `_` because we never read it;
// it exists only to keep SANE alive.
/// An open scanner. `sane_close` and then `sane_exit` run when it is dropped.
pub struct Device {
    handle: ffi::SaneHandle,
    _sane: Sane,
}

impl Device {
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

/// A value to give an option with [`Device::set_option`].
#[derive(Debug, Clone, PartialEq)]
pub enum OptionValue {
    Bool(bool),
    Int(i32),
    /// A real number; sent as SANE's 16.16 fixed point.
    Fixed(f64),
    Text(String),
}

impl Device {
    /// Find an option's number and descriptor by its name.
    fn find_option(&self, name: &str) -> Result<(i32, &ffi::SaneOptionDescriptor), ScanError> {
        let count = self.option_count()?;
        for index in 1..count {
            // SAFETY: the handle is open and `index` is below the option count.
            let desc = unsafe { ffi::sane_get_option_descriptor(self.handle, index) };
            if desc.is_null() {
                continue;
            }
            // SAFETY: `desc` is non-null and stays valid while the device is open, which the
            // returned reference's tie to `&self` guarantees.
            let desc = unsafe { &*desc };
            // SAFETY: descriptor names are null or NUL-terminated strings owned by SANE.
            if unsafe { c_str(desc.name) } == name {
                return Ok((index, desc));
            }
        }
        Err(ScanError::Sane {
            status: ffi::STATUS_UNSUPPORTED,
            text: format!("the scanner has no option named {name:?}"),
        })
    }

    /// Set option `name` for this session. Returns the value SANE reports back when it had to
    /// round the requested one (`INFO_INEXACT`), so the caller can log it.
    pub fn set_option(&self, name: &str, value: &OptionValue) -> Result<Option<Value>, ScanError> {
        let (index, desc) = self.find_option(name)?;
        let size = usize::try_from(desc.size).unwrap_or(0);
        let mut info: ffi::SaneWord = 0;
        let status = match value {
            OptionValue::Text(text) => {
                // A string option's buffer is `size` bytes, NUL included.
                let mut bytes = text.clone().into_bytes();
                if bytes.len() >= size {
                    return Err(ScanError::Sane {
                        status: ffi::STATUS_INVAL,
                        text: format!("value {text:?} too long for option {name}"),
                    });
                }
                bytes.resize(size, 0);
                self.set_raw(index, bytes.as_mut_ptr().cast::<c_void>(), &mut info)
            }
            number => {
                // Fixed point: SANE stores a real number as an integer scaled by 2^16, so 3600.0
                // becomes 3600 × 65536. `as i32` converts the rounded float to an integer.
                let word: ffi::SaneWord = match number {
                    OptionValue::Bool(b) => ffi::SaneWord::from(*b),
                    OptionValue::Int(n) => *n,
                    OptionValue::Fixed(x) => (x * 65536.0).round() as ffi::SaneWord,
                    OptionValue::Text(_) => 0, // handled by the arm above
                };
                // One word for a single-valued option; a larger buffer would also be fine.
                let mut words = vec![word; (size / 4).max(1)];
                self.set_raw(index, words.as_mut_ptr().cast::<c_void>(), &mut info)
            }
        };
        if status != ffi::STATUS_GOOD {
            return Err(status_error(status));
        }
        if info & ffi::INFO_INEXACT != 0 {
            let value_type = ValueType::from_sane(desc.type_).unwrap_or(ValueType::Int);
            return self.read_value(index, value_type, desc.size);
        }
        Ok(None)
    }

    /// `sane_control_option(SET_VALUE)` on a buffer the caller sized from the descriptor.
    fn set_raw(&self, index: i32, buffer: *mut c_void, info: &mut ffi::SaneWord) -> i32 {
        // SAFETY: the handle is open; `set_option` sizes `buffer` to at least the option's
        // `size` bytes (SANE reads at most that many) and keeps it alive during the call;
        // `info` is a writable `SANE_Int`.
        unsafe { ffi::sane_control_option(self.handle, index, ffi::ACTION_SET_VALUE, buffer, info) }
    }

    /// Start a frame: with pieusb this scans the whole frame (and advances the film if asked)
    /// before returning. Then report the frame's layout.
    pub fn start(&self) -> Result<ffi::SaneParameters, ScanError> {
        // SAFETY: the handle is open.
        let status = unsafe { ffi::sane_start(self.handle) };
        if status != ffi::STATUS_GOOD {
            return Err(status_error(status));
        }
        let mut params = ffi::SaneParameters {
            format: 0,
            last_frame: 0,
            bytes_per_line: 0,
            pixels_per_line: 0,
            lines: 0,
            depth: 0,
        };
        // SAFETY: the handle is open and `params` is a writable `SANE_Parameters`.
        let status = unsafe { ffi::sane_get_parameters(self.handle, &mut params) };
        if status != ffi::STATUS_GOOD {
            self.cancel();
            return Err(status_error(status));
        }
        Ok(params)
    }

    /// Read the next bytes of the frame into `buf`. `Ok(0)` at the end of the frame.
    pub fn read(&self, buf: &mut [u8]) -> Result<usize, ScanError> {
        let max_len = ffi::SaneWord::try_from(buf.len()).unwrap_or(ffi::SaneWord::MAX);
        let mut len: ffi::SaneWord = 0;
        // SAFETY: the handle is open, `buf` is writable for `max_len` bytes (never more than
        // its length), and `len` is a writable `SANE_Int`.
        let status = unsafe { ffi::sane_read(self.handle, buf.as_mut_ptr(), max_len, &mut len) };
        match status {
            ffi::STATUS_GOOD => Ok(usize::try_from(len).unwrap_or(0)),
            ffi::STATUS_EOF => Ok(0),
            other => Err(status_error(other)),
        }
    }

    /// End the current frame (or abandon it). Harmless when no frame is in progress.
    pub fn cancel(&self) {
        // SAFETY: the handle is open; `sane_cancel` may be called at any time.
        unsafe { ffi::sane_cancel(self.handle) };
    }
}

/// The options to set for `settings` on `backend` (data-model.md, "Scan Settings"), in the
/// order to set them: `mode` first, because changing it can change which other options apply.
pub fn option_plan(backend: &str, settings: &ScanSettings) -> Vec<(&'static str, OptionValue)> {
    let (mode, depth) = match settings.color {
        ColorFormat::Rgbi16 => ("RGBI", 16),
        ColorFormat::Rgb8 => ("Color", 8),
    };
    if backend != "pieusb" {
        // SANE's `test` backend (used by `cargo test`) has no RGBI, `advance` or pieusb
        // post-processing options, so only the color format and resolution are set; RGBI
        // becomes 16-bit RGB. A low resolution keeps the test image small but real.
        let mode = if mode == "RGBI" { "Color" } else { mode };
        return vec![
            ("mode", OptionValue::Text(mode.to_string())),
            ("depth", OptionValue::Int(depth)),
            ("resolution", OptionValue::Fixed(25.0)),
        ];
    }
    let mut plan = vec![
        ("mode", OptionValue::Text(mode.to_string())),
        ("depth", OptionValue::Int(depth)),
    ];
    match settings.kind {
        ScanKind::Full | ScanKind::Throwaway => {
            plan.push(("resolution", OptionValue::Fixed(settings.resolution_dpi)));
            plan.push(("preview", OptionValue::Bool(false)));
        }
        // pieusb uses its own preview resolution and never advances during a preview.
        ScanKind::Preview => plan.push(("preview", OptionValue::Bool(true))),
    }
    plan.push(("advance", OptionValue::Bool(settings.advance_after)));
    // Raw data only: pieusb's dust removal and infrared correction are derivative processing
    // (constitution Principle I).
    plan.push(("clean-image", OptionValue::Bool(false)));
    plan.push(("correct-infrared", OptionValue::Bool(false)));
    plan
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

impl Drop for Device {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful `sane_open` and is closed exactly once,
        // here. SANE has not exited yet: the `Sane` it needs is our own field, dropped after
        // this function returns.
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
#[derive(Clone)]
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
                // pieusb skips a scanner it can't open. If another program claimed it after
                // our check in `diagnose_before_init`, that is "busy", not a driver problem.
                Some(usb) if self.usb.is_claimed(&usb) => ScanError::Busy { device: None },
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
        // `open` takes ownership of `sane`; when `open` is dropped at the end of this function
        // (or on any error path), the device is closed and then SANE exits.
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

    type Session = SaneSession;

    fn open(&mut self, device: Option<&str>) -> Result<SaneSession, ScanError> {
        let (sane, devices) = self.init_and_list()?;
        let info = choose_device(&devices, device)?;
        let opened = sane
            .open(&info.name)
            .map_err(|e| self.explain(e, &info.name, "opening the scanner"))?;
        self.log.step(&format!("opened {} for scanning", info.name));
        Ok(SaneSession {
            device: opened,
            info,
            checks: self.clone(),
        })
    }
}

/// An open scanner, ready to capture frames. Dropping it closes the scanner and exits SANE.
pub struct SaneSession {
    device: Device,
    info: DeviceInfo,
    /// The backend's diagnosis helpers and logger, to explain failures precisely.
    checks: SaneBackend,
}

impl SaneSession {
    /// Turn a SANE failure during a frame into the most precise outcome: disconnected, busy,
    /// permission denied, or else "the scanner reported an error".
    fn frame_error(&self, err: ScanError) -> ScanError {
        // We hold the scanner open, so no other program can be using it: "device busy" here
        // means the scanner itself is not ready, e.g. still pulling in a strip that was just
        // inserted (seen on the XA, hardware-validation.md).
        if let ScanError::Sane {
            status: ffi::STATUS_DEVICE_BUSY,
            ..
        } = err
        {
            return ScanError::ScanFailed {
                frame: None,
                text: "the scanner is not ready (still loading the film?); wait a few seconds \
                       and try again"
                    .to_string(),
            };
        }
        match self.checks.explain(err, &self.info.name, "scanning") {
            ScanError::Sane {
                status: ffi::STATUS_CANCELLED,
                ..
            } => ScanError::Interrupted,
            ScanError::Sane { text, .. } => ScanError::ScanFailed { frame: None, text },
            other => other,
        }
    }
}

impl ScanSession for SaneSession {
    fn device(&self) -> &DeviceInfo {
        &self.info
    }

    fn configure(&mut self, settings: &ScanSettings) -> Result<(), ScanError> {
        for (name, value) in option_plan(&self.checks.backend, settings) {
            self.checks.log.step(&format!("set {name} = {value:?}"));
            let rounded = self
                .device
                .set_option(name, &value)
                .map_err(|e| self.frame_error(e))?;
            if let Some(actual) = rounded {
                self.checks
                    .log
                    .step(&format!("  scanner adjusted {name} to {actual:?}"));
            }
        }
        Ok(())
    }

    fn start_frame(&mut self, _cancel: &AtomicBool) -> Result<FrameParams, ScanError> {
        self.checks.log.step("sane_start");
        let params = self.device.start().map_err(|e| self.frame_error(e))?;
        self.checks.log.step(&format!(
            "frame: {} × {} pixels, {} bits, {} bytes per line",
            params.pixels_per_line, params.lines, params.depth, params.bytes_per_line
        ));
        FrameParams::from_sane(
            params.bytes_per_line,
            params.pixels_per_line,
            params.lines,
            params.depth,
        )
        .inspect_err(|_| self.device.cancel())
    }

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, ScanError> {
        self.device.read(buf).map_err(|e| self.frame_error(e))
    }

    fn end_frame(&mut self) {
        self.device.cancel();
    }

    fn options(&mut self) -> Result<Vec<Capability>, ScanError> {
        // A flag nobody sets: reading options takes well under a second.
        let never = AtomicBool::new(false);
        let count = self.device.option_count()?;
        self.device.options(count, &never, &self.checks.log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(kind: ScanKind, color: ColorFormat, advance_after: bool) -> ScanSettings {
        ScanSettings {
            kind,
            resolution_dpi: 3600.0,
            color,
            advance_after,
        }
    }

    fn value<'a>(plan: &'a [(&str, OptionValue)], name: &str) -> Option<&'a OptionValue> {
        plan.iter().find(|(n, _)| *n == name).map(|(_, v)| v)
    }

    /// data-model.md's pieusb mapping, exactly.
    #[test]
    fn pieusb_full_scan_options() {
        let plan = option_plan(
            "pieusb",
            &settings(ScanKind::Full, ColorFormat::Rgbi16, true),
        );
        assert_eq!(plan[0], ("mode", OptionValue::Text("RGBI".into())));
        assert_eq!(value(&plan, "depth"), Some(&OptionValue::Int(16)));
        assert_eq!(
            value(&plan, "resolution"),
            Some(&OptionValue::Fixed(3600.0))
        );
        assert_eq!(value(&plan, "preview"), Some(&OptionValue::Bool(false)));
        assert_eq!(value(&plan, "advance"), Some(&OptionValue::Bool(true)));
        assert_eq!(value(&plan, "clean-image"), Some(&OptionValue::Bool(false)));
        assert_eq!(
            value(&plan, "correct-infrared"),
            Some(&OptionValue::Bool(false))
        );
    }

    #[test]
    fn pieusb_preview_never_advances() {
        let plan = option_plan(
            "pieusb",
            &settings(ScanKind::Preview, ColorFormat::Rgb8, false),
        );
        assert_eq!(plan[0], ("mode", OptionValue::Text("Color".into())));
        assert_eq!(value(&plan, "preview"), Some(&OptionValue::Bool(true)));
        assert_eq!(value(&plan, "advance"), Some(&OptionValue::Bool(false)));
        assert_eq!(value(&plan, "resolution"), None);
    }

    #[test]
    fn test_backend_gets_color_only() {
        let plan = option_plan("test", &settings(ScanKind::Full, ColorFormat::Rgbi16, true));
        assert_eq!(
            plan,
            vec![
                ("mode", OptionValue::Text("Color".into())),
                ("depth", OptionValue::Int(16)),
                ("resolution", OptionValue::Fixed(25.0)),
            ]
        );
    }
}
