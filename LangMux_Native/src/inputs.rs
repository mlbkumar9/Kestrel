use crate::formats::{self, Container};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InputItem {
    File(PathBuf),
    Folder { path: PathBuf, recursive: bool },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SourceFile {
    pub path: PathBuf,
    pub relative: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputTarget {
    File(PathBuf),
    Folder(PathBuf),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PlannedOutput {
    pub input: PathBuf,
    pub output: PathBuf,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExistingPolicy {
    #[default]
    Skip,
    Overwrite,
    Rename,
}

pub fn expand(items: &[InputItem], exclude: &[PathBuf]) -> Vec<SourceFile> {
    let excluded = exclude
        .iter()
        .map(|path| resolved_path(path))
        .collect::<Vec<_>>();
    let mut seen = HashSet::new();
    let mut sources = Vec::new();

    for item in items {
        match item {
            InputItem::File(path) => {
                if formats::is_supported_input(path) && !is_partial_path(path) {
                    push_source(
                        path.clone(),
                        file_relative(path),
                        &excluded,
                        &mut seen,
                        &mut sources,
                    );
                }
            }
            InputItem::Folder { path, recursive } => {
                expand_folder(path, path, *recursive, &excluded, &mut seen, &mut sources);
            }
        }
    }

    sources.sort_by(|left, right| natural_path_cmp(&left.relative, &right.relative));
    sources
}

fn expand_folder(
    root: &Path,
    directory: &Path,
    recursive: bool,
    excluded: &[PathBuf],
    seen: &mut HashSet<PathBuf>,
    sources: &mut Vec<SourceFile>,
) {
    if is_hidden_directory(directory) || is_excluded(directory, excluded) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            if recursive {
                expand_folder(root, &path, recursive, excluded, seen, sources);
            }
        } else if file_type.is_file()
            && formats::is_supported_input(&path)
            && !is_partial_path(&path)
        {
            let relative = path
                .strip_prefix(root)
                .map(Path::to_path_buf)
                .unwrap_or_else(|_| file_relative(&path));
            push_source(path, relative, excluded, seen, sources);
        }
    }
}

fn push_source(
    path: PathBuf,
    relative: PathBuf,
    excluded: &[PathBuf],
    seen: &mut HashSet<PathBuf>,
    sources: &mut Vec<SourceFile>,
) {
    if is_excluded(&path, excluded) {
        return;
    }
    let key = resolved_path(&path);
    if seen.insert(key) {
        sources.push(SourceFile { path, relative });
    }
}

fn is_excluded(path: &Path, excluded: &[PathBuf]) -> bool {
    let path = resolved_path(path);
    excluded
        .iter()
        .any(|candidate| path == *candidate || path.starts_with(candidate))
}

fn resolved_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| normalize_path(path))
}

fn normalize_path(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn is_hidden_directory(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with('.'))
}

fn is_partial_path(path: &Path) -> bool {
    path.file_name().is_some_and(|name| {
        name.to_string_lossy()
            .to_ascii_lowercase()
            .contains(".langmux-partial-")
    })
}

fn file_relative(path: &Path) -> PathBuf {
    path.file_name()
        .map(PathBuf::from)
        .unwrap_or_else(|| path.to_path_buf())
}

pub fn default_output_target(items: &[InputItem], container: Container) -> OutputTarget {
    if let [InputItem::File(path)] = items {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let stem = path.file_stem().unwrap_or_default();
        return OutputTarget::File(parent.join("converted").join(format!(
            "{}.{}",
            stem.to_string_lossy(),
            container.extension()
        )));
    }

    let folder_items = items
        .iter()
        .filter_map(|item| match item {
            InputItem::Folder { path, .. } => Some(path.clone()),
            InputItem::File(_) => None,
        })
        .collect::<Vec<_>>();
    let root = if let [folder] = folder_items.as_slice() {
        folder.clone()
    } else if let Some(common_parent) = common_parent(items) {
        common_parent
    } else {
        items
            .first()
            .map(item_path)
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."))
    };
    OutputTarget::Folder(root.join("converted"))
}

pub fn output_exclusions(target: &OutputTarget, items: &[InputItem]) -> Vec<PathBuf> {
    match target {
        OutputTarget::Folder(path) => {
            let folder = normalize_path(path);
            let nested_in_source_folder = items.iter().any(|item| match item {
                InputItem::Folder { path, .. } => {
                    let source = normalize_path(path);
                    folder != source && folder.starts_with(source)
                }
                InputItem::File(_) => false,
            });
            let contains_input_item = items.iter().any(|item| {
                let source = normalize_path(item_path(item));
                source == folder || source.starts_with(&folder)
            });
            if nested_in_source_folder && !contains_input_item {
                vec![path.clone()]
            } else {
                Vec::new()
            }
        }
        OutputTarget::File(path) => vec![path.clone()],
    }
}

fn item_path(item: &InputItem) -> &Path {
    match item {
        InputItem::File(path) | InputItem::Folder { path, .. } => path,
    }
}

