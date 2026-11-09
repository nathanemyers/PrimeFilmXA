//! Raw declarations of the parts of SANE's C API we use, mirroring `/usr/include/sane/sane.h`.
//!
//! FFI ("foreign function interface") is how Rust calls code written in another language.
//! Nothing here is safe to use directly: the compiler cannot check what C does with the
//! pointers we hand it. The safe wrapper in `mod.rs` is the only code that calls these.
//!
//! Feature 001 only read options (`ACTION_GET_VALUE`). Feature 002 adds what scanning needs:
//! setting options (`ACTION_SET_VALUE`; values last for the session only, nothing is stored
//! on the scanner) and `sane_start` / `sane_get_parameters` / `sane_read` / `sane_cancel`.

use std::ffi::{c_char, c_void};

/// `SANE_Word`, `SANE_Int`, `SANE_Bool` and `SANE_Fixed` are all a C `int` (32 bits).
pub type SaneWord = i32;

/// An opaque pointer SANE gives us for an open device. We never look inside it.
pub type SaneHandle = *mut c_void;

// `#[repr(C)]` tells Rust to lay out the struct's fields in memory exactly as a C compiler
// would (same order, same padding). Without it Rust may reorder fields, and SANE would read
// garbage. `*const c_char` is a *raw pointer* to a C string: unlike a Rust reference (`&str`)
// it may be null or dangling, so reading through it requires `unsafe`.

/// `SANE_Device`: one scanner found by `sane_get_devices`.
#[repr(C)]
pub struct SaneDevice {
    pub name: *const c_char,
    pub vendor: *const c_char,
    pub model: *const c_char,
    /// `type` in C; renamed because `type` is a Rust keyword.
    pub type_: *const c_char,
}

/// `SANE_Range`: allowed values `min..=max` in steps of `quant` (0 = any value).
#[repr(C)]
pub struct SaneRange {
    pub min: SaneWord,
    pub max: SaneWord,
    pub quant: SaneWord,
}

/// `SANE_Option_Descriptor`: describes one option (setting) of an open device.
#[repr(C)]
pub struct SaneOptionDescriptor {
    pub name: *const c_char,
    pub title: *const c_char,
    pub desc: *const c_char,
    /// One of the `TYPE_*` constants.
    pub type_: i32,
    /// One of the `UNIT_*` constants.
    pub unit: i32,
    /// Size of the value in bytes.
    pub size: SaneWord,
    /// Bit flags, see the `CAP_*` constants.
    pub cap: SaneWord,
    /// One of the `CONSTRAINT_*` constants; says how to read `constraint`.
    pub constraint_type: i32,
    /// In C this is a union of three pointers (`string_list`, `word_list`, `range`). All three
    /// are pointers of the same size, so one untyped pointer has the same layout; the safe
    /// wrapper casts it to the right type after checking `constraint_type`.
    pub constraint: *const c_void,
}

/// `SANE_Parameters`: the layout of the frame `sane_start` has begun.
#[repr(C)]
pub struct SaneParameters {
    /// One of the `SANE_FRAME_*` values. pieusb reports RGBI as RGB, so we don't rely on it.
    pub format: i32,
    pub last_frame: i32,
    pub bytes_per_line: i32,
    pub pixels_per_line: i32,
    pub lines: i32,
    pub depth: i32,
}

// `SANE_Status` values.
pub const STATUS_GOOD: i32 = 0;
pub const STATUS_UNSUPPORTED: i32 = 1;
pub const STATUS_CANCELLED: i32 = 2;
pub const STATUS_DEVICE_BUSY: i32 = 3;
pub const STATUS_INVAL: i32 = 4;
pub const STATUS_EOF: i32 = 5;
pub const STATUS_JAMMED: i32 = 6;
pub const STATUS_NO_DOCS: i32 = 7;
pub const STATUS_COVER_OPEN: i32 = 8;
pub const STATUS_IO_ERROR: i32 = 9;
pub const STATUS_NO_MEM: i32 = 10;
pub const STATUS_ACCESS_DENIED: i32 = 11;

// `SANE_Value_Type` values.
pub const TYPE_BOOL: i32 = 0;
pub const TYPE_INT: i32 = 1;
pub const TYPE_FIXED: i32 = 2;
pub const TYPE_STRING: i32 = 3;
pub const TYPE_BUTTON: i32 = 4;
pub const TYPE_GROUP: i32 = 5;

