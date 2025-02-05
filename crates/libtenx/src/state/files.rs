use std::path::{Path, PathBuf};

use globset::Glob;
use ignore::{overrides::OverrideBuilder, WalkBuilder};
use pathdiff::diff_paths;

use crate::TenxError;

/// Walk project directory using ignore rules, returning all included files relative to project
/// root.
///
/// Applies project glob patterns and uses the ignore crate's functionality for respecting
/// .gitignore and other ignore files. Glob patterns can be positive (include) or negative
/// (exclude, prefixed with !).
///
/// Files are sorted by path.
use crate::state::abspath::AbsPath;

pub fn list_files(root: AbsPath, globs: Vec<String>) -> crate::Result<Vec<PathBuf>> {
    // Build override rules from project config
    let mut builder = OverrideBuilder::new(&root);

    // Add glob patterns directly - they're already in the correct format
    for pattern in &globs {
        builder
            .add(pattern)
            .map_err(|e| TenxError::Internal(format!("Invalid glob pattern: {}", e)))?;
    }
    builder
        .add("!/.git")
        .map_err(|e| TenxError::Internal(format!("Invalid glob pattern: {}", e)))?; // Don't include the .git directory

    let overrides = builder
        .build()
        .map_err(|e| TenxError::Internal(format!("Failed to build override rules: {}", e)))?;

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
        let entry = result.map_err(|e| TenxError::Internal(format!("Walk error: {}", e)))?;
        if entry.file_type().is_some_and(|ft| ft.is_file()) {
            if let Ok(path) = entry.path().strip_prefix(&root) {
                files.push(path.to_path_buf());
            }
        }
    }

    Ok(files)
}

/// Traverse the root using the include patterns provided, finding files that match `pattern`.
/// The pattern can be either a glob match pattern or a simple path.
///
/// # Arguments
///
/// * `root` - The absolute root path to search from
/// * `include` - List of glob patterns to include in the search
/// * `current_dir` - The absolute path of the current directory, which must be equal to or under `root`
/// * `pattern` - The glob pattern to match against files
///
/// # Errors
///
/// Returns an error if:
/// - The pattern is invalid
/// - The current_dir is not equal to or under root
/// - Any matched file does not exist
pub fn find_files(
    root: AbsPath,
    include: Vec<String>,
    current_dir: AbsPath,
    pattern: &str,
) -> crate::Result<Vec<PathBuf>> {
    // Verify that current_dir is under root
    if !current_dir.starts_with(&*root) {
        return Err(TenxError::Internal(format!(
            "Current directory {} must be under root {}",
            current_dir, root
        )));
    }

    let glob = Glob::new(pattern)
        .map_err(|e| TenxError::Internal(format!("Invalid glob pattern: {}", e)))?;
    let included_files = list_files(root.clone(), include)?;

    let mut matched_files = Vec::new();

    for file in included_files {
        let relative_path = if file.is_absolute() {
            file.strip_prefix(root.clone()).unwrap_or(&file)
        } else {
            &file
        };

        let match_path = if current_dir.as_ref() != root.as_ref() {
            // If we're in a subdirectory, we need to adjust the path for matching
            diff_paths(
                relative_path,
                Path::new(&*current_dir)
                    .strip_prefix(&*root)
                    .unwrap_or(Path::new("")),
            )
            .unwrap_or_else(|| relative_path.to_path_buf())
        } else {
            relative_path.to_path_buf()
        };

        if glob.compile_matcher().is_match(&match_path) {
            let absolute_path = root.join(relative_path);
            if absolute_path.exists() {
                matched_files.push(relative_path.to_path_buf());
            } else {
                return Err(TenxError::Internal(format!(
                    "File does not exist: {:?}",
                    absolute_path
                )));
            }
        }
    }

    Ok(matched_files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path, process::Command};
    use tempfile::TempDir;

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
