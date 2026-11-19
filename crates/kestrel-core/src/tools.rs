use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU8, Ordering};

/// Scheduling priority of every tool process Kestrel starts (FFmpeg, FFprobe, MKVToolNix).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Priority {
    #[default]
    Normal,
    BelowNormal,
    Low,
}

impl Priority {
    /// Maps the settings list index (Normal, Below normal, Low); unknown values mean Normal.
    pub fn from_index(index: usize) -> Self {
        match index {
            1 => Self::BelowNormal,
            2 => Self::Low,
            _ => Self::Normal,
        }
    }

    /// Windows priority class creation flag. Normal is named explicitly: with no class flag a
    /// child inherits a below-normal or idle class from its parent.
    pub fn windows_class_flag(self) -> u32 {
        match self {
            Self::Normal => 0x0000_0020,
            Self::BelowNormal => 0x0000_4000,
            Self::Low => 0x0000_0040,
        }
    }

    /// `nice` increment on Unix; `None` keeps the normal niceness.
    pub fn nice_increment(self) -> Option<u8> {
        match self {
            Self::Normal => None,
            Self::BelowNormal => Some(5),
            Self::Low => Some(15),
        }
    }

    fn to_raw(self) -> u8 {
        match self {
            Self::Normal => 0,
            Self::BelowNormal => 1,
            Self::Low => 2,
        }
    }
}

static PRIORITY: AtomicU8 = AtomicU8::new(0);

/// Sets the priority [`command`] gives every process it builds from now on.
pub fn set_priority(priority: Priority) {
    PRIORITY.store(priority.to_raw(), Ordering::SeqCst);
}

pub fn priority() -> Priority {
    Priority::from_index(usize::from(PRIORITY.load(Ordering::SeqCst)))
}

