#![cfg(test)]

use crate::{config::Config, Session};
use fs_err as fs;
use std::path::Path;
use tempfile::{tempdir, TempDir};

pub fn create_dummy_project(temp_dir: &Path) -> std::io::Result<()> {
    // Create workspace Cargo.toml
    fs::write(
        temp_dir.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crate1\", \"crate2\"]",
    )?;

    // Create crate1
    fs::create_dir(temp_dir.join("crate1"))?;
    fs::write(
        temp_dir.join("crate1/Cargo.toml"),
        "[package]\nname = \"crate1\"\nversion = \"0.1.0\"",
    )?;
    fs::create_dir(temp_dir.join("crate1/src"))?;
    fs::write(temp_dir.join("crate1/src/lib.rs"), "// Dummy content")?;

    // Create crate2
    fs::create_dir(temp_dir.join("crate2"))?;
    fs::write(
        temp_dir.join("crate2/Cargo.toml"),
        "[package]\nname = \"crate2\"\nversion = \"0.1.0\"",
    )?;
    fs::create_dir(temp_dir.join("crate2/src"))?;
    fs::write(temp_dir.join("crate2/src/lib.rs"), "// Dummy content")?;

    Ok(())
}

/// Creates a file tree structure in the given directory based on the provided paths.
pub fn create_file_tree(dir: &Path, paths: &[&str]) -> std::io::Result<()> {
    for path in paths {
        let full_path = dir.join(path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::File::create(full_path)?;
    }
    Ok(())
}

/// A structure representing a mock project for testing purposes.
pub struct TestProject {
    /// The configuration for the mock project.
    pub config: Config,
    /// The session associated with the mock project.
    pub session: Session,
    /// A temporary directory for the mock project.
    pub tempdir: TempDir,
}

/// Creates a new MockProject instance.
//
/// This function sets up a temporary directory and initializes a Config and Session
/// for use in tests.
pub fn test_project() -> TestProject {
    let tempdir = tempdir().unwrap();
    let tempdir_path = tempdir.path().to_path_buf();

    let config = Config::default()
        .with_root(&tempdir_path)
        .with_test_cwd(tempdir_path);

    let session = Session::default();

    TestProject {
        config,
        session,
        tempdir,
    }
}

impl TestProject {
    /// Creates a file tree structure in the mock project's temporary directory.
    ///
    /// # Arguments
    ///
    /// * `paths` - A slice of string slices representing the paths of files to create.
    ///
    /// # Panics
    ///
    /// Panics if the file tree creation fails.
    pub fn create_file_tree(&self, paths: &[&str]) {
        create_file_tree(self.tempdir.path(), paths).expect("Failed to create file tree");
    }

    /// Sets the current working directory for the mock project's configuration.
    ///
    /// # Arguments
    ///
    /// * `path` - A path (relative to the temporary directory) to set as the new working directory.
    pub fn set_cwd<P: AsRef<Path>>(&mut self, path: P) {
        let new_cwd = self.tempdir.path().join(path);
        self.config = std::mem::take(&mut self.config).with_test_cwd(new_cwd);
    }

    /// Writes content to a file in the mock project's temporary directory.
    ///
    /// # Arguments
    ///
    /// * `path` - A path (relative to the temporary directory) where the file should be written.
    /// * `content` - The content to write to the file.
    ///
    /// # Panics
    ///
    /// Panics if the write operation fails.
    pub fn write<P: AsRef<Path>, C: AsRef<[u8]>>(&self, path: P, content: C) {
        fs::write(self.tempdir.path().join(path), content).expect("Failed to write file");
    }

    /// Reads content from a file in the mock project's temporary directory.
    ///
    /// # Arguments
    ///
    /// * `path` - A path (relative to the temporary directory) of the file to read.
    ///
    /// # Panics
    ///
    /// Panics if the read operation fails.
    pub fn read<P: AsRef<Path>>(&self, path: P) -> String {
        fs::read_to_string(self.tempdir.path().join(path)).expect("Failed to read file")
    }
}
