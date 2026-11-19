//! Preset management: unique names and ids, rename, duplicate, delete, import and export.
//!
//! Built-in presets are read-only. Everything here works on the preset list and returns what
//! the caller should tell the user; the root view owns the dialogs and file pickers.

use kestrel_core::presets::{self, Preset};

/// Id prefix of user presets; the number after it never repeats within a settings file.
pub const USER_PREFIX: &str = "user-";

fn next_id(counter: &mut u32) -> String {
    *counter += 1;
    format!("{USER_PREFIX}{counter}")
}

fn name_taken(presets: &[Preset], name: &str, except_id: Option<&str>) -> bool {
    presets.iter().any(|preset| {
        Some(preset.id.as_str()) != except_id
            && preset.name.trim().eq_ignore_ascii_case(name.trim())
    })
}

/// `wanted` if no preset has that name, else `wanted 2`, `wanted 3`, ... Names stay unique
/// because the Transcode page finds the loaded preset by name.
pub fn unique_name(presets: &[Preset], wanted: &str) -> String {
    let wanted = wanted.trim();
    if !name_taken(presets, wanted, None) {
        return wanted.to_owned();
    }
    (2..)
        .map(|number| format!("{wanted} {number}"))
        .find(|candidate| !name_taken(presets, candidate, None))
        .expect("an unused numbered name exists")
}

fn user_preset<'a>(presets: &'a [Preset], id: &str) -> Result<&'a Preset, String> {
    let preset = presets
        .iter()
        .find(|preset| preset.id == id)
        .ok_or_else(|| "That preset no longer exists".to_owned())?;
    if preset.builtin {
        return Err(format!("{} is built in and cannot be changed", preset.name));
    }
    Ok(preset)
}

/// Renames a user preset and returns the old and new name.
pub fn rename(presets: &mut [Preset], id: &str, name: &str) -> Result<(String, String), String> {
    let old = user_preset(presets, id)?.name.clone();
    let name = name.trim();
    if name.is_empty() {
        return Err("Enter a preset name".to_owned());
    }
    if name_taken(presets, name, Some(id)) {
        return Err(format!("A preset named {name} already exists"));
    }
    presets::rename_user_preset(presets, id, name)?;
    Ok((old, name.to_owned()))
}

/// Adds a user copy of any preset (built-ins included) and returns its index.
pub fn duplicate(presets: &mut Vec<Preset>, id: &str, counter: &mut u32) -> Result<usize, String> {
    let source = presets
        .iter()
        .find(|preset| preset.id == id)
        .ok_or_else(|| "That preset no longer exists".to_owned())?
        .clone();
    let name = unique_name(presets, &format!("{} copy", source.name));
    presets.push(Preset {
        id: next_id(counter),
        name,
        builtin: false,
        ..source
    });
    Ok(presets.len() - 1)
}

/// Removes a user preset and returns its name.
pub fn delete(presets: &mut Vec<Preset>, id: &str) -> Result<String, String> {
    let name = user_preset(presets, id)?.name.clone();
    if !presets::delete_user_preset(presets, id) {
        return Err("That preset no longer exists".to_owned());
    }
    Ok(name)
}

/// What an import changed.
#[derive(Debug, Eq, PartialEq)]
pub struct Imported {
    pub added: usize,
    /// Presets whose name was already taken and got a number.
    pub renamed: usize,
}

/// Adds the user presets of an exported file. Ids are always new, and names that clash with an
/// existing preset get a number, so an import never replaces anything.
pub fn import(
    presets: &mut Vec<Preset>,
    json: &str,
    counter: &mut u32,
) -> Result<Imported, String> {
    let incoming = presets::import_user_presets(json)?;
    if incoming.is_empty() {
        return Err("The file holds no presets".to_owned());
    }
    let mut report = Imported {
        added: 0,
        renamed: 0,
    };
    for mut preset in incoming {
        let name = unique_name(presets, &preset.name);
        if name != preset.name.trim() {
            report.renamed += 1;
        }
        preset.name = name;
        preset.id = next_id(counter);
        preset.builtin = false;
        presets.push(preset);
        report.added += 1;
    }
    Ok(report)
}