// `SANE_Unit` values.
pub const UNIT_NONE: i32 = 0;
pub const UNIT_PIXEL: i32 = 1;
pub const UNIT_BIT: i32 = 2;
pub const UNIT_MM: i32 = 3;
pub const UNIT_DPI: i32 = 4;
pub const UNIT_PERCENT: i32 = 5;
pub const UNIT_MICROSECOND: i32 = 6;

// `SANE_CAP_*` bit flags in `SaneOptionDescriptor::cap`.
pub const CAP_SOFT_SELECT: i32 = 1;
pub const CAP_HARD_SELECT: i32 = 2;
pub const CAP_SOFT_DETECT: i32 = 4;
pub const CAP_EMULATED: i32 = 8;
pub const CAP_AUTOMATIC: i32 = 16;
pub const CAP_INACTIVE: i32 = 32;
pub const CAP_ADVANCED: i32 = 64;

// `SANE_Constraint_Type` values.
pub const CONSTRAINT_NONE: i32 = 0;
pub const CONSTRAINT_RANGE: i32 = 1;
pub const CONSTRAINT_WORD_LIST: i32 = 2;
pub const CONSTRAINT_STRING_LIST: i32 = 3;

/// `SANE_ACTION_GET_VALUE`: read an option.
pub const ACTION_GET_VALUE: i32 = 0;
/// `SANE_ACTION_SET_VALUE`: change an option for this session.
pub const ACTION_SET_VALUE: i32 = 1;

// `SANE_INFO_*` bits SANE sets after a SET_VALUE.
/// The value was rounded to one the scanner supports; read it back to see it.
pub const INFO_INEXACT: i32 = 1;
/// Other options changed as a result (e.g. became active or inactive).
pub const INFO_RELOAD_OPTIONS: i32 = 2;

/// `SANE_Auth_Callback`: called by network backends to ask for a password. We pass `None`.
pub type SaneAuthCallback = Option<unsafe extern "C" fn(*const c_char, *mut c_char, *mut c_char)>;

// `extern "C"` declares functions that live in a C library and use the C calling convention.
// `#[link(name = "sane")]` makes the linker pull in `libsane.so`. The block is marked `unsafe`
// because we are promising these signatures match the C header; Rust cannot verify that.
#[link(name = "sane")]
unsafe extern "C" {
    pub fn sane_init(version_code: *mut SaneWord, authorize: SaneAuthCallback) -> i32;
    pub fn sane_exit();
    /// `device_list` receives a pointer to a NULL-terminated array of device pointers.
    pub fn sane_get_devices(device_list: *mut *mut *const SaneDevice, local_only: SaneWord) -> i32;
    pub fn sane_open(name: *const c_char, handle: *mut SaneHandle) -> i32;
    pub fn sane_close(handle: SaneHandle);
    pub fn sane_get_option_descriptor(
        handle: SaneHandle,
        option: SaneWord,
    ) -> *const SaneOptionDescriptor;
    pub fn sane_control_option(
        handle: SaneHandle,
        option: SaneWord,
        action: i32,
        value: *mut c_void,
        info: *mut SaneWord,
    ) -> i32;
    pub fn sane_strstatus(status: i32) -> *const c_char;
    pub fn sane_start(handle: SaneHandle) -> i32;
    pub fn sane_get_parameters(handle: SaneHandle, params: *mut SaneParameters) -> i32;
    /// Copies up to `max_len` bytes into `data` and stores how many in `len`.
    pub fn sane_read(
        handle: SaneHandle,
        data: *mut u8,
        max_len: SaneWord,
        len: *mut SaneWord,
    ) -> i32;
    pub fn sane_cancel(handle: SaneHandle);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    // If these sizes differ from C's, every field after the mismatch would be read from the
    // wrong place. Values are for x86_64 Linux (8-byte pointers, 4-byte int).
    #[test]
    fn struct_sizes_match_c() {
        assert_eq!(size_of::<SaneDevice>(), 32);
        assert_eq!(size_of::<SaneRange>(), 12);
        assert_eq!(size_of::<SaneOptionDescriptor>(), 56);
        assert_eq!(size_of::<SaneParameters>(), 24);
    }
}
