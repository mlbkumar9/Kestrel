//! Persisted preferences and source-selection migration.
use crate::inputs::{self, ExistingPolicy, InputItem, OutputTarget};
use crate::mkv;
use crate::presets::Preset;
use crate::transcode::JobSettings;
use crate::worker::RemuxEngine;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// String storage used by the preference loader and saver. Values retain the legacy RON format.
pub trait Storage {
    fn get_string(&self, key: &str) -> Option<String>;
    fn set_string(&mut self, key: &str, value: String);
}

fn get_value<T: serde::de::DeserializeOwned>(storage: &dyn Storage, key: &str) -> Option<T> {
    ron::from_str(&storage.get_string(key)?).ok()
}

fn set_value<T: Serialize>(storage: &mut dyn Storage, key: &str, value: &T) {
    if let Ok(value) = ron::ser::to_string(value) {
        storage.set_string(key, value);
    }
}

/// Saves preferences without resurrecting cleared source selections.
pub fn save_preferences(storage: &mut dyn Storage, preferences: &Preferences) {
    set_value(
        storage,
        crate::branding::PREFERENCES_KEY,
        &preferences_for_save(preferences),
    );
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
/// Persisted settings, including fields retained for legacy-record migration.
pub struct Preferences {
    pub light_theme: bool,
    pub input_items: Vec<InputItem>,
    #[serde(default)]
    pub removed_sources: Vec<PathBuf>,
    pub output_directory: String,
    pub audio_languages: String,
    pub subtitle_languages: String,
    #[serde(default, alias = "engine")]
    pub remux_engine: RemuxEngine,
    pub dry_run: bool,
    pub logs_open: bool,
    pub selected_preset_id: String,
    pub current_settings: JobSettings,
    pub user_presets: Vec<Preset>,
    pub existing: ExistingPolicy,
    pub include_subfolders: bool,
    pub last_input_dir: String,
    pub output_target: Option<OutputTarget>,
    pub input_directory: String,
    pub skip_existing: bool,
    pub legacy_banner_dismissed: bool,
    #[serde(default)]
    pub output_target_explicit: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            light_theme: false,
            input_items: Vec::new(),
            removed_sources: Vec::new(),
            output_directory: String::new(),
            audio_languages: "eng, jpn".to_owned(),
            subtitle_languages: "eng".to_owned(),
            remux_engine: RemuxEngine::MkvToolNix,
            dry_run: false,
            logs_open: false,
            selected_preset_id: "remux".to_owned(),
            current_settings: JobSettings::default(),
            user_presets: Vec::new(),
            existing: ExistingPolicy::Skip,
            include_subfolders: true,
            last_input_dir: String::new(),
            output_target: None,
            input_directory: String::new(),
            skip_existing: true,
            legacy_banner_dismissed: false,
            output_target_explicit: false,
        }
    }
}

/// Upgrades legacy source/output fields and synchronizes language selection settings.
pub fn migrate_preferences(preferences: &mut Preferences, from_legacy_record: bool) {
    if preferences.input_items.is_empty() && !preferences.input_directory.is_empty() {
        preferences.input_items.push(InputItem::Folder {
            path: PathBuf::from(&preferences.input_directory),
            recursive: if from_legacy_record {
                preferences.include_subfolders
            } else {
                true
            },
        });
    }
    if from_legacy_record {
        preferences.output_target_explicit = !preferences.output_directory.is_empty()
            && Path::new(&preferences.output_directory)
                != mkv::default_output_dir(Path::new(&preferences.input_directory));
        preferences.existing = if preferences.skip_existing {
            ExistingPolicy::Skip
        } else {
            ExistingPolicy::Overwrite
        };
        if preferences.output_target.is_none() && !preferences.output_directory.is_empty() {
            preferences.output_target = Some(OutputTarget::Folder(PathBuf::from(
                &preferences.output_directory,
            )));
        }
        preferences.input_directory.clear();
        preferences.skip_existing = true;
    }
    preferences.current_settings.audio.languages =
        mkv::parse_language_field(&preferences.audio_languages);
    preferences.current_settings.subtitles.languages =
        mkv::parse_language_field(&preferences.subtitle_languages);
}