fn common_parent(items: &[InputItem]) -> Option<PathBuf> {
    let parents = items
        .iter()
        .map(|item| match item {
            InputItem::File(path) => path.parent().unwrap_or_else(|| Path::new(".")),
            InputItem::Folder { path, .. } => path.parent().unwrap_or_else(|| Path::new(".")),
        })
        .collect::<Vec<_>>();
    let first = parents.first()?;
    let mut common = PathBuf::new();
    for component in first.components() {
        if parents
            .iter()
            .all(|parent| parent.components().nth(common.components().count()) == Some(component))
        {
            common.push(component.as_os_str());
        } else {
            break;
        }
    }
    Some(if common.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        common
    })
}

pub fn plan_outputs(
    sources: &[SourceFile],
    target: &OutputTarget,
    container: Container,
) -> Result<Vec<PlannedOutput>, String> {
    if let OutputTarget::File(_) = target
        && sources.len() != 1
    {
        return Err("A file output target requires exactly one input".to_owned());
    }

    let mut planned = Vec::with_capacity(sources.len());
    let mut used = HashSet::new();
    for source in sources {
        let candidate = match target {
            OutputTarget::File(path) => path.with_extension(container.extension()),
            OutputTarget::Folder(folder) => folder
                .join(&source.relative)
                .with_extension(container.extension()),
        };
        let output = unique_output(candidate, &mut used);
        planned.push(PlannedOutput {
            input: source.path.clone(),
            output,
        });
    }
    Ok(planned)
}

fn unique_output(candidate: PathBuf, used: &mut HashSet<PathBuf>) -> PathBuf {
    let key = collision_key(&candidate);
    if used.insert(key) {
        return candidate;
    }

    let parent = candidate.parent().unwrap_or_else(|| Path::new("."));
    let stem = candidate.file_stem().unwrap_or_default().to_string_lossy();
    let extension = candidate.extension().unwrap_or_default().to_string_lossy();
    let mut suffix = 2;
    loop {
        let name = if extension.is_empty() {
            format!("{stem} ({suffix})")
        } else {
            format!("{stem} ({suffix}).{extension}")
        };
        let output = parent.join(name);
        if used.insert(collision_key(&output)) {
            return output;
        }
        suffix += 1;
    }
}

fn collision_key(path: &Path) -> PathBuf {
    if cfg!(windows) || cfg!(target_os = "macos") {
        PathBuf::from(path.to_string_lossy().to_ascii_lowercase())
    } else {
        path.to_path_buf()
    }
}

fn natural_path_cmp(left: &Path, right: &Path) -> Ordering {
    let left = left.to_string_lossy();
    let right = right.to_string_lossy();
    natural_cmp(&left, &right).then_with(|| left.cmp(&right))
}

fn natural_cmp(left: &str, right: &str) -> Ordering {
    let mut left_chars = left.chars().peekable();
    let mut right_chars = right.chars().peekable();
    loop {
        match (left_chars.peek().copied(), right_chars.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(left_char), Some(right_char))
                if left_char.is_ascii_digit() && right_char.is_ascii_digit() =>
            {
                let left_digits = take_digit_run(&mut left_chars);
                let right_digits = take_digit_run(&mut right_chars);
                let left_significant = left_digits.trim_start_matches('0');
                let right_significant = right_digits.trim_start_matches('0');
                let ordering = left_significant
                    .len()
                    .cmp(&right_significant.len())
                    .then_with(|| left_significant.cmp(right_significant))
                    .then_with(|| left_digits.len().cmp(&right_digits.len()));
                if ordering != Ordering::Equal {
                    return ordering;
                }
            }
            (Some(left_char), Some(right_char)) => {
                let ordering = left_char
                    .to_ascii_lowercase()
                    .cmp(&right_char.to_ascii_lowercase());
                if ordering != Ordering::Equal {
                    return ordering;
                }
                left_chars.next();
                right_chars.next();
            }
        }
    }
}

