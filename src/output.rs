//! Where scans are saved and how, so that no existing file is ever replaced and no incomplete
//! file ever looks complete (research D7, D8; constitution Principle I).
//!
//! Layout: `<root>/<roll>/<roll>-<NNN>.tif` plus `<roll>-<NNN>.json` for each scan, and
//! `<root>/previews/preview-<YYYYMMDD-HHMMSS>.tif` for previews.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use crate::error::ScanError;

/// Highest frame number; three digits keep file names sorting in film order.
pub const MAX_FRAME: u32 = 999;

/// The default output root: `$HOME/Pictures/primefilm`.
pub fn default_output_root() -> Result<PathBuf, ScanError> {
    match std::env::var_os("HOME") {
        Some(home) if !home.is_empty() => {
            Ok(PathBuf::from(home).join("Pictures").join("primefilm"))
        }
        _ => Err(ScanError::OutputError {
            path: "$HOME".to_string(),
            reason: "HOME is not set; give an output folder with --output".to_string(),
        }),
    }
}

/// Check a roll name: 1–64 characters from `A–Z a–z 0–9 - _ .`, not starting with `.`.
/// Returns the name, so clap can use this as a value parser (a bad name is a usage error).
pub fn validate_roll_name(name: &str) -> Result<String, String> {
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.');
    if name.is_empty() || name.len() > 64 {
        return Err("roll names are 1 to 64 characters long".to_string());
    }
    if name.starts_with('.') {
        return Err("roll names cannot start with '.'".to_string());
    }
    if !name.chars().all(allowed) {
        return Err("roll names may only contain letters, digits, '-', '_' and '.'".to_string());
    }
    Ok(name.to_string())
}

/// Today's local date, `YYYY-MM-DD`: the roll name when none is given.
pub fn default_roll_name() -> String {
    jiff::Zoned::now().strftime("%Y-%m-%d").to_string()
}

/// One roll's folder.
pub struct RollFolder {
    name: String,
    dir: PathBuf,
}

impl RollFolder {
    /// Use (and create if needed) `<root>/<name>/`. `name` must already be valid.
    pub fn open(root: &Path, name: &str) -> Result<RollFolder, ScanError> {
        let dir = root.join(name);
        fs::create_dir_all(&dir).map_err(|e| io_error(&dir, &e))?;
        Ok(RollFolder {
            name: name.to_string(),
            dir,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// One past the highest frame number among `<name>-NNN*.tif` files in the folder; 1 if
    /// there are none. Lets a second scan or a resumed roll continue the numbering.
    pub fn next_frame(&self) -> Result<u32, ScanError> {
        let entries = fs::read_dir(&self.dir).map_err(|e| io_error(&self.dir, &e))?;
        let prefix = format!("{}-", self.name);
        let mut highest = 0;
        for entry in entries {
            let entry = entry.map_err(|e| io_error(&self.dir, &e))?;
            let file_name = entry.file_name();
            // Names that aren't valid UTF-8 can't be ours; skip them.
            let Some(file_name) = file_name.to_str() else {
                continue;
            };
            if let Some(frame) = frame_number(file_name, &prefix) {
                highest = highest.max(frame);
            }
        }
        Ok(highest + 1)
    }

    /// The file stem for `frame`: `<name>-<NNN>`, or `<name>-<NNN>-2`, `-3`, … if taken.
    pub fn scan_stem(&self, frame: u32) -> String {
        free_stem(&self.dir, &format!("{}-{frame:03}", self.name))
    }
}

/// The frame number in `<prefix>NNN.tif` or `<prefix>NNN-<k>.tif`, if `name` is one of those.
fn frame_number(name: &str, prefix: &str) -> Option<u32> {
    let rest = name.strip_prefix(prefix)?.strip_suffix(".tif")?;
    let (digits, suffix) = rest.split_at_checked(3)?;
    if !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let suffix_ok = suffix.is_empty()
        || suffix
            .strip_prefix('-')
            .is_some_and(|k| !k.is_empty() && k.chars().all(|c| c.is_ascii_digit()));
    if !suffix_ok {
        return None;
    }
    digits.parse().ok()
}

/// The previews folder (created if needed) and a free stem `preview-<YYYYMMDD-HHMMSS>`.
pub fn preview_stem(root: &Path) -> Result<(PathBuf, String), ScanError> {
    let dir = root.join("previews");
    fs::create_dir_all(&dir).map_err(|e| io_error(&dir, &e))?;
    let base = format!("preview-{}", jiff::Zoned::now().strftime("%Y%m%d-%H%M%S"));
    let stem = free_stem(&dir, &base);
    Ok((dir, stem))
}

/// `base`, or `base-2`, `base-3`, … : the first stem with neither a `.tif` nor a `.json`.
fn free_stem(dir: &Path, base: &str) -> String {
    let taken = |stem: &str| {
        dir.join(format!("{stem}.tif")).exists() || dir.join(format!("{stem}.json")).exists()
    };
    if !taken(base) {
        return base.to_string();
    }
    let mut k = 2;
    loop {
        let stem = format!("{base}-{k}");
        if !taken(&stem) {
            return stem;
        }
        k += 1;
    }
}

/// `.partial` files left behind in `dir` by a run that was killed, for the `-v` log.
pub fn leftover_partials(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    // `flatten` skips entries that failed to read; `filter` keeps the partial files.
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with('.') && n.ends_with(".partial"))
        })
        .collect()
}

