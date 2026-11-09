//! Look at the scanner through Linux's `/sys` view of USB (research D4).
//!
//! SANE alone cannot tell "switched off" from "in use by another program": in both cases the
//! scanner is simply missing from its device list. `/sys/bus/usb/devices` tells them apart
//! without touching the scanner. Nothing here claims or configures the device.

use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

/// The PrimeFilm XA's USB vendor and product IDs (Genesys Logic `05e3:0144`).
pub const VENDOR_ID: &str = "05e3";
pub const PRODUCT_ID: &str = "0144";

/// Reads USB device information from a `/sys`-style directory.
pub struct UsbProbe {
    sysfs_root: PathBuf,
}

/// A USB device found under `/sys`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsbDevice {
    /// Directory name under the sysfs root, e.g. `1-2`.
    pub sys_name: String,
    pub bus: u32,
    pub device: u32,
}

impl UsbProbe {
    /// The real system view.
    pub fn system() -> UsbProbe {
        UsbProbe::at("/sys/bus/usb/devices")
    }

    /// A probe rooted somewhere else, so tests can build a fake tree.
    pub fn at(path: impl Into<PathBuf>) -> UsbProbe {
        UsbProbe {
            sysfs_root: path.into(),
        }
    }

    /// Find the first device with these IDs. Entries that can't be read are skipped.
    pub fn find(&self, vendor: &str, product: &str) -> Option<UsbDevice> {
        let entries = fs::read_dir(&self.sysfs_root).ok()?;
        // `.flatten()` skips directory entries that could not be read.
        for entry in entries.flatten() {
            let dir = entry.path();
            if read_trimmed(&dir, "idVendor").as_deref() != Some(vendor)
                || read_trimmed(&dir, "idProduct").as_deref() != Some(product)
            {
                continue;
            }
            let bus = read_trimmed(&dir, "busnum").and_then(|s| s.parse().ok());
            let device = read_trimmed(&dir, "devnum").and_then(|s| s.parse().ok());
            if let (Some(bus), Some(device)) = (bus, device) {
                return Some(UsbDevice {
                    sys_name: entry.file_name().to_string_lossy().into_owned(),
                    bus,
                    device,
                });
            }
        }
        None
    }

    /// True when a program has claimed the scanner's interface through `usbfs` (that is what
    /// libusb, and so SANE and VueScan, use). The kernel shows this as the interface's
    /// `driver` link pointing at `usbfs`.
    pub fn is_claimed(&self, dev: &UsbDevice) -> bool {
        let link = self
            .sysfs_root
            .join(format!("{}:1.0", dev.sys_name))
            .join("driver");
        match fs::read_link(link) {
            Ok(target) => target.file_name().is_some_and(|name| name == "usbfs"),
            Err(_) => false,
        }
    }
}

impl UsbDevice {
    /// The device node libusb opens, e.g. `/dev/bus/usb/001/004`.
    pub fn node_path(&self) -> PathBuf {
        PathBuf::from(format!("/dev/bus/usb/{:03}/{:03}", self.bus, self.device))
    }

    /// Check we may open the device node read-write. Opening the node does not claim the
    /// interface, so this is harmless even if another program is using the scanner. The file
    /// is closed again immediately when it goes out of scope.
    pub fn check_node_access(&self) -> std::io::Result<()> {
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(self.node_path())
            .map(|_file| ())
    }
}

fn read_trimmed(dir: &Path, file: &str) -> Option<String> {
    fs::read_to_string(dir.join(file))
        .ok()
        .map(|s| s.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway fake sysfs tree, deleted when the test ends.
    struct FakeSysfs {
        root: PathBuf,
    }

    impl FakeSysfs {
        fn new(name: &str) -> FakeSysfs {
            let root = std::env::temp_dir()
                .join(format!("primefilm-usb-test-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            FakeSysfs { root }
        }

        fn add_device(&self, sys_name: &str, vendor: &str, product: &str) {
            let dir = self.root.join(sys_name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("idVendor"), format!("{vendor}\n")).unwrap();
            fs::write(dir.join("idProduct"), format!("{product}\n")).unwrap();
            fs::write(dir.join("busnum"), "001\n").unwrap();
            fs::write(dir.join("devnum"), "4\n").unwrap();
        }
    }

    impl Drop for FakeSysfs {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn no_matching_device() {
        let sys = FakeSysfs::new("none");
        sys.add_device("1-1", "046d", "c52b");
        assert_eq!(UsbProbe::at(&sys.root).find(VENDOR_ID, PRODUCT_ID), None);
    }

    #[test]
    fn finds_scanner() {
        let sys = FakeSysfs::new("found");
        sys.add_device("1-1", "046d", "c52b");
        sys.add_device("1-2", "05e3", "0144");
        let dev = UsbProbe::at(&sys.root).find(VENDOR_ID, PRODUCT_ID).unwrap();
        assert_eq!(dev.sys_name, "1-2");
        assert_eq!((dev.bus, dev.device), (1, 4));
        assert_eq!(dev.node_path(), PathBuf::from("/dev/bus/usb/001/004"));
    }

    #[test]
    fn claimed_when_driver_is_usbfs() {
        let sys = FakeSysfs::new("claimed");
        sys.add_device("1-2", "05e3", "0144");
        let interface = sys.root.join("1-2:1.0");
        fs::create_dir_all(&interface).unwrap();
        std::os::unix::fs::symlink("../../../bus/usb/drivers/usbfs", interface.join("driver"))
            .unwrap();
        let probe = UsbProbe::at(&sys.root);
        let dev = probe.find(VENDOR_ID, PRODUCT_ID).unwrap();
        assert!(probe.is_claimed(&dev));
    }

    #[test]
    fn not_claimed_without_driver_link() {
        let sys = FakeSysfs::new("unclaimed");
        sys.add_device("1-2", "05e3", "0144");
        fs::create_dir_all(sys.root.join("1-2:1.0")).unwrap();
        let probe = UsbProbe::at(&sys.root);
        let dev = probe.find(VENDOR_ID, PRODUCT_ID).unwrap();
        assert!(!probe.is_claimed(&dev));
    }
}