pub fn clear_source_preferences(preferences: &mut Preferences) {
    preferences.input_items.clear();
    preferences.removed_sources.clear();
    preferences.input_directory.clear();
}

pub fn remove_sources(preferences: &mut Preferences, paths: &[PathBuf]) {
    let mut processed = Vec::new();
    for path in paths {
        let resolved = inputs::resolved_path(path);
        if processed.contains(&resolved) {
            continue;
        }
        processed.push(resolved.clone());
        let is_explicit_file = preferences.input_items.iter().any(
            |item| matches!(item, InputItem::File(file) if inputs::resolved_path(file) == resolved),
        );
        if is_explicit_file {
            preferences.input_items.retain(
                |item| !matches!(item, InputItem::File(file) if inputs::resolved_path(file) == resolved),
            );
        } else if !preferences
            .removed_sources
            .iter()
            .any(|removed| inputs::resolved_path(removed) == resolved)
        {
            preferences.removed_sources.push(resolved);
        }
    }
    if preferences.input_items.is_empty() {
        preferences.input_directory.clear();
        preferences.removed_sources.clear();
    }
}

pub fn add_input_item(preferences: &mut Preferences, item: InputItem) {
    match &item {
        InputItem::File(path) => {
            let resolved = inputs::resolved_path(path);
            preferences
                .removed_sources
                .retain(|removed| inputs::resolved_path(removed) != resolved);
        }
        InputItem::Folder { path, recursive } => {
            let folder = inputs::resolved_path(path);
            preferences.removed_sources.retain(|removed| {
                let removed = inputs::resolved_path(removed);
                let covered = if *recursive {
                    removed.starts_with(&folder) && removed != folder
                } else {
                    removed.parent() == Some(folder.as_path())
                };
                !covered
            });
        }
    }
    if !preferences.input_items.contains(&item) {
        preferences.input_items.push(item);
    }
}

pub fn preferences_for_save(preferences: &Preferences) -> Preferences {
    let mut preferences = preferences.clone();
    if preferences.input_items.is_empty() {
        preferences.input_directory.clear();
        preferences.removed_sources.clear();
    }
    preferences
}

/// Loads the first valid current or legacy RON record and returns whether v2 migration is needed.
/// Missing or invalid records fall back to defaults. Call [migrate_preferences] afterwards.
pub fn load_preferences(storage: &dyn Storage) -> (Preferences, bool) {
    if let Some(preferences) = get_value(storage, crate::branding::PREFERENCES_KEY) {
        return (preferences, false);
    }
    crate::branding::LEGACY_PREFERENCES_KEYS
        .iter()
        .find_map(|key| {
            get_value(storage, key).map(|preferences| {
                (
                    preferences,
                    *key == crate::branding::LEGACY_V2_PREFERENCES_KEY,
                )
            })
        })
        .unwrap_or_else(|| (Preferences::default(), false))
}

pub fn effective_output_target(
    stored_target: Option<&OutputTarget>,
    target_is_explicit: bool,
    selected_paths: &[PathBuf],
    input_items: &[InputItem],
    container: crate::formats::Container,
) -> OutputTarget {
    if target_is_explicit && let Some(target) = stored_target {
        return match (target, selected_paths.len()) {
            (OutputTarget::File(path), 1) => OutputTarget::File(path.clone()),
            (OutputTarget::File(path), _) => OutputTarget::Folder(
                path.parent()
                    .unwrap_or_else(|| Path::new("."))
                    .to_path_buf(),
            ),
            (OutputTarget::Folder(path), _) => OutputTarget::Folder(path.clone()),
        };
    }
    if let [path] = selected_paths {
        return inputs::default_output_target(&[InputItem::File(path.clone())], container);
    }
    inputs::default_output_target(input_items, container)
}