/// A file being written under a hidden temporary name, `.<final name>.partial`. It appears
/// under its real name only when [`publish`](PartialFile::publish) succeeds; if it is dropped
/// unpublished (an error, Ctrl-C), the temporary file is deleted.
pub struct PartialFile {
    partial: PathBuf,
    target: PathBuf,
    published: bool,
}

impl PartialFile {
    /// Create `.<final_name>.partial` in `dir` and return it with the open file to write.
    pub fn create(dir: &Path, final_name: &str) -> Result<(PartialFile, File), ScanError> {
        let partial = dir.join(format!(".{final_name}.partial"));
        let target = dir.join(final_name);
        // `create` (not `create_new`): a stale partial from a killed run may be replaced, it
        // was never a finished file.
        let file = File::create(&partial).map_err(|e| io_error(&partial, &e))?;
        Ok((
            PartialFile {
                partial,
                target,
                published: false,
            },
            file,
        ))
    }

    /// The name the file will have once published.
    pub fn target(&self) -> &Path {
        &self.target
    }

    /// Flush `file` to disk and give it its real name. Fails, leaving any existing file
    /// untouched, if the real name is already taken.
    pub fn publish(mut self, file: File) -> Result<PathBuf, ScanError> {
        // `sync_all` waits until the data is really on disk, so a power cut right after this
        // cannot leave a published file with missing data.
        file.sync_all().map_err(|e| io_error(&self.partial, &e))?;
        drop(file);
        // A hard link gives the same file a second name. Unlike `rename`, which silently
        // replaces whatever has the target name, `hard_link` refuses if the name exists, so an
        // existing scan can never be overwritten (FR-010), even by another program racing us.
        fs::hard_link(&self.partial, &self.target).map_err(|e| io_error(&self.target, &e))?;
        self.published = true;
        // The file now has two names; removing the temporary one leaves just the real one.
        // If that fails, a harmless hidden duplicate remains, so only report it.
        if let Err(e) = fs::remove_file(&self.partial) {
            eprintln!(
                "primefilm: could not remove {}: {e}",
                self.partial.display()
            );
        }
        Ok(self.target.clone())
    }
}

impl Drop for PartialFile {
    fn drop(&mut self) {
        if !self.published {
            // Best effort: the file may not exist any more, which is fine.
            let _ = fs::remove_file(&self.partial);
        }
    }
}

/// Publish a scan's settings record first, then its image, so an image never exists without
/// the record of how it was made. Returns (image path, settings path).
pub fn publish_scan(
    sidecar: (PartialFile, File),
    image: (PartialFile, File),
) -> Result<(PathBuf, PathBuf), ScanError> {
    let settings = sidecar.0.publish(sidecar.1)?;
    let tiff = image.0.publish(image.1)?;
    Ok((tiff, settings))
}

