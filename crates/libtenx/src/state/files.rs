use std::path::PathBuf;

use ignore::{overrides::OverrideBuilder, WalkBuilder};

use crate::TenxError;

/// Walk project directory using ignore rules, returning all included files relative to project
/// root.
///
/// Applies project glob patterns and uses the ignore crate's functionality for respecting
/// .gitignore and other ignore files. Glob patterns can be positive (include) or negative
/// (exclude, prefixed with !).
pub fn walk_files(root: PathBuf, globs: Vec<String>) -> crate::Result<Vec<PathBuf>> {
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
    fn test_walk_project() -> crate::Result<()> {
        let temp_dir = TempDir::new()?;
        let root = temp_dir.path();

        // Initialize git repo
        init_git_repo(root)?;

        // Create test file structure
        create_file(root, "src/main.rs")?;
        create_file(root, "src/lib.rs")?;
        create_file(root, "tests/test1.rs")?;
        create_file(root, "target/debug/build.rs")?;
        create_file(root, ".gitignore")?;

        // Write gitignore content
        fs::write(root.join(".gitignore"), "/target\n*.tmp\n.git/\n")?;

        let files = walk_files(
            root.to_path_buf(),
            vec!["*.rs".to_string(), "!*.tmp".to_string()],
        )?;

        let expected: Vec<PathBuf> = vec!["src/lib.rs", "src/main.rs", "tests/test1.rs"]
            .into_iter()
            .map(PathBuf::from)
            .collect();

        assert_eq!(files, expected, "Files don't match expected list");

        Ok(())
    }
}
