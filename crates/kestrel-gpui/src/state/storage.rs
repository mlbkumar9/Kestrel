use kestrel_core::{
    branding,
    settings::{self, Preferences, Storage},
};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct SettingsPaths {
    pub gpui: PathBuf,
    pub egui: PathBuf,
    pub legacy_egui: Vec<PathBuf>,
}
impl SettingsPaths {
    pub fn discover() -> io::Result<Self> {
        let config = dirs::config_dir()
            .ok_or_else(|| io::Error::other("Configuration directory is unavailable"))?;
        let data =
            dirs::data_dir().ok_or_else(|| io::Error::other("Data directory is unavailable"))?;
        Ok(Self {
            gpui: config.join(branding::APP_ID).join("gpui.ron"),
            egui: egui_file(&data, branding::APP_ID),
            legacy_egui: branding::LEGACY_APP_IDS
                .iter()
                .map(|id| egui_file(&data, id))
                .collect(),
        })
    }
}
fn egui_file(data: &Path, id: &str) -> PathBuf {
    let directory = data.join(id);
    #[cfg(windows)]
    let directory = directory.join("data");
    directory.join("app.ron")
}

pub struct FileStorage {
    path: PathBuf,
    values: BTreeMap<String, String>,
}
impl Storage for FileStorage {
    fn get_string(&self, key: &str) -> Option<String> {
        self.values.get(key).cloned()
    }
    fn set_string(&mut self, key: &str, value: String) {
        self.values.insert(key.into(), value);
    }
}
impl FileStorage {
    pub fn open(paths: &SettingsPaths) -> io::Result<(Self, Preferences)> {
        let existing = read_map(&paths.gpui)?;
        let first_run = existing.is_none();
        let values = if let Some(values) = existing {
            values
        } else {
            let mut imported = None;
            for path in std::iter::once(&paths.egui).chain(&paths.legacy_egui) {
                if let Some(values) = read_map(path)? {
                    imported = Some(values);
                    break;
                }
            }
            imported.unwrap_or_default()
        };
        let mut storage = Self {
            path: paths.gpui.clone(),
            values,
        };
        let (mut preferences, legacy) = settings::load_preferences(&storage);
        settings::migrate_preferences(&mut preferences, legacy);
        if first_run {
            storage.values.clear();
            storage.save(&preferences)?;
        }
        Ok((storage, preferences))
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn save(&mut self, preferences: &Preferences) -> io::Result<()> {
        settings::save_preferences(self, preferences);
        self.flush()
    }
    pub fn flush(&self) -> io::Result<()> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| io::Error::other("Settings path has no parent"))?;
        fs::create_dir_all(parent)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        let text = ron::ser::to_string(&self.values).map_err(io::Error::other)?;
        temporary.write_all(text.as_bytes())?;
        temporary.as_file().sync_all()?;
        temporary.persist(&self.path).map_err(|e| e.error)?;
        Ok(())
    }
}
fn read_map(path: &Path) -> io::Result<Option<BTreeMap<String, String>>> {
    match fs::read_to_string(path) {
        Ok(text) => ron::from_str(&text).map(Some).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}: {e}", path.display()),
            )
        }),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn paths(root: &Path) -> SettingsPaths {
        SettingsPaths {
            gpui: root.join("config/gpui.ron"),
            egui: root.join("egui/app.ron"),
            legacy_egui: vec![root.join("langmux/app.ron")],
        }
    }
    #[test]
    fn atomic_round_trip_and_replacement() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        let (mut storage, mut prefs) = FileStorage::open(&paths).unwrap();
        prefs.audio_languages = "fra, jpn".into();
        prefs.dry_run = true;
        storage.save(&prefs).unwrap();
        prefs.subtitle_languages = "deu".into();
        storage.save(&prefs).unwrap();
        let (_, loaded) = FileStorage::open(&paths).unwrap();
        assert_eq!(loaded.audio_languages, "fra, jpn");
        assert!(loaded.dry_run);
        assert_eq!(loaded.current_settings.subtitles.languages, vec!["deu"]);
        assert_eq!(
            fs::read_dir(paths.gpui.parent().unwrap()).unwrap().count(),
            1
        );
    }
    #[test]
    fn imports_egui_fixture_only_on_first_run() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        fs::create_dir_all(paths.egui.parent().unwrap()).unwrap();
        fs::write(&paths.egui, include_str!("fixtures/egui.ron")).unwrap();
        let original = fs::read(&paths.egui).unwrap();
        let (_, prefs) = FileStorage::open(&paths).unwrap();
        assert_eq!(prefs.audio_languages, "fra");
        assert_eq!(prefs.subtitle_languages, "deu");
        assert!(prefs.dry_run);
        assert_eq!(fs::read(&paths.egui).unwrap(), original);
        fs::write(&paths.egui, "invalid").unwrap();
        assert_eq!(FileStorage::open(&paths).unwrap().1.audio_languages, "fra");
    }
    #[test]
    fn missing_egui_uses_defaults_and_legacy_location_is_supported() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        fs::create_dir_all(paths.legacy_egui[0].parent().unwrap()).unwrap();
        fs::write(&paths.legacy_egui[0], include_str!("fixtures/egui.ron")).unwrap();
        assert_eq!(FileStorage::open(&paths).unwrap().1.audio_languages, "fra");
        let fresh = tempfile::tempdir().unwrap();
        assert_eq!(
            FileStorage::open(&super::tests::paths(fresh.path()))
                .unwrap()
                .1
                .audio_languages,
            Preferences::default().audio_languages
        );
    }
    #[test]
    fn corrupt_files_and_failed_writes_are_reported_without_overwrite() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        fs::create_dir_all(paths.gpui.parent().unwrap()).unwrap();
        fs::write(&paths.gpui, "invalid").unwrap();
        assert!(FileStorage::open(&paths).is_err());
        assert_eq!(fs::read_to_string(&paths.gpui).unwrap(), "invalid");
        fs::remove_file(&paths.gpui).unwrap();
        fs::create_dir(&paths.gpui).unwrap();
        assert!(FileStorage::open(&paths).is_err());
        let mut storage = FileStorage {
            path: paths.gpui,
            values: BTreeMap::new(),
        };
        assert!(storage.save(&Preferences::default()).is_err());
    }
}