#[derive(Clone, Debug)]
pub struct Tool {
    pub path: PathBuf,
    pub version: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Toolchain {
    pub mkvmerge: Option<Tool>,
    pub ffmpeg: Option<Tool>,
    pub ffprobe: Option<Tool>,
}

impl Toolchain {
    pub fn discover() -> Self {
        Self {
            mkvmerge: discover_tool("mkvmerge"),
            ffmpeg: discover_tool("ffmpeg"),
            ffprobe: discover_tool("ffprobe"),
        }
    }
}

fn discover_tool(name: &str) -> Option<Tool> {
    let path = find_executable(name)?;
    let version = read_version(&path, name);
    Some(Tool { path, version })
}

fn find_executable(name: &str) -> Option<PathBuf> {
    let executable = executable_name(name);
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        && let Some(candidate) = find_bundled(&exe_dir, &executable)
    {
        return Some(candidate);
    }

    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            let candidate = directory.join(&executable);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    find_platform_default(name, &executable)
}

fn find_bundled(exe_dir: &Path, executable: &OsString) -> Option<PathBuf> {
    let adjacent = exe_dir.join(executable);
    if adjacent.is_file() {
        return Some(adjacent);
    }

    let tools_dir = exe_dir.join("tools");
    let direct = tools_dir.join(executable);
    if direct.is_file() {
        return Some(direct);
    }

    let mut first_level = std::fs::read_dir(&tools_dir)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .collect::<Vec<_>>();
    first_level.sort_by_key(|entry| entry.file_name());

    for directory in first_level {
        let candidate = directory.path().join(executable);
        if candidate.is_file() {
            return Some(candidate);
        }

        let mut second_level = std::fs::read_dir(directory.path())
            .ok()?
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .collect::<Vec<_>>();
        second_level.sort_by_key(|entry| entry.file_name());
        for nested in second_level {
            let candidate = nested.path().join(executable);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(windows)]
fn executable_name(name: &str) -> OsString {
    OsString::from(format!("{name}.exe"))
}

#[cfg(not(windows))]
fn executable_name(name: &str) -> OsString {
    OsString::from(name)
}

#[cfg(windows)]
fn find_platform_default(name: &str, executable: &OsString) -> Option<PathBuf> {
    if name == "mkvmerge" {
        let candidate = Path::new(r"C:\Program Files\MKVToolNix").join(executable);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn find_platform_default(name: &str, executable: &OsString) -> Option<PathBuf> {
    if name == "mkvmerge"
        && let Ok(applications) = std::fs::read_dir("/Applications")
    {
        let mut bundles = applications
            .filter_map(Result::ok)
            .filter(|entry| {
                let filename = entry.file_name().to_string_lossy().to_string();
                filename.starts_with("MKVToolNix-") && filename.ends_with(".app")
            })
            .collect::<Vec<_>>();
        bundles.sort_by_key(|entry| entry.file_name());
        for bundle in bundles {
            let candidate = bundle
                .path()
                .join("Contents")
                .join("MacOS")
                .join(executable);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    find_in_directories(
        executable,
        [Path::new("/opt/homebrew/bin"), Path::new("/usr/local/bin")],
    )
}

#[cfg(target_os = "linux")]
fn find_platform_default(_name: &str, executable: &OsString) -> Option<PathBuf> {
    find_in_directories(
        executable,
        [Path::new("/usr/bin"), Path::new("/usr/local/bin")],
    )
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn find_platform_default(_name: &str, _executable: &OsString) -> Option<PathBuf> {
    None
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn find_in_directories<'a>(
    executable: &OsString,
    directories: impl IntoIterator<Item = &'a Path>,
) -> Option<PathBuf> {
    directories
        .into_iter()
        .map(|directory| directory.join(executable))
        .find(|candidate| candidate.is_file())
}

fn read_version(path: &Path, name: &str) -> Option<String> {
    let argument = if name == "mkvmerge" {
        "--version"
    } else {
        "-version"
    };
    let output = command(path).arg(argument).output().ok()?;
    let text = if output.stdout.is_empty() {
        String::from_utf8_lossy(&output.stderr)
    } else {
        String::from_utf8_lossy(&output.stdout)
    };
    text.lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
}

/// A command for a tool process: no console window on Windows, at the configured [`priority`].
pub fn command(executable: &Path) -> Command {
    command_with_priority(executable, priority())
}

/// Windows applies the priority class at process creation; Unix starts the tool through `nice`.
pub fn command_with_priority(executable: &Path, priority: Priority) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut command = Command::new(executable);
        command.creation_flags(0x08000000 | priority.windows_class_flag());
        command
    }
    #[cfg(not(windows))]
    {
        match priority.nice_increment() {
            Some(increment) => {
                let mut command = Command::new("nice");
                command.arg("-n").arg(increment.to_string()).arg(executable);
                command
            }
            None => Command::new(executable),
        }
    }
}

pub fn display_command(executable: &Path, arguments: &[OsString]) -> String {
    display_command_for(executable, arguments, cfg!(windows))
}

fn display_command_for(executable: &Path, arguments: &[OsString], windows: bool) -> String {
    let quote = |value: &str| {
        if !value.is_empty()
            && value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "_-./:=".contains(c))
        {
            value.to_owned()
        } else if windows {
            format!("'{}'", value.replace('\'', "''"))
        } else {
            format!("'{}'", value.replace('\'', "'\\''"))
        }
    };
    let parts = std::iter::once(executable.as_os_str())
        .chain(arguments.iter().map(|a| a.as_os_str()))
        .map(|s| quote(&s.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" ");
    if windows { format!("& {parts}") } else { parts }
}

#[cfg(test)]
mod priority_tests {
    use super::*;

    #[test]
    fn settings_index_maps_to_priority_and_flags() {
        assert_eq!(Priority::from_index(0), Priority::Normal);
        assert_eq!(Priority::from_index(1), Priority::BelowNormal);
        assert_eq!(Priority::from_index(2), Priority::Low);
        assert_eq!(Priority::from_index(9), Priority::Normal);
        assert_eq!(Priority::Normal.windows_class_flag(), 0x20);
        assert_eq!(Priority::BelowNormal.windows_class_flag(), 0x4000);
        assert_eq!(Priority::Low.windows_class_flag(), 0x40);
        assert_eq!(Priority::Normal.nice_increment(), None);
        assert_eq!(Priority::Low.nice_increment(), Some(15));
    }

    /// Asks the started process itself which priority class it runs in.
    #[cfg(windows)]
    #[test]
    fn spawned_process_runs_in_the_requested_priority_class() {
        for (priority, expected) in [
            (Priority::Normal, "Normal"),
            (Priority::BelowNormal, "BelowNormal"),
            (Priority::Low, "Idle"),
        ] {
            let output = command_with_priority(Path::new("powershell.exe"), priority)
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    "(Get-Process -Id $PID).PriorityClass",
                ])
                .output()
                .expect("powershell runs");
            assert_eq!(
                String::from_utf8_lossy(&output.stdout).trim(),
                expected,
                "{priority:?}"
            );
        }
    }

    /// Asks the started shell for its own niceness (`ps` reads it the same way on Linux and
    /// macOS; BSD `nice` with no command prints nothing).
    #[cfg(unix)]
    #[test]
    fn spawned_process_runs_at_the_requested_niceness() {
        for (priority, expected) in [(Priority::BelowNormal, "5"), (Priority::Low, "15")] {
            let mut command = command_with_priority(Path::new("sh"), priority);
            command.args(["-c", "ps -o ni= -p $$"]);
            let output = command.output().expect("sh runs");
            assert_eq!(
                String::from_utf8_lossy(&output.stdout).trim(),
                expected,
                "{priority:?}"
            );
        }
    }
}

#[cfg(test)]
mod command_display_tests {
    use super::*;
    #[test]
    fn quotes_spaces_quotes_and_empty_arguments_for_each_shell() {
        let args = vec![
            OsString::from("a b"),
            OsString::from("it's \"quoted\""),
            OsString::from(""),
        ];
        assert_eq!(
            display_command_for(Path::new("/tool path/ffmpeg"), &args, false),
            "'/tool path/ffmpeg' 'a b' 'it'\\''s \"quoted\"' ''"
        );
        assert_eq!(
            display_command_for(Path::new(r"C:\Program Files\ffmpeg.exe"), &args, true),
            "& 'C:\\Program Files\\ffmpeg.exe' 'a b' 'it''s \"quoted\"' ''"
        );
        assert_eq!(
            display_command_for(Path::new("ffmpeg"), &[], false),
            "ffmpeg"
        );
    }
}