fn io_error(path: &Path, err: &io::Error) -> ScanError {
    ScanError::OutputError {
        path: path.display().to_string(),
        reason: err.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A fresh, empty directory under the system temp dir for one test.
    fn temp_dir(label: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "primefilm-output-test-{}-{label}-{n}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path, contents: &str) {
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn roll_names() {
        assert!(validate_roll_name("2026-11-09").is_ok());
        assert!(validate_roll_name("Kodak_Gold.200").is_ok());
        assert!(validate_roll_name("").is_err());
        assert!(validate_roll_name(".hidden").is_err());
        assert!(validate_roll_name("a/b").is_err());
        assert!(validate_roll_name("with space").is_err());
        assert!(validate_roll_name(&"x".repeat(65)).is_err());
        assert!(validate_roll_name(&"x".repeat(64)).is_ok());
    }

    #[test]
    fn default_roll_name_is_a_date() {
        let name = default_roll_name();
        assert_eq!(name.len(), 10);
        assert!(validate_roll_name(&name).is_ok());
    }

    #[test]
    fn next_frame_rules() {
        let root = temp_dir("next");
        let roll = RollFolder::open(&root, "r").unwrap();
        assert_eq!(roll.next_frame().unwrap(), 1);
        touch(&roll.dir().join("r-001.tif"), "");
        touch(&roll.dir().join("r-004-2.tif"), "");
        // Not counted: other rolls, sidecars, partials, malformed names.
        touch(&roll.dir().join("rx-009.tif"), "");
        touch(&roll.dir().join("r-008.json"), "");
        touch(&roll.dir().join(".r-007.tif.partial"), "");
        touch(&roll.dir().join("r-12.tif"), "");
        touch(&roll.dir().join("r-006-x.tif"), "");
        assert_eq!(roll.next_frame().unwrap(), 5);
    }

    #[test]
    fn taken_names_get_a_suffix() {
        let root = temp_dir("suffix");
        let roll = RollFolder::open(&root, "r").unwrap();
        assert_eq!(roll.scan_stem(3), "r-003");
        touch(&roll.dir().join("r-003.tif"), "");
        assert_eq!(roll.scan_stem(3), "r-003-2");
        touch(&roll.dir().join("r-003-2.json"), "");
        assert_eq!(roll.scan_stem(3), "r-003-3");
    }

    #[test]
    fn publish_never_replaces_an_existing_file() {
        let dir = temp_dir("publish");
        touch(&dir.join("a.tif"), "original");
        let (partial, mut file) = PartialFile::create(&dir, "a.tif").unwrap();
        file.write_all(b"new").unwrap();
        assert!(partial.publish(file).is_err());
        assert_eq!(fs::read_to_string(dir.join("a.tif")).unwrap(), "original");
        assert!(
            leftover_partials(&dir).is_empty(),
            "failed partial was removed"
        );
    }

    #[test]
    fn publish_moves_partial_to_real_name() {
        let dir = temp_dir("ok");
        let (partial, mut file) = PartialFile::create(&dir, "b.tif").unwrap();
        file.write_all(b"data").unwrap();
        assert!(dir.join(".b.tif.partial").exists());
        assert!(!dir.join("b.tif").exists());
        let path = partial.publish(file).unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), "data");
        assert!(leftover_partials(&dir).is_empty());
    }

    #[test]
    fn dropped_partial_leaves_nothing() {
        let dir = temp_dir("drop");
        {
            let (_partial, mut file) = PartialFile::create(&dir, "c.tif").unwrap();
            file.write_all(b"half").unwrap();
        }
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
    }

    #[test]
    fn scan_publishes_sidecar_and_image() {
        let dir = temp_dir("scan");
        let sidecar = PartialFile::create(&dir, "r-001.json").unwrap();
        let image = PartialFile::create(&dir, "r-001.tif").unwrap();
        let (tiff, json) = publish_scan(sidecar, image).unwrap();
        assert!(tiff.ends_with("r-001.tif") && tiff.exists());
        assert!(json.ends_with("r-001.json") && json.exists());
    }

    #[test]
    fn previews_have_their_own_folder() {
        let root = temp_dir("preview");
        let (dir, stem) = preview_stem(&root).unwrap();
        assert_eq!(dir, root.join("previews"));
        assert!(stem.starts_with("preview-") && stem.len() == "preview-YYYYMMDD-HHMMSS".len());
    }
}