#[cfg(test)]
mod tests {
    use super::{
        Preferences, add_input_item, clear_source_preferences, effective_output_target,
        load_preferences, migrate_preferences, preferences_for_save, remove_sources,
    };
    use super::{Storage, set_value};
    use crate::formats::Container;
    use crate::inputs::{ExistingPolicy, InputItem, OutputTarget};
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let id = NEXT_TEMP.fetch_add(1, AtomicOrdering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "kestrel-removed-sources-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn touch(path: &std::path::Path) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, b"fixture").unwrap();
    }

    #[derive(Default)]
    struct MemoryStorage(HashMap<String, String>);

    #[test]
    fn missing_and_invalid_records_use_defaults() {
        let mut storage = MemoryStorage::default();
        let (preferences, legacy) = load_preferences(&storage);
        assert!(!legacy);
        assert_eq!(preferences.selected_preset_id, "remux");
        storage.set_string(crate::branding::PREFERENCES_KEY, "not valid RON".to_owned());
        let (preferences, legacy) = load_preferences(&storage);
        assert!(!legacy);
        assert_eq!(preferences.selected_preset_id, "remux");
    }

    #[test]
    fn invalid_current_record_falls_back_to_legacy() {
        let mut storage = MemoryStorage::default();
        storage.set_string(crate::branding::PREFERENCES_KEY, "invalid".to_owned());
        set_value(
            &mut storage,
            crate::branding::LEGACY_V2_PREFERENCES_KEY,
            &Preferences {
                input_directory: "/movies".to_owned(),
                ..Default::default()
            },
        );
        let (preferences, legacy) = load_preferences(&storage);
        assert!(legacy);
        assert_eq!(preferences.input_directory, "/movies");
    }

