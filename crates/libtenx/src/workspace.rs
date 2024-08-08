use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::error::{Result, TenxError};

#[derive(Debug)]
pub struct Workspace {
    root_path: PathBuf,
}

impl Workspace {
    pub fn discover<P: AsRef<Path>>(paths: &[P]) -> Result<Self> {
        let common_ancestor = Self::find_common_ancestor(paths)?;
        let root_path = Self::find_workspace_root(&common_ancestor)?;

        Ok(Workspace { root_path })
    }

    fn find_common_ancestor<P: AsRef<Path>>(paths: &[P]) -> Result<PathBuf> {
        if paths.is_empty() {
            return Err(TenxError::Workspace("No paths provided".to_string()));
        }

        let mut common_ancestor = paths[0].as_ref().to_path_buf();
        for path in paths.iter().skip(1) {
            while !path.as_ref().starts_with(&common_ancestor) {
                if !common_ancestor.pop() {
                    return Err(TenxError::Workspace("No common ancestor found".to_string()));
                }
            }
        }

        Ok(common_ancestor)
    }

    fn find_workspace_root(start_dir: &Path) -> Result<PathBuf> {
        let mut current_dir = if start_dir.is_absolute() {
            start_dir.to_path_buf()
        } else {
            std::env::current_dir()?.join(start_dir)
        };

        loop {
            let cargo_toml = current_dir.join("Cargo.toml");
            if cargo_toml.exists() {
                return Ok(current_dir);
            }
            if !current_dir.pop() {
                break;
            }
        }
        Err(TenxError::Workspace("Workspace root not found".to_string()))
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.root_path.join("Cargo.toml")
    }

    pub fn relative_path<P: AsRef<Path>>(&self, path: P) -> Result<PathBuf> {
        let path = self.to_absolute_path(path)?;

        path.strip_prefix(&self.root_path)
            .map(|p| p.to_path_buf())
            .map_err(|e| TenxError::Workspace(format!("Failed to get relative path: {}", e)))
    }

    pub fn read_file<P: AsRef<Path>>(&self, path: P) -> Result<String> {
        let full_path = self.root_path.join(path);
        fs::read_to_string(&full_path).map_err(|e| {
            TenxError::Workspace(format!(
                "Failed to read file '{}': {}",
                full_path.display(),
                e
            ))
        })
    }

