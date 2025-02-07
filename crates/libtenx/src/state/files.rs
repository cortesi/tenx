//! File and path manipulation for filesystem state.
use std::path::PathBuf;

use ignore::{overrides::OverrideBuilder, WalkBuilder};
use path_clean;
use pathdiff::diff_paths;

use super::abspath::IntoAbsPath;
use crate::TenxError;

const GLOB_START: &str = "*";

/// Normalize a given path to be a relative path under the root. Returns an error if the
/// resulting path is not under the root.
///
/// - If the path starts with relative path components, the path is joined to the CWD.
/// - If the path starts with a glob component (i.e. `*`), it is preserved as-is.
/// - All paths are cleaned to remove redundant ".." and "." components.
///
/// This function only normalizes the path - it does not check if the file exists.
pub fn normalize_path<R, C>(root: R, cwd: C, path: &str) -> crate::Result<PathBuf>
where
    R: IntoAbsPath,
    C: IntoAbsPath,
{
    let root = root.into_abs_path()?;
    let cwd = cwd.into_abs_path()?;
    if path.starts_with(GLOB_START) {
        return Ok(PathBuf::from(path));
    }
    let path = path_clean::clean(path);

    let abs_path = if path.is_relative() {
        cwd.join(&path)
    } else {
        path.clone()
    };

    let rel_path = diff_paths(&abs_path, &root).ok_or_else(|| {
        TenxError::Path(format!(
            "Path not under current directory: {}",
            path.display()
        ))
    })?;
    if rel_path.starts_with("..") {
        return Err(TenxError::Path(format!(
            "Path not under current directory: {}",
            path.display()
        )));
    }
    Ok(rel_path)
}

