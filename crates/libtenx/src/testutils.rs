#![cfg(test)]

use std::fs;
use std::path::Path;

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