fn take_digit_run(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut digits = String::new();
    while chars.peek().is_some_and(char::is_ascii_digit) {
        digits.push(chars.next().unwrap());
    }
    digits
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

    fn temp_dir() -> PathBuf {
        let id = NEXT_TEMP.fetch_add(1, AtomicOrdering::Relaxed);
        let path = std::env::temp_dir().join(format!("langmux-inputs-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, b"fixture").unwrap();
    }

    #[test]
    fn expands_supported_files_recursively_and_excludes_hidden_and_output_folders() {
        let root = temp_dir();
        touch(&root.join("a10.mkv"));
        touch(&root.join("a2.MP4"));
        touch(&root.join("nested").join("b1.avi"));
        touch(&root.join(".hidden").join("secret.mkv"));
        touch(&root.join("converted").join("previous.webm"));
        touch(&root.join("clip.langmux-partial-22-0.mkv"));

        let input = InputItem::Folder {
            path: root.clone(),
            recursive: true,
        };
        let files = expand(&[input], &[root.join("converted")]);
        let relatives = files
            .iter()
            .map(|file| file.relative.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(relatives, ["a2.MP4", "a10.mkv", "nested/b1.avi"]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn canonical_path_deduplication_keeps_the_first_item() {
        let root = temp_dir();
        let file = root.join("film.mkv");
        touch(&file);
        let files = expand(
            &[InputItem::File(file.clone()), InputItem::File(file.clone())],
            &[],
        );
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, file);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn output_planning_replaces_extensions_mirrors_paths_and_numbers_collisions() {
        let sources = [
            SourceFile {
                path: PathBuf::from("one/a.mp4"),
                relative: PathBuf::from("a.mp4"),
            },
            SourceFile {
                path: PathBuf::from("two/a.avi"),
                relative: PathBuf::from("a.avi"),
            },
            SourceFile {
                path: PathBuf::from("three/sub/b.mkv"),
                relative: PathBuf::from("sub/b.mkv"),
            },
        ];
        let outputs = plan_outputs(
            &sources,
            &OutputTarget::Folder(PathBuf::from("out")),
            Container::Mkv,
        )
        .unwrap();
        assert_eq!(outputs[0].output, PathBuf::from("out/a.mkv"));
        assert_eq!(outputs[1].output, PathBuf::from("out/a (2).mkv"));
        assert_eq!(outputs[2].output, PathBuf::from("out/sub/b.mkv"));
    }

    #[test]
    fn file_targets_require_one_source_and_default_targets_are_safe() {
        let sources = [
            SourceFile {
                path: PathBuf::from("a.mkv"),
                relative: PathBuf::from("a.mkv"),
            },
            SourceFile {
                path: PathBuf::from("b.mkv"),
                relative: PathBuf::from("b.mkv"),
            },
        ];
        assert!(
            plan_outputs(
                &sources,
                &OutputTarget::File(PathBuf::from("out.mkv")),
                Container::Mkv
            )
            .is_err()
        );
        assert_eq!(
            default_output_target(&[InputItem::File(PathBuf::from("a.mkv"))], Container::Mkv),
            OutputTarget::File(PathBuf::from("converted/a.mkv"))
        );
        assert_eq!(
            default_output_target(
                &[InputItem::Folder {
                    path: PathBuf::from("movies"),
                    recursive: false
                }],
                Container::Mkv
            ),
            OutputTarget::Folder(PathBuf::from("movies/converted"))
        );
    }

    #[test]
    fn excludes_the_effective_output_directory_from_folder_sources() {
        let root = temp_dir();
        let input = InputItem::Folder {
            path: root.join("v"),
            recursive: true,
        };
        let output = OutputTarget::Folder(root.join("v/converted"));
        let exclude = output_exclusions(&output, std::slice::from_ref(&input));
        touch(&root.join("v/a.mkv"));
        touch(&root.join("v/converted/a.mkv"));

        let files = expand(std::slice::from_ref(&input), &exclude);
        assert_eq!(
            files
                .iter()
                .map(|source| source.path.file_name().unwrap().to_string_lossy())
                .collect::<Vec<_>>(),
            ["a.mkv"]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn file_target_excludes_only_its_exact_file_from_a_recursive_source() {
        let root = temp_dir();
        let folder = InputItem::Folder {
            path: root.clone(),
            recursive: true,
        };
        let film = root.join("film.mkv");
        let final_output = root.join("final.mkv");
        touch(&film);
        touch(&final_output);
        let target = OutputTarget::File(final_output.clone());
        let exclude = output_exclusions(&target, std::slice::from_ref(&folder));
        assert_eq!(exclude, [final_output]);

        let files = expand(std::slice::from_ref(&folder), &exclude);
        assert_eq!(
            files
                .iter()
                .map(|source| source.path.file_name().unwrap().to_string_lossy())
                .collect::<Vec<_>>(),
            ["film.mkv"]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn nested_file_target_does_not_exclude_sibling_files() {
        let root = temp_dir();
        let folder = InputItem::Folder {
            path: root.clone(),
            recursive: true,
        };
        let sibling = root.join("sub/other.mkv");
        let final_output = root.join("sub/final.mkv");
        touch(&sibling);
        touch(&final_output);
        let target = OutputTarget::File(final_output);
        let exclude = output_exclusions(&target, std::slice::from_ref(&folder));

        let files = expand(std::slice::from_ref(&folder), &exclude);
        assert_eq!(
            files
                .iter()
                .map(|source| source.relative.to_string_lossy())
                .collect::<Vec<_>>(),
            ["sub/other.mkv"]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn folder_target_does_not_exclude_source_roots_or_ancestor_paths() {
        let root = temp_dir();
        let folder = InputItem::Folder {
            path: root.clone(),
            recursive: true,
        };
        let target = OutputTarget::Folder(root.clone());
        assert!(output_exclusions(&target, std::slice::from_ref(&folder)).is_empty());

        let nested_file = InputItem::File(root.join("sub/film.mkv"));
        assert!(output_exclusions(&target, &[nested_file]).is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}