    impl Storage for MemoryStorage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }

        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.to_owned(), value);
        }
    }

    #[test]
    fn loads_preferences_from_a_legacy_langmux_key() {
        let mut storage = MemoryStorage::default();
        set_value(
            &mut storage,
            crate::branding::LEGACY_V2_PREFERENCES_KEY,
            &Preferences {
                input_directory: "/movies".to_owned(),
                output_directory: "/exports".to_owned(),
                skip_existing: false,
                ..Preferences::default()
            },
        );
        let (mut preferences, from_legacy) = load_preferences(&storage);
        assert!(from_legacy);
        migrate_preferences(&mut preferences, from_legacy);
        assert_eq!(
            preferences.input_items,
            [InputItem::Folder {
                path: PathBuf::from("/movies"),
                recursive: true,
            }]
        );
        assert_eq!(preferences.existing, ExistingPolicy::Overwrite);
        assert!(preferences.output_target_explicit);
        assert_eq!(
            preferences.output_target,
            Some(OutputTarget::Folder(PathBuf::from("/exports")))
        );
        assert_eq!(preferences.output_directory, "/exports");
    }

    #[test]
    fn deleting_explicit_and_folder_sources_persists_exact_exclusions() {
        let root = temp_dir();
        let folder = root.join("folder");
        let explicit_file = root.join("explicit.mkv");
        let removed_folder_file = folder.join("removed.mkv");
        let sibling = folder.join("sibling.mkv");
        touch(&explicit_file);
        touch(&removed_folder_file);
        touch(&sibling);

        let mut preferences = Preferences {
            input_items: vec![
                InputItem::File(explicit_file.clone()),
                InputItem::Folder {
                    path: folder.clone(),
                    recursive: true,
                },
            ],
            input_directory: root.to_string_lossy().into_owned(),
            ..Preferences::default()
        };
        remove_sources(
            &mut preferences,
            &[
                explicit_file.clone(),
                explicit_file,
                removed_folder_file.clone(),
                removed_folder_file.clone(),
            ],
        );

        assert_eq!(
            preferences.removed_sources,
            [crate::inputs::resolved_path(&removed_folder_file)]
        );

        let saved = preferences_for_save(&preferences);
        let mut storage = MemoryStorage::default();
        set_value(&mut storage, crate::branding::PREFERENCES_KEY, &saved);
        let (mut loaded, from_legacy) = load_preferences(&storage);
        assert!(!from_legacy);
        migrate_preferences(&mut loaded, from_legacy);
        let sources = crate::inputs::expand(&loaded.input_items, &[], &loaded.removed_sources);
        assert_eq!(
            sources
                .iter()
                .map(|source| source.path.file_name().unwrap().to_string_lossy())
                .collect::<Vec<_>>(),
            ["sibling.mkv"]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn readding_a_folder_restores_its_removed_files() {
        let root = temp_dir();
        let file = root.join("a.mkv");
        touch(&file);

        let mut preferences = Preferences {
            input_items: vec![InputItem::Folder {
                path: root.clone(),
                recursive: true,
            }],
            ..Preferences::default()
        };
        remove_sources(&mut preferences, std::slice::from_ref(&file));
        assert_eq!(
            preferences.removed_sources,
            [crate::inputs::resolved_path(&file)]
        );

        add_input_item(
            &mut preferences,
            InputItem::Folder {
                path: root.clone(),
                recursive: true,
            },
        );

        assert!(preferences.removed_sources.is_empty());
        let sources =
            crate::inputs::expand(&preferences.input_items, &[], &preferences.removed_sources);
        assert_eq!(
            sources
                .iter()
                .map(|source| source.path.file_name().unwrap().to_string_lossy())
                .collect::<Vec<_>>(),
            ["a.mkv"]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn adding_nonrecursive_parent_preserves_nested_source_removal() {
        let root = temp_dir();
        let folder = root.join("v");
        let shows = folder.join("shows");
        let removed_file = shows.join("e.mkv");
        touch(&removed_file);

        let mut preferences = Preferences {
            input_items: vec![InputItem::Folder {
                path: shows,
                recursive: false,
            }],
            ..Preferences::default()
        };
        remove_sources(&mut preferences, std::slice::from_ref(&removed_file));
        add_input_item(
            &mut preferences,
            InputItem::Folder {
                path: folder,
                recursive: false,
            },
        );

        assert_eq!(
            preferences.removed_sources,
            [crate::inputs::resolved_path(&removed_file)]
        );
        let sources =
            crate::inputs::expand(&preferences.input_items, &[], &preferences.removed_sources);
        assert!(!sources.iter().any(|source| source.path == removed_file));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn readding_nonrecursive_folder_restores_direct_child() {
        let root = temp_dir();
        let folder = root.join("v");
        let file = folder.join("a.mkv");
        touch(&file);

        let item = InputItem::Folder {
            path: folder,
            recursive: false,
        };
        let mut preferences = Preferences {
            input_items: vec![item.clone()],
            ..Preferences::default()
        };
        remove_sources(&mut preferences, std::slice::from_ref(&file));
        assert_eq!(
            preferences.removed_sources,
            [crate::inputs::resolved_path(&file)]
        );

        add_input_item(&mut preferences, item.clone());

        assert!(preferences.removed_sources.is_empty());
        assert_eq!(preferences.input_items, [item]);
        let sources =
            crate::inputs::expand(&preferences.input_items, &[], &preferences.removed_sources);
        assert!(sources.iter().any(|source| source.path == file));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn adding_recursive_parent_restores_nested_source_removal() {
        let root = temp_dir();
        let folder = root.join("v");
        let shows = folder.join("shows");
        let removed_file = shows.join("e.mkv");
        touch(&removed_file);

        let mut preferences = Preferences {
            input_items: vec![InputItem::Folder {
                path: shows,
                recursive: false,
            }],
            ..Preferences::default()
        };
        remove_sources(&mut preferences, std::slice::from_ref(&removed_file));
        add_input_item(
            &mut preferences,
            InputItem::Folder {
                path: folder,
                recursive: true,
            },
        );

        assert!(preferences.removed_sources.is_empty());
        let sources =
            crate::inputs::expand(&preferences.input_items, &[], &preferences.removed_sources);
        assert!(sources.iter().any(|source| source.path == removed_file));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn adding_a_different_folder_preserves_existing_removals() {
        let root = temp_dir();
        let movies = root.join("movies");
        let shows = root.join("shows");
        std::fs::create_dir_all(&movies).unwrap();
        std::fs::create_dir_all(&shows).unwrap();
        let removed_file = movies.join("removed.mkv");
        touch(&removed_file);

        let mut preferences = Preferences {
            input_items: vec![InputItem::Folder {
                path: movies,
                recursive: true,
            }],
            removed_sources: vec![crate::inputs::resolved_path(&removed_file)],
            ..Preferences::default()
        };
        add_input_item(
            &mut preferences,
            InputItem::Folder {
                path: shows.clone(),
                recursive: true,
            },
        );

        assert_eq!(
            preferences.removed_sources,
            [crate::inputs::resolved_path(&removed_file)]
        );
        assert!(preferences.input_items.contains(&InputItem::Folder {
            path: shows,
            recursive: true,
        }));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn adding_a_removed_file_explicitly_restores_it() {
        let root = temp_dir();
        let removed_file = root.join("removed.mkv");
        let sibling = root.join("sibling.mkv");
        touch(&removed_file);
        touch(&sibling);

        let mut preferences = Preferences {
            input_items: vec![InputItem::Folder {
                path: root.clone(),
                recursive: true,
            }],
            removed_sources: vec![crate::inputs::resolved_path(&removed_file)],
            ..Preferences::default()
        };
        add_input_item(&mut preferences, InputItem::File(removed_file.clone()));

        assert!(preferences.removed_sources.is_empty());
        let sources =
            crate::inputs::expand(&preferences.input_items, &[], &preferences.removed_sources);
        assert_eq!(
            sources
                .iter()
                .map(|source| source.path.file_name().unwrap().to_string_lossy())
                .collect::<Vec<_>>(),
            ["removed.mkv", "sibling.mkv"]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn clear_source_preferences_resets_removed_sources() {
        let mut preferences = Preferences {
            input_items: vec![InputItem::Folder {
                path: PathBuf::from("/movies"),
                recursive: true,
            }],
            removed_sources: vec![PathBuf::from("/movies/removed.mkv")],
            input_directory: "/movies".to_owned(),
            ..Preferences::default()
        };
        clear_source_preferences(&mut preferences);

        assert!(preferences.input_items.is_empty());
        assert!(preferences.removed_sources.is_empty());
        assert!(preferences.input_directory.is_empty());
    }

    #[test]
    fn saving_empty_sources_clears_a_stale_input_directory_before_reload() {
        let preferences = Preferences {
            input_directory: "/movies".to_owned(),
            ..Preferences::default()
        };
        let saved = preferences_for_save(&preferences);
        assert!(saved.input_directory.is_empty());
        assert!(saved.removed_sources.is_empty());

        let mut storage = MemoryStorage::default();
        set_value(&mut storage, crate::branding::PREFERENCES_KEY, &saved);
        let (mut loaded, from_legacy) = load_preferences(&storage);
        assert!(!from_legacy);
        migrate_preferences(&mut loaded, from_legacy);
        assert!(loaded.input_items.is_empty());
    }

    #[test]
    fn app_preferences_v3_fallback_is_not_migrated_as_a_v1_v2_record() {
        let mut storage = MemoryStorage::default();
        set_value(
            &mut storage,
            "app.preferences.v3",
            &Preferences {
                input_items: vec![InputItem::Folder {
                    path: PathBuf::from("/movies"),
                    recursive: true,
                }],
                input_directory: "/movies".to_owned(),
                output_directory: "/custom-output".to_owned(),
                output_target: Some(OutputTarget::Folder(PathBuf::from("/movies/converted"))),
                output_target_explicit: false,
                existing: ExistingPolicy::Rename,
                skip_existing: true,
                ..Preferences::default()
            },
        );

        let (mut preferences, from_legacy) = load_preferences(&storage);
        assert!(!from_legacy);
        migrate_preferences(&mut preferences, from_legacy);

        assert_eq!(preferences.existing, ExistingPolicy::Rename);
        assert!(preferences.skip_existing);
        assert_eq!(preferences.output_directory, "/custom-output");
        assert!(!preferences.output_target_explicit);
        assert_eq!(
            preferences.output_target,
            Some(OutputTarget::Folder(PathBuf::from("/movies/converted",)))
        );
        assert_eq!(
            effective_output_target(
                preferences.output_target.as_ref(),
                preferences.output_target_explicit,
                &[],
                &preferences.input_items,
                Container::Mkv,
            ),
            OutputTarget::Folder(PathBuf::from("/movies/converted"))
        );
    }

    #[test]
    fn early_v3_record_restores_input_directory_without_changing_other_preferences() {
        let mut storage = MemoryStorage::default();
        let key = "app.preferences.v3";
        set_value(
            &mut storage,
            key,
            &Preferences {
                input_directory: "/movies".to_owned(),
                output_directory: "/custom-output".to_owned(),
                output_target: Some(OutputTarget::Folder(PathBuf::from("/movies/converted"))),
                output_target_explicit: false,
                existing: ExistingPolicy::Rename,
                skip_existing: true,
                ..Preferences::default()
            },
        );
        let mut record = storage.get_string(key).unwrap();
        let input_items_start = record.find("input_items:").unwrap();
        let input_items_end =
            input_items_start + record[input_items_start..].find(',').unwrap() + 1;
        record.replace_range(input_items_start..input_items_end, "");
        storage.set_string(key, record);

        let (mut preferences, from_legacy) = load_preferences(&storage);
        assert!(!from_legacy);
        migrate_preferences(&mut preferences, from_legacy);

        assert_eq!(
            preferences.input_items,
            [InputItem::Folder {
                path: PathBuf::from("/movies"),
                recursive: true,
            }]
        );
        assert_eq!(preferences.existing, ExistingPolicy::Rename);
        assert!(preferences.skip_existing);
        assert_eq!(preferences.output_directory, "/custom-output");
        assert_eq!(
            preferences.output_target,
            Some(OutputTarget::Folder(PathBuf::from("/movies/converted",)))
        );
        assert!(!preferences.output_target_explicit);
        assert_eq!(
            effective_output_target(
                preferences.output_target.as_ref(),
                preferences.output_target_explicit,
                &[],
                &preferences.input_items,
                Container::Mkv,
            ),
            OutputTarget::Folder(PathBuf::from("/movies/converted"))
        );
    }

    #[test]
    fn v3_record_keeps_existing_input_items_when_input_directory_differs() {
        let mut storage = MemoryStorage::default();
        let input_items = [InputItem::Folder {
            path: PathBuf::from("/selected"),
            recursive: false,
        }];
        set_value(
            &mut storage,
            "app.preferences.v3",
            &Preferences {
                input_items: input_items.to_vec(),
                input_directory: "/different".to_owned(),
                output_target: Some(OutputTarget::Folder(PathBuf::from("/selected/converted"))),
                output_target_explicit: false,
                existing: ExistingPolicy::Rename,
                ..Preferences::default()
            },
        );

        let (mut preferences, from_legacy) = load_preferences(&storage);
        assert!(!from_legacy);
        migrate_preferences(&mut preferences, from_legacy);

        assert_eq!(preferences.input_items, input_items);
        assert_eq!(preferences.input_directory, "/different");
        assert_eq!(preferences.existing, ExistingPolicy::Rename);
        assert_eq!(
            preferences.output_target,
            Some(OutputTarget::Folder(PathBuf::from("/selected/converted",)))
        );
        assert!(!preferences.output_target_explicit);
    }

    #[test]
    fn legacy_fields_migrate_only_from_the_legacy_record() {
        let mut current = Preferences {
            input_directory: "/v".to_owned(),
            skip_existing: false,
            ..Preferences::default()
        };
        migrate_preferences(&mut current, false);
        assert_eq!(
            current.input_items,
            [InputItem::Folder {
                path: PathBuf::from("/v"),
                recursive: true
            }]
        );
        assert_eq!(current.existing, ExistingPolicy::Skip);
        assert_eq!(current.input_directory, "/v");

        migrate_preferences(&mut current, true);
        assert_eq!(
            current.input_items,
            [InputItem::Folder {
                path: PathBuf::from("/v"),
                recursive: true
            }]
        );
        assert_eq!(current.existing, ExistingPolicy::Overwrite);
        assert!(current.input_directory.is_empty());
        assert!(current.skip_existing);
    }

    #[test]
    fn legacy_custom_output_directory_remains_explicit_for_jobs() {
        let mut preferences = Preferences {
            input_directory: "/movies".to_owned(),
            output_directory: "/exports".to_owned(),
            ..Preferences::default()
        };
        migrate_preferences(&mut preferences, true);

        assert!(preferences.output_target_explicit);
        let target = effective_output_target(
            preferences.output_target.as_ref(),
            preferences.output_target_explicit,
            &[PathBuf::from("/movies/film.mkv")],
            &preferences.input_items,
            Container::Mkv,
        );
        assert_eq!(target, OutputTarget::Folder(PathBuf::from("/exports")));
    }

    #[test]
    fn legacy_default_output_directory_remains_inferred() {
        let mut preferences = Preferences {
            input_directory: "/movies".to_owned(),
            output_directory: "/movies/converted".to_owned(),
            ..Preferences::default()
        };
        migrate_preferences(&mut preferences, true);

        assert!(!preferences.output_target_explicit);
        let target = effective_output_target(
            preferences.output_target.as_ref(),
            preferences.output_target_explicit,
            &[PathBuf::from("/movies/film.mkv")],
            &preferences.input_items,
            Container::Mkv,
        );
        assert_eq!(
            target,
            OutputTarget::File(PathBuf::from("/movies/converted/film.mkv"))
        );
    }

    #[test]
    fn clearing_upgraded_sources_does_not_restore_them_after_restart() {
        let mut preferences = Preferences {
            input_directory: "/v".to_owned(),
            ..Preferences::default()
        };
        migrate_preferences(&mut preferences, true);
        clear_source_preferences(&mut preferences);
        migrate_preferences(&mut preferences, false);
        assert!(preferences.input_items.is_empty());
        assert!(preferences.input_directory.is_empty());
    }

    #[test]
    fn skip_choice_after_legacy_upgrade_survives_restart() {
        let mut preferences = Preferences {
            skip_existing: false,
            ..Preferences::default()
        };
        migrate_preferences(&mut preferences, true);
        assert_eq!(preferences.existing, ExistingPolicy::Overwrite);
        preferences.existing = ExistingPolicy::Skip;
        migrate_preferences(&mut preferences, false);
        assert_eq!(preferences.existing, ExistingPolicy::Skip);
    }

    #[test]
    fn inferred_single_file_target_tracks_the_current_selection() {
        let stored = OutputTarget::File(PathBuf::from("/v/converted/a.mkv"));
        let target = |path: &str| {
            effective_output_target(
                Some(&stored),
                false,
                &[PathBuf::from(path)],
                &[InputItem::File(PathBuf::from(path))],
                Container::Mkv,
            )
        };
        assert_eq!(
            target("/v/a.mkv"),
            OutputTarget::File(PathBuf::from("/v/converted/a.mkv"))
        );
        assert_eq!(
            target("/v/b.mkv"),
            OutputTarget::File(PathBuf::from("/v/converted/b.mkv"))
        );
    }
}
