//! Write a private SANE configuration so only the backend we want is loaded (research D3).
//!
//! With every SANE backend loaded, another driver can claim the scanner's USB interface before
//! pieusb gets to it ("Device busy"), and webcams show up as scanners. Pointing
//! `SANE_CONFIG_DIR` at a directory whose `dll.conf` names a single backend avoids both.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::ScanError;

// `include_str!` copies a file's contents into the program at compile time, so the binary
// works without the repository next to it. The path is relative to this source file.
const PIEUSB_CONF: &str = include_str!("../../../sane.d/pieusb.conf");
const TEST_CONF: &str = include_str!("../../../sane.d/test.conf");

/// Write `dll.conf` (and the backend's own config, if we ship one) and point SANE at it.
///
/// Must be called before the Ctrl-C handler is installed, or any other thread is started,
/// because it changes environment variables (see the `SAFETY` note below). With
/// `driver_debug`, also turns on pieusb's own debug log on stderr (`-vv`).
pub fn prepare(backend: &str, driver_debug: bool) -> Result<PathBuf, ScanError> {
    let dir = match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(runtime) if !runtime.is_empty() => {
            PathBuf::from(runtime).join("primefilm").join("sane.d")
        }
        _ => std::env::temp_dir().join("primefilm-sane.d"),
    };
    fs::create_dir_all(&dir).map_err(|e| io_error(&dir, e))?;

    write_atomically(&dir.join("dll.conf"), &format!("{backend}\n"))?;
    let backend_conf = match backend {
        "pieusb" => Some(PIEUSB_CONF),
        "test" => Some(TEST_CONF),
        _ => None,
    };
    if let Some(contents) = backend_conf {
        write_atomically(&dir.join(format!("{backend}.conf")), contents)?;
    }

    // SAFETY: `set_var` is unsafe because another thread reading the environment at the same
    // moment would be a data race. This runs from `main` before the Ctrl-C handler (the only
    // other thread this program creates) is installed and before SANE is loaded, so no other
    // thread exists yet. No trailing colon: SANE then ignores `/etc/sane.d` entirely.
    unsafe {
        std::env::set_var("SANE_CONFIG_DIR", &dir);
        if driver_debug {
            std::env::set_var("SANE_DEBUG_PIEUSB", "5");
        }
    }
    Ok(dir)
}

/// Write to a temporary file and rename it into place, so a second `primefilm` starting at the
/// same moment (e.g. parallel tests) never reads a half-written file.
fn write_atomically(path: &Path, contents: &str) -> Result<(), ScanError> {
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(&tmp, contents).map_err(|e| io_error(&tmp, e))?;
    fs::rename(&tmp, path).map_err(|e| io_error(path, e))
}

fn io_error(path: &Path, err: std::io::Error) -> ScanError {
    ScanError::Sane {
        status: -1,
        text: format!("cannot write SANE config {}: {err}", path.display()),
    }
}
