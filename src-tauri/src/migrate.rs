//! One-time move of the data saved by anySCP into Talktoall Files.
//!
//! The rename changed the bundle identifier, so Tauri now resolves a new app
//! data folder. On the first launch, when that folder has no database yet and
//! the old `com.macnev2013.anyscp` folder (next to it) has one, everything in
//! the old folder is copied over. The old folder is left untouched, so anySCP
//! keeps working if it is still installed.

use std::path::{Path, PathBuf};

/// Bundle identifier used before the rename.
pub const LEGACY_IDENTIFIER: &str = "com.macnev2013.anyscp";
const DB_FILE: &str = "anyscp.db";

/// Settings from the old look that would hide the new one (anySCP's blue
/// accent, its font, a light theme). They are dropped after an import so the
/// Talktoall Files defaults show; the user can set them again.
pub const RESET_APPEARANCE_KEYS: [&str; 4] = [
    "app_theme",
    "app_accent_hue",
    "app_accent_custom",
    "app_interface_font",
];

fn legacy_dir(new_dir: &Path) -> Option<PathBuf> {
    Some(new_dir.parent()?.join(LEGACY_IDENTIFIER))
}

/// Copy the anySCP data into `new_dir` when this is the first launch.
/// Returns true when something was imported.
pub fn import_from_anyscp(new_dir: &Path) -> std::io::Result<bool> {
    if new_dir.join(DB_FILE).exists() {
        return Ok(false);
    }
    let Some(old) = legacy_dir(new_dir) else {
        return Ok(false);
    };
    if !old.join(DB_FILE).is_file() {
        return Ok(false);
    }
    copy_dir(&old, new_dir)?;
    tracing::info!(from = %old.display(), to = %new_dir.display(), "imported anySCP data");
    Ok(true)
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let src = entry.path();
        let dst = to.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_dir(&src, &dst)?;
        } else if kind.is_file() && !dst.exists() {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> PathBuf {
        let p = std::env::temp_dir().join(format!("ttf-migrate-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn copies_old_folder_once() {
        let root = temp_root();
        let old = root.join(LEGACY_IDENTIFIER);
        std::fs::create_dir_all(old.join("sub")).unwrap();
        std::fs::write(old.join(DB_FILE), b"db").unwrap();
        std::fs::write(old.join("anyscp.db-wal"), b"wal").unwrap();
        std::fs::write(old.join("sub").join("known_hosts"), b"k").unwrap();
        let new = root.join("br.com.talktoall.files");

        assert!(import_from_anyscp(&new).unwrap());
        assert_eq!(std::fs::read(new.join(DB_FILE)).unwrap(), b"db");
        assert_eq!(std::fs::read(new.join("anyscp.db-wal")).unwrap(), b"wal");
        assert_eq!(std::fs::read(new.join("sub").join("known_hosts")).unwrap(), b"k");

        // Second launch: the new database exists, nothing is copied again.
        std::fs::write(new.join(DB_FILE), b"newer").unwrap();
        assert!(!import_from_anyscp(&new).unwrap());
        assert_eq!(std::fs::read(new.join(DB_FILE)).unwrap(), b"newer");
        assert!(old.join(DB_FILE).exists(), "old data stays in place");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn nothing_to_import_without_old_database() {
        let root = temp_root();
        let new = root.join("br.com.talktoall.files");
        assert!(!import_from_anyscp(&new).unwrap());
        assert!(!new.join(DB_FILE).exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