/// JSON for one user preset.
pub fn export_one(presets: &[Preset], id: &str) -> Result<String, String> {
    let preset = user_preset(presets, id)?;
    presets::export_user_presets(std::slice::from_ref(preset))
}

/// JSON for every user preset.
pub fn export_all(presets: &[Preset]) -> Result<String, String> {
    if presets.iter().all(|preset| preset.builtin) {
        return Err("There are no saved presets to export".to_owned());
    }
    presets::export_user_presets(presets)
}

/// File name suggested for an export: the preset name made safe for a path.
pub fn export_file_name(name: Option<&str>) -> String {
    let stem: String = name
        .unwrap_or("kestrel-presets")
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || matches!(ch, '-' | '_' | ' ') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    format!("{}.json", stem.trim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kestrel_core::encoders::EncoderSet;
    use kestrel_core::formats::Container;

    fn list() -> (Vec<Preset>, u32) {
        let mut presets = presets::builtins(&EncoderSet::default());
        let mut settings = presets[1].settings.clone();
        settings.container = Container::Mp4;
        presets.push(Preset {
            id: "user-1".to_owned(),
            name: "Phone".to_owned(),
            description: "mine".to_owned(),
            builtin: false,
            settings,
        });
        (presets, 1)
    }

    fn names(presets: &[Preset]) -> Vec<&str> {
        presets.iter().map(|preset| preset.name.as_str()).collect()
    }

    #[test]
    fn rename_changes_a_user_preset_and_checks_the_name() {
        let (mut presets, _) = list();
        let renamed = rename(&mut presets, "user-1", "  Tablet ").unwrap();
        assert_eq!(renamed, ("Phone".to_owned(), "Tablet".to_owned()));
        assert_eq!(presets.last().unwrap().name, "Tablet");
        assert!(
            rename(&mut presets, "user-1", "")
                .unwrap_err()
                .contains("Enter")
        );
        // Another preset's name, in any letter case, is taken; its own name is not.
        assert!(
            rename(&mut presets, "user-1", "change tracks only")
                .unwrap_err()
                .contains("already exists")
        );
        assert!(rename(&mut presets, "user-1", "tablet").is_ok());
        assert!(
            rename(&mut presets, "gone", "X")
                .unwrap_err()
                .contains("no longer")
        );
    }

    #[test]
    fn built_in_presets_are_read_only() {
        let (mut presets, _) = list();
        let builtin = presets[0].id.clone();
        assert!(
            rename(&mut presets, &builtin, "Mine")
                .unwrap_err()
                .contains("built in")
        );
        assert!(
            delete(&mut presets, &builtin)
                .unwrap_err()
                .contains("built in")
        );
        assert!(
            export_one(&presets, &builtin)
                .unwrap_err()
                .contains("built in")
        );
        assert_eq!(presets.len(), 6);
    }

    #[test]
    fn duplicate_copies_settings_into_a_new_user_preset() {
        let (mut presets, mut counter) = list();
        let phone = duplicate(&mut presets, "user-1", &mut counter).unwrap();
        assert_eq!(presets[phone].name, "Phone copy");
        assert_eq!(presets[phone].id, "user-2");
        assert!(!presets[phone].builtin);
        assert_eq!(presets[phone].settings, presets[5].settings);
        // A built-in can be copied; the copy is editable, and the names stay unique.
        let first = presets[0].id.clone();
        let copy = duplicate(&mut presets, &first, &mut counter).unwrap();
        assert_eq!(presets[copy].name, "Change tracks only copy");
        assert!(!presets[copy].builtin);
        let again = duplicate(&mut presets, "user-1", &mut counter).unwrap();
        assert_eq!(presets[again].name, "Phone copy 2");
        assert_eq!(counter, 4);
        assert!(presets.iter().filter(|p| p.builtin).count() == 5);
    }

    #[test]
    fn delete_removes_only_the_chosen_user_preset() {
        let (mut presets, mut counter) = list();
        duplicate(&mut presets, "user-1", &mut counter).unwrap();
        assert_eq!(delete(&mut presets, "user-1").unwrap(), "Phone");
        assert_eq!(names(&presets).last(), Some(&"Phone copy"));
        assert!(
            delete(&mut presets, "user-1")
                .unwrap_err()
                .contains("no longer")
        );
        assert_eq!(presets.len(), 6);
    }

    #[test]
    fn export_then_import_round_trips_without_replacing_anything() {
        let (mut presets, mut counter) = list();
        let json = export_one(&presets, "user-1").unwrap();
        assert!(json.contains("\"Phone\""));
        assert!(!json.contains("Change tracks only"));
        // Importing into the same list keeps the original and adds a renamed copy.
        let report = import(&mut presets, &json, &mut counter).unwrap();
        assert_eq!(
            report,
            Imported {
                added: 1,
                renamed: 1
            }
        );
        assert_eq!(presets.len(), 7);
        assert_eq!(presets[6].name, "Phone 2");
        assert_eq!(presets[6].id, "user-2");
        assert_eq!(presets[6].settings, presets[5].settings);
        assert!(!presets[6].builtin);
        // Into a fresh list the name and settings arrive unchanged.
        let mut fresh = presets::builtins(&EncoderSet::default());
        let mut fresh_counter = 7;
        let report = import(&mut fresh, &json, &mut fresh_counter).unwrap();
        assert_eq!(
            report,
            Imported {
                added: 1,
                renamed: 0
            }
        );
        assert_eq!(fresh.last().unwrap().name, "Phone");
        assert_eq!(fresh.last().unwrap().id, "user-8");
    }

    #[test]
    fn import_rejects_bad_files_and_resets_the_builtin_flag() {
        let (mut presets, mut counter) = list();
        assert!(
            import(&mut presets, "not json", &mut counter)
                .unwrap_err()
                .contains("Invalid preset file")
        );
        assert!(
            import(&mut presets, r#"{"version":9,"presets":[]}"#, &mut counter)
                .unwrap_err()
                .contains("Unsupported")
        );
        assert!(
            import(&mut presets, r#"{"version":1,"presets":[]}"#, &mut counter)
                .unwrap_err()
                .contains("no presets")
        );
        // A file cannot smuggle in a built-in preset or reuse an id.
        let mut hostile = presets[0].clone();
        hostile.builtin = true;
        let json = serde_json::json!({"version": 1, "presets": [hostile]}).to_string();
        let report = import(&mut presets, &json, &mut counter).unwrap();
        assert_eq!(report.added, 1);
        let added = presets.last().unwrap();
        assert!(!added.builtin);
        assert!(added.id.starts_with(USER_PREFIX));
        assert_eq!(added.name, "Change tracks only 2");
        assert_eq!(presets.iter().filter(|p| p.builtin).count(), 5);
    }

    #[test]
    fn export_all_holds_every_user_preset_and_nothing_built_in() {
        let (mut presets, mut counter) = list();
        duplicate(&mut presets, "user-1", &mut counter).unwrap();
        let json = export_all(&presets).unwrap();
        let back = presets::import_user_presets(&json).unwrap();
        assert_eq!(back.len(), 2);
        assert!(back.iter().all(|preset| !preset.builtin));
        let only_builtin = presets::builtins(&EncoderSet::default());
        assert!(
            export_all(&only_builtin)
                .unwrap_err()
                .contains("no saved presets")
        );
    }

    #[test]
    fn export_file_names_are_safe_for_paths() {
        assert_eq!(
            export_file_name(Some("Phone / 720p: v2")),
            "Phone _ 720p_ v2.json"
        );
        assert_eq!(export_file_name(None), "kestrel-presets.json");
    }
}
