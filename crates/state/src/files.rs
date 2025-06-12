//! File and path manipulation for filesystem state.
use std::path::{Component, PathBuf};

use ignore::{overrides::OverrideBuilder, WalkBuilder};
use path_clean;

use super::abspath::IntoAbsPath;

use crate::error::{Error, Result};

const GLOB_START: &str = "*";

/// Normalize a given path to be a relative path under the root. Returns an error if the
/// resulting path is not under the root.
///
/// - If the path starts with relative path components, the path is joined to the CWD.
/// - If the path starts with a glob component (i.e. `*`), it is preserved as-is.
/// - All paths are cleaned to remove redundant ".." and "." components.
///
/// This function only normalizes the path - it does not check if the file exists.
pub fn normalize_path<R, C>(root: R, cwd: C, path: &str) -> Result<PathBuf>
where
    R: IntoAbsPath,
    C: IntoAbsPath,
{
    let root = root.into_abs_path()?;
    let cwd = cwd.into_abs_path()?;
    if path.starts_with(GLOB_START) {
        return Ok(PathBuf::from(path));
    }
    // Convert Windows-style separators to Unix-style for consistent handling
    let normalized_input = path.replace('\\', "/");
    let path = path_clean::clean(&normalized_input);

    let abs_path = if path.is_relative() {
        cwd.join(&path)
    } else {
        path.clone()
    };

    // Manually resolve the path by processing components
    let mut resolved = PathBuf::new();

    for component in abs_path.components() {
        match component {
            Component::Prefix(p) => resolved.push(p.as_os_str()),
            Component::RootDir => resolved.push("/"),
            Component::CurDir => {} // Skip "."
            Component::ParentDir => {
                // Pop the last component if we go up with ".."
                if !resolved.pop() {
                    // If we can't pop, the path goes outside the root
                    return Err(Error::Path(format!(
                        "Path not under current directory: {}",
                        path.display()
                    )));
                }
            }
            Component::Normal(p) => resolved.push(p),
        }
    }

    // Check if the resolved path is under the root
    if !resolved.starts_with(&*root) {
        return Err(Error::Path(format!(
            "Path not under current directory: {}",
            path.display()
        )));
    }

    // Get the relative path from root
    let rel_path = resolved
        .strip_prefix(&*root)
        .map_err(|_| {
            Error::Path(format!(
                "Path not under current directory: {}",
                path.display()
            ))
        })?
        .to_path_buf();

    // The path is valid - it's within the root after resolution
    Ok(rel_path)
}

/// Walk project directory using ignore rules, returning all included files relative to project
/// root.
///
/// Glob patterns can be positive (equivalent to --include) or negative (prefixed with `!`,
/// equivalent to --exclude). If no glob patterns are provided, all files are included.
///
/// Files are sorted by path.
pub fn list_files<R>(root: R, globs: Vec<String>) -> Result<Vec<PathBuf>>
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
            .map_err(|e| Error::Path(format!("Invalid glob pattern: {e}")))?;
    }
    builder
        .add("!/.git")
        .map_err(|e| Error::Path(format!("Invalid glob pattern: {e}")))?; // Don't include the .git directory

    let overrides = builder
        .build()
        .map_err(|e| Error::Path(format!("Failed to build override rules: {e}")))?;

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
        let entry = result.map_err(|e| Error::Path(format!("Walk error: {e}")))?;
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
    use crate::abspath::AbsPath;
    use std::{fs, path::Path, process::Command};
    use tempfile::TempDir;

    #[test]
    fn test_normalize_path() -> Result<()> {
        // Helper to run a test case
        let test = |root: &str, cwd: &str, input: &str, expected: Option<&str>| -> Result<()> {
            let root = AbsPath::new(PathBuf::from(root))?;
            let cwd = AbsPath::new(PathBuf::from(cwd))?;
            let result = normalize_path(root, cwd, input);
            match (result, expected) {
                (Ok(path), Some(exp)) => assert_eq!(path, PathBuf::from(exp)),
                (Err(_), None) => {} // expected error
                (Ok(path), None) => panic!("Expected error for '{}', got: {:?}", input, path),
                (Err(e), Some(exp)) => {
                    panic!("Expected '{}' for '{}', got error: {:?}", exp, input, e)
                }
            }
            Ok(())
        };

        // Glob patterns remain unchanged
        test("/project", "/project/src", "*.rs", Some("*.rs"))?;
        test("/project", "/project/src", "**/test.rs", Some("**/test.rs"))?;

        // Basic path resolution
        test("/project", "/project/src", "test.rs", Some("src/test.rs"))?;
        test(
            "/project",
            "/project/src",
            "/project/src/test.rs",
            Some("src/test.rs"),
        )?;
        test(
            "/project",
            "/anywhere",
            "/project/src/test.rs",
            Some("src/test.rs"),
        )?;

        // Path traversal attempts (should fail)
        test("/project", "/project/src", "../../../etc/passwd", None)?;
        test("/project", "/project/src", "/etc/passwd", None)?;
        test(
            "/project",
            "/project/src",
            "subdir/../../../outside.txt",
            None,
        )?;
        test("/project", "/other/dir", "test.rs", None)?;

        // Valid .. usage within root
        test(
            "/project",
            "/project/src/subdir",
            "../test.rs",
            Some("src/test.rs"),
        )?;
        test(
            "/project",
            "/project/src/subdir",
            "./.././test.rs",
            Some("src/test.rs"),
        )?;
        test("/project", "/project/src", "./test.rs", Some("src/test.rs"))?;

        // Complex path patterns
        test(
            "/project",
            "/project",
            "src/../src/../src/test.rs",
            Some("src/test.rs"),
        )?;

        // Windows-style paths
        test("/project", "/project/src", "..\\..\\..\\etc\\passwd", None)?;
        test("/project", "/project/src", "../..\\../etc/passwd", None)?;

        // Edge cases (these don't escape root)
        test(
            "/project",
            "/project/src",
            "%2e%2e/%2e%2e/etc/passwd",
            Some("src/%2e%2e/%2e%2e/etc/passwd"),
        )?;
        test(
            "/project",
            "/project/src",
            "test.rs\x00.txt",
            Some("src/test.rs\x00.txt"),
        )?;

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
    fn test_list_files() -> Result<()> {
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
