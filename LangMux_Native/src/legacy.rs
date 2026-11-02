use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LegacyInstall {
    pub display_version: String,
    pub uninstaller: PathBuf,
}

pub fn parse_uninstall_entry(
    reg_query_output: &str,
    current_install_directory: &str,
) -> Option<LegacyInstall> {
    let mut display_version = None;
    let mut uninstall_string = None;

    for line in reg_query_output.lines() {
        let parts = line.split_whitespace().collect::<Vec<_>>();
        if parts.len() < 3 {
            continue;
        }
        let value = line.split_once(parts[1]).map(|(_, value)| value.trim())?;
        match parts[0].to_ascii_lowercase().as_str() {
            "displayversion" => display_version = Some(value.to_owned()),
            "uninstallstring" => uninstall_string = Some(value.to_owned()),
            _ => {}
        }
    }

    let display_version = display_version?;
    if !display_version.starts_with("1.") {
        return None;
    }
    let uninstall_string = uninstall_string?;
    let uninstaller = uninstaller_path(&uninstall_string)?;
    let filename = uninstaller.rsplit(['\\', '/']).next()?;
    if !filename.eq_ignore_ascii_case("uninstall.exe") {
        return None;
    }
    let parent = uninstaller
        .rsplit_once(['\\', '/'])
        .map_or("", |(parent, _)| parent);
    if same_or_nested_windows_path(parent, current_install_directory) {
        return None;
    }

    Some(LegacyInstall {
        display_version,
        uninstaller: PathBuf::from(uninstaller),
    })
}

fn uninstaller_path(command_line: &str) -> Option<&str> {
    let command_line = command_line.trim();
    if let Some(quoted) = command_line.strip_prefix('"') {
        return quoted.split_once('"').map(|(path, _)| path);
    }
    if let Some(end) = command_line
        .to_ascii_lowercase()
        .find("uninstall.exe")
        .map(|index| index + "uninstall.exe".len())
    {
        return Some(&command_line[..end]);
    }
    command_line.split_whitespace().next()
}

fn same_or_nested_windows_path(path: &str, root: &str) -> bool {
    let normalize = |path: &str| {
        let mut normalized = path.trim().trim_matches('"').replace('/', "\\");
        while normalized.ends_with('\\') && normalized.len() > 3 {
            normalized.pop();
        }
        normalized.to_ascii_lowercase()
    };
    let path = normalize(path);
    let root = normalize(root);
    !root.is_empty() && (path == root || path.starts_with(&format!("{root}\\")))
}

#[cfg(windows)]
pub fn find_legacy_install() -> Option<LegacyInstall> {
    let own_directory = std::env::current_exe()
        .ok()?
        .parent()?
        .to_string_lossy()
        .into_owned();
    let mut candidate = None;
    for key in [
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\LangMux",
        r"HKLM\Software\Microsoft\Windows\CurrentVersion\Uninstall\LangMux",
    ] {
        let output = crate::tools::command(std::path::Path::new("reg"))
            .args(["query", key])
            .output();
        if let Ok(output) = output
            && output.status.success()
            && let Some(install) =
                parse_uninstall_entry(&String::from_utf8_lossy(&output.stdout), &own_directory)
            && candidate.is_none()
        {
            candidate = Some(install);
        }
    }
    candidate
}

#[cfg(not(windows))]
pub fn find_legacy_install() -> Option<LegacyInstall> {
    None
}

#[cfg(windows)]
pub fn remove_legacy_install(install: &LegacyInstall) -> Result<(), String> {
    let status = crate::tools::command(&install.uninstaller)
        .arg("/S")
        .status()
        .map_err(|error| format!("Could not start the older installer uninstaller: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("Older installer uninstaller exited with {status}"))
    }
}

#[cfg(not(windows))]
pub fn remove_legacy_install(_install: &LegacyInstall) -> Result<(), String> {
    Err("Older installer cleanup is available on Windows only.".to_owned())
}

#[cfg(debug_assertions)]
pub fn debug_sample() -> LegacyInstall {
    LegacyInstall {
        display_version: "1.0.0".to_owned(),
        uninstaller: PathBuf::from(r"C:\Users\Example\AppData\Local\LangMux\uninstall.exe"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REG_QUERY_OUTPUT: &str = r#"
HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Uninstall\LangMux
    DisplayVersion    REG_SZ    1.0.0
    UninstallString   REG_SZ    "C:\Users\Example\AppData\Local\LangMux\uninstall.exe" /S
"#;

    #[test]
    fn parses_captured_v1_registry_output() {
        let install = parse_uninstall_entry(REG_QUERY_OUTPUT, r"C:\Program Files\LangMux").unwrap();
        assert_eq!(install.display_version, "1.0.0");
        assert_eq!(
            install.uninstaller,
            PathBuf::from(r"C:\Users\Example\AppData\Local\LangMux\uninstall.exe")
        );
    }

    #[test]
    fn ignores_non_v1_and_installers_inside_current_directory() {
        let v2 = REG_QUERY_OUTPUT.replace("1.0.0", "2.0.0");
        assert!(parse_uninstall_entry(&v2, r"C:\Program Files\LangMux").is_none());
        let current_install = REG_QUERY_OUTPUT.replace(
            r"C:\Users\Example\AppData\Local\LangMux",
            r"C:\Program Files\LangMux\uninstall",
        );
        assert!(parse_uninstall_entry(&current_install, r"C:\Program Files\LangMux").is_none());
    }

    #[test]
    fn rejects_other_uninstall_commands() {
        let invalid = REG_QUERY_OUTPUT.replace("uninstall.exe", "remove.cmd");
        assert!(parse_uninstall_entry(&invalid, r"C:\Program Files\LangMux").is_none());
    }
}
