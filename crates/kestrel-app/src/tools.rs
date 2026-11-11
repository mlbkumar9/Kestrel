use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

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

pub fn command(executable: &Path) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut command = Command::new(executable);
        command.creation_flags(0x08000000);
        command
    }
    #[cfg(not(windows))]
    {
        Command::new(executable)
    }
}