/// Walk project directory using ignore rules, returning all included files relative to project
/// root.
///
/// Glob patterns can be positive (equivalent to --include) or negative (prefixed with `!`,
/// equivalent to --exclude). If no glob patterns are provided, all files are included.
///
/// Files are sorted by path.
pub fn list_files<R>(root: R, globs: Vec<String>) -> crate::Result<Vec<PathBuf>>
where
    R: IntoAbsPath,
{
    let root = root.into_abs_path()?;
    // Build override rules from project config
    let mut builder = OverrideBuilder::new(&root);

    // Add glob patterns directly - they're already in the correct format
    for pattern in &globs {
        builder
            .add(pattern)
            .map_err(|e| TenxError::Path(format!("Invalid glob pattern: {}", e)))?;
    }
    builder
        .add("!/.git")
        .map_err(|e| TenxError::Path(format!("Invalid glob pattern: {}", e)))?; // Don't include the .git directory

    let overrides = builder
        .build()
        .map_err(|e| TenxError::Path(format!("Failed to build override rules: {}", e)))?;

    // Build and configure the walker
    let mut walker = WalkBuilder::new(&root);
    walker
        .hidden(false) // Don't skip hidden files
        .git_ignore(true) // Respect .gitignore
        .git_global(true) // Respect global gitignore
        .git_exclude(true) // Respect .git/info/exclude
        .overrides(overrides)
        .sort_by_file_path(|a, b| a.cmp(b)); // Sort files by path

    // Collect all files, converting to relative paths
    let mut files = Vec::new();
    for result in walker.build() {
        let entry = result.map_err(|e| TenxError::Path(format!("Walk error: {}", e)))?;
        if entry.file_type().is_some_and(|ft| ft.is_file()) {
            if let Ok(path) = entry.path().strip_prefix(&root) {
                files.push(path.to_path_buf());
            }
        }
    }

    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::abspath::AbsPath;
    use std::{fs, path::Path, process::Command};
    use tempfile::TempDir;

    #[test]
    fn test_normalize_path() -> crate::Result<()> {
        struct TestCase {
            name: &'static str,
            root: &'static str,
            cwd: &'static str,
            input: &'static str,
            expected: Option<&'static str>,
        }

        let cases = vec![
            // Glob patterns must remain unchanged.
            TestCase {
                name: "simple glob pattern",
                root: "/project",
                cwd: "/project/src",
                input: "*.rs",
                expected: Some("*.rs"),
            },
            TestCase {
                name: "recursive glob pattern",
                root: "/project",
                cwd: "/project/src",
                input: "**/test.rs",
                expected: Some("**/test.rs"),
            },
            // Relative path: joining with cwd yields a path relative to project root.
            TestCase {
                name: "relative path",
                root: "/project",
                cwd: "/project/src",
                input: "test.rs",
                expected: Some("src/test.rs"),
            },
            // Absolute path under root should normalize correctly.
            TestCase {
                name: "absolute path under root",
                root: "/project",
                cwd: "/project/src",
                input: "/project/src/test.rs",
                expected: Some("src/test.rs"),
            },
            // Paths outside the root should yield an error.
            TestCase {
                name: "path outside root",
                root: "/project",
                cwd: "/project/src",
                input: "/outside.rs",
                expected: None,
            },
            // Test with cwd outside root - should fail
            TestCase {
                name: "cwd outside root",
                root: "/project",
                cwd: "/other/dir",
                input: "test.rs",
                expected: None,
            },
            // Test with cwd at different absolute path but same relative path
            TestCase {
                name: "cwd in different absolute path",
                root: "/other/project",
                cwd: "/other/project/src",
                input: "test.rs",
                expected: Some("src/test.rs"),
            },
            // Test with root deep in filesystem
            TestCase {
                name: "deep root path",
                root: "/very/deep/project/path",
                cwd: "/very/deep/project/path/src",
                input: "test.rs",
                expected: Some("src/test.rs"),
            },
        ];

        for case in cases {
            let root = AbsPath::new(PathBuf::from(case.root))?;
            let cwd = AbsPath::new(PathBuf::from(case.cwd))?;
            let result = normalize_path(root, cwd, case.input);
            match (result, case.expected) {
                (Ok(path), Some(exp)) => {
                    assert_eq!(
                        path,
                        PathBuf::from(exp),
                        "Failed case: {}. Input: {}",
                        case.name,
                        case.input
                    );
                }
                (Err(_), None) => { /* expected error */ }
                (Ok(path), None) => {
                    panic!(
                        "Expected error for case '{}', but got path: {:?}",
                        case.name, path
                    );
                }
                (Err(e), Some(exp)) => {
                    panic!(
                        "Expected path '{}' for case '{}', but got error: {:?}",
                        exp, case.name, e
                    );
                }
            }
        }
        Ok(())
    }

    fn create_file(root: &Path, path: &str) -> std::io::Result<()> {
        let full_path = root.join(path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(full_path, "")?;
        Ok(())
    }

    fn init_git_repo(path: &Path) -> std::io::Result<()> {
        Command::new("git")
            .args(["init"])
            .current_dir(path)
            .output()?;
        Ok(())
    }

    #[test]
    fn test_list_files() -> crate::Result<()> {
        let temp_dir = TempDir::new()?;
        let root = AbsPath::new(temp_dir.path().to_path_buf())?;

        // Initialize git repo
        init_git_repo(&root)?;

        // Create test file structure
        create_file(&root, "src/main.rs")?;
        create_file(&root, "src/lib.rs")?;
        create_file(&root, "tests/test1.rs")?;
        create_file(&root, "target/debug/build.rs")?;
        create_file(&root, ".gitignore")?;

        // Write gitignore content
        fs::write(root.join(".gitignore"), "/target\n*.tmp\n.git/\n")?;

        let files = list_files(root.clone(), vec!["*.rs".to_string(), "!*.tmp".to_string()])?;

        let expected: Vec<PathBuf> = vec!["src/lib.rs", "src/main.rs", "tests/test1.rs"]
            .into_iter()
            .map(PathBuf::from)
            .collect();

        assert_eq!(files, expected, "Files don't match expected list");

        Ok(())
    }
}
