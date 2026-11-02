use std::io;
use std::path::{Path, PathBuf};

pub const APP_NAME: &str = "LangMux";
pub const APP_ID: &str = "langmux";
pub const LEGACY_APP_IDS: &[&str] = &[];

pub fn migrate_storage() {
    let Some(current_dir) = eframe::storage_dir(APP_ID) else {
        return;
    };
    let legacy_dirs = LEGACY_APP_IDS
        .iter()
        .filter_map(|app_id| eframe::storage_dir(app_id))
        .collect::<Vec<_>>();
    let _ = migrate_storage_from_dirs(&current_dir, &legacy_dirs);
}

pub fn migrate_storage_from_dirs(current_dir: &Path, legacy_dirs: &[PathBuf]) -> io::Result<bool> {
    let current_file = current_dir.join("app.ron");
    if current_file.exists() {
        return Ok(false);
    }

    let Some(legacy_file) = legacy_dirs
        .iter()
        .map(|directory| directory.join("app.ron"))
        .find(|path| path.is_file())
    else {
        return Ok(false);
    };

    std::fs::create_dir_all(current_dir)?;
    std::fs::copy(legacy_file, current_file)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

    fn temp_dir() -> PathBuf {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("langmux-branding-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn migrates_the_first_available_legacy_storage_file() {
        let root = temp_dir();
        let current = root.join("current");
        let missing = root.join("missing");
        let legacy = root.join("legacy");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("app.ron"), b"saved state").unwrap();

        assert!(migrate_storage_from_dirs(&current, &[missing, legacy.clone()]).unwrap());
        assert_eq!(
            std::fs::read(current.join("app.ron")).unwrap(),
            b"saved state"
        );
        assert!(!migrate_storage_from_dirs(&current, &[legacy]).unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn leaves_current_storage_untouched() {
        let root = temp_dir();
        let current = root.join("current");
        let legacy = root.join("legacy");
        std::fs::create_dir_all(&current).unwrap();
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(current.join("app.ron"), b"current").unwrap();
        std::fs::write(legacy.join("app.ron"), b"legacy").unwrap();

        assert!(!migrate_storage_from_dirs(&current, &[legacy]).unwrap());
        assert_eq!(std::fs::read(current.join("app.ron")).unwrap(), b"current");
        std::fs::remove_dir_all(root).unwrap();
    }
}