    pub fn write_file<P: AsRef<Path>>(&self, path: P, content: &str) -> Result<()> {
        let full_path = self.root_path.join(path);

        // Ensure the parent directory exists
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                TenxError::Workspace(format!(
                    "Failed to create directory '{}': {}",
                    parent.display(),
                    e
                ))
            })?;
        }

        fs::write(&full_path, content).map_err(|e| {
            TenxError::Workspace(format!(
                "Failed to write file '{}': {}",
                full_path.display(),
                e
            ))
        })
    }

    fn to_absolute_path<P: AsRef<Path>>(&self, path: P) -> Result<PathBuf> {
        let path = path.as_ref();
        if path.is_absolute() {
            Ok(path.to_path_buf())
        } else {
            std::env::current_dir()
                .map(|current_dir| current_dir.join(path))
                .map_err(|e| {
                    TenxError::Workspace(format!("Failed to get current directory: {}", e))
                })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    use crate::testutils::{create_dummy_project, TempEnv};

    #[test]
    fn test_discover_workspace() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        create_dummy_project(temp_dir.path()).unwrap();

        let _temp_env = TempEnv::new(temp_dir.path())?;

        let paths = vec![
            temp_dir.path().join("crate1/src/lib.rs"),
            temp_dir.path().join("crate2/src/lib.rs"),
        ];

        let workspace = Workspace::discover(&paths)?;

        assert_eq!(
            workspace.manifest_path(),
            temp_dir.path().join("Cargo.toml")
        );

        Ok(())
    }

    #[test]
    fn test_discover_single_crate() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        create_dummy_project(temp_dir.path()).unwrap();

        let _temp_env = TempEnv::new(temp_dir.path())?;

        let paths = vec![temp_dir.path().join("crate1/src/lib.rs")];

        let workspace = Workspace::discover(&paths)?;

        assert_eq!(
            workspace.manifest_path(),
            temp_dir.path().join("crate1/Cargo.toml")
        );

        Ok(())
    }

    #[test]
    fn test_no_cargo_toml() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();

        let _temp_env = TempEnv::new(temp_dir.path())?;

        let paths = vec![temp_dir.path().to_path_buf()];

        let result = Workspace::discover(&paths);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().ends_with("root not found"));

        Ok(())
    }

    #[test]
    fn test_no_paths_provided() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        let _temp_env = TempEnv::new(temp_dir.path())?;

        let paths: Vec<PathBuf> = vec![];

        let result = Workspace::discover(&paths);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .ends_with("No paths provided"));

        Ok(())
    }

    #[test]
    fn test_no_common_ancestor() -> Result<()> {
        let temp_dir1 = TempDir::new().unwrap();
        let temp_dir2 = TempDir::new().unwrap();

        let _temp_env = TempEnv::new(&temp_dir1)?;

        let paths = vec![
            temp_dir1.path().to_path_buf(),
            temp_dir2.path().to_path_buf(),
        ];

        let result = Workspace::discover(&paths);

        assert!(result.is_err());

        Ok(())
    }

    #[test]
    fn test_relative_path_with_absolute_input() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        create_dummy_project(temp_dir.path()).unwrap();

        let _temp_env = TempEnv::new(temp_dir.path())?;

        let paths = vec![temp_dir.path().join("crate1/src/lib.rs")];
        let workspace = Workspace::discover(&paths)?;

        let absolute_path = temp_dir.path().join("crate1/src/main.rs");
        let relative_path = workspace.relative_path(absolute_path)?;

        assert_eq!(relative_path, PathBuf::from("src/main.rs"));

        Ok(())
    }

    #[test]
    fn test_relative_path_outside_workspace() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        create_dummy_project(temp_dir.path()).unwrap();

        let _temp_env = TempEnv::new(temp_dir.path())?;

        let paths = vec![temp_dir.path().join("crate1/src/lib.rs")];
        let workspace = Workspace::discover(&paths)?;

        let outside_path = temp_dir.path().join("../outside.rs");
        let result = workspace.relative_path(outside_path);

        assert!(result.is_err());

        Ok(())
    }

    #[test]
    fn test_get_contents() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        create_dummy_project(temp_dir.path()).unwrap();

        let _temp_env = TempEnv::new(temp_dir.path())?;

        let paths = vec![temp_dir.path().join("crate1/src/lib.rs")];
        let workspace = Workspace::discover(&paths)?;

        // Test reading an existing file
        let contents = workspace.read_file("src/lib.rs")?;
        assert_eq!(contents.trim(), "// Dummy content");

        // Test reading a non-existent file
        let result = workspace.read_file("src/nonexistent.txt");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Failed to read file"));

        Ok(())
    }

    #[test]
    fn test_write_file() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        create_dummy_project(temp_dir.path()).unwrap();

        let _temp_env = TempEnv::new(temp_dir.path())?;

        let paths = vec![temp_dir.path().join("crate1/src/lib.rs")];
        let workspace = Workspace::discover(&paths)?;

        // Test writing a new file
        workspace.write_file("src/new_file.rs", "// New file content")?;
        let contents = workspace.read_file("src/new_file.rs")?;
        assert_eq!(contents.trim(), "// New file content");

        // Test overwriting an existing file
        workspace.write_file("src/lib.rs", "// Updated content")?;
        let contents = workspace.read_file("src/lib.rs")?;
        assert_eq!(contents.trim(), "// Updated content");

        // Test writing a file in a new directory
        workspace.write_file("new_dir/new_file.txt", "Content in new directory")?;
        let contents = workspace.read_file("new_dir/new_file.txt")?;
        assert_eq!(contents.trim(), "Content in new directory");

        Ok(())
    }
}
