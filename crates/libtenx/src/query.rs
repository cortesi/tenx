use crate::error::Result;
use std::path::{Path, PathBuf};

use crate::workspace::Workspace;

#[derive(Debug)]
pub struct Query {
    pub attach_paths: Vec<PathBuf>,
    pub edit_paths: Vec<PathBuf>,
    pub user_prompt: String,
    pub workspace: Workspace,
}

impl Query {
    pub fn new<P: AsRef<Path>>(
        edit_paths: Vec<P>,
        attach_paths: Vec<P>,
        user_prompt: String,
    ) -> Result<Self> {
        let edit_paths: Vec<PathBuf> = edit_paths
            .into_iter()
            .map(|p| p.as_ref().to_path_buf())
            .collect();
        let attach_paths: Vec<PathBuf> = attach_paths
            .into_iter()
            .map(|p| p.as_ref().to_path_buf())
            .collect();

        let all_paths: Vec<&Path> = edit_paths
            .iter()
            .chain(attach_paths.iter())
            .map(AsRef::as_ref)
            .collect();

        let workspace = Workspace::discover(&all_paths)?;

        // Convert paths to relative paths
        let relative_edit_paths = edit_paths
            .into_iter()
            .map(|p| workspace.relative_path(p))
            .collect::<Result<Vec<PathBuf>>>()?;

        let relative_attach_paths = attach_paths
            .into_iter()
            .map(|p| workspace.relative_path(p))
            .collect::<Result<Vec<PathBuf>>>()?;

        Ok(Query {
            edit_paths: relative_edit_paths,
            attach_paths: relative_attach_paths,
            user_prompt,
            workspace,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutils::{create_dummy_project, TempEnv};
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn test_query_new() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        create_dummy_project(temp_dir.path()).unwrap();

        let _temp_env = TempEnv::new(temp_dir.path())?;

        let edit_paths = vec![
            temp_dir.path().join("crate1/src/lib.rs"),
            temp_dir.path().join("crate2/src/lib.rs"),
        ];
        let attach_paths = vec![temp_dir.path().join("crate3/src/lib.rs")];
        let user_prompt = "Test prompt".to_string();

        let query = Query::new(edit_paths, attach_paths, user_prompt)?;

        assert_eq!(query.edit_paths.len(), 2);
        assert_eq!(query.edit_paths[0], PathBuf::from("crate1/src/lib.rs"));
        assert_eq!(query.edit_paths[1], PathBuf::from("crate2/src/lib.rs"));
        assert_eq!(query.attach_paths.len(), 1);
        assert_eq!(query.attach_paths[0], PathBuf::from("crate3/src/lib.rs"));
        assert_eq!(query.user_prompt, "Test prompt");

        Ok(())
    }

    #[test]
    fn test_query_workspace_discovery() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        create_dummy_project(temp_dir.path()).unwrap();

        let _temp_env = TempEnv::new(temp_dir.path())?;

        let edit_paths = vec![temp_dir.path().join("crate1/src/lib.rs")];
        let attach_paths = vec![temp_dir.path().join("crate2/src/lib.rs")];
        let user_prompt = String::new();

        let query = Query::new(edit_paths, attach_paths, user_prompt)?;

        assert_eq!(
            query.workspace.manifest_path(),
            temp_dir.path().join("Cargo.toml")
        );

        Ok(())
    }

    #[test]
    fn test_query_with_empty_paths() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        create_dummy_project(temp_dir.path()).unwrap();

        let _temp_env = TempEnv::new(temp_dir.path())?;

        let edit_paths: Vec<PathBuf> = vec![];
        let attach_paths: Vec<PathBuf> = vec![];
        let user_prompt = String::new();

        let result = Query::new(edit_paths, attach_paths, user_prompt);

        assert!(result.is_err());

        Ok(())
    }
}
