//! A unified interface over all persistent state.
//!
//! All modifications are made through `Patch` operations, and return an ID that can be used
//! to revert the state to a previous snapshot.
//!
//! The actual state consists of a filesystem directory and an in-memory store. Files in the
//! in-memory store are prefixed with `::`.
mod directory;
mod error;
mod memory;

mod abspath;
pub mod files;
mod patch;

pub use crate::error::*;
pub use crate::patch::*;

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fmt::Debug,
    path::{Path, PathBuf},
};

use globset::Glob;
use serde::{Deserialize, Serialize};

/// Prefix for in-memory files
pub const MEM_PREFIX: &str = "::";

trait SubStore: Debug {
    fn list(&self) -> Result<Vec<PathBuf>>;
    fn read(&self, path: &Path) -> Result<String>;
    fn write(&mut self, path: &Path, content: &str) -> Result<()>;
    fn remove(&mut self, path: &Path) -> Result<()>;
}

/// Information about a patch operation, including success/failure counts and any errors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchInfo {
    /// Rollback ID which can be used to revert this patch
    pub rollback_id: u64,
    /// Number of patch operations that succeeded
    pub succeeded: usize,
    /// All patch failures with their associated operations
    pub failures: Vec<PatchFailure>,
}

impl PatchInfo {
    pub fn add_failure(&mut self, change: Operation, error: Error) -> Result<()> {
        match error {
            Error::Patch { user, model } => {
                self.failures.push(PatchFailure {
                    user,
                    model,
                    operation: change,
                });
                Ok(())
            }
            _ => Err(error),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Snapshot {
    content: HashMap<PathBuf, String>,
    created: Vec<PathBuf>,
}

impl Snapshot {
    pub fn insert(&mut self, path: PathBuf, content: String) {
        self.content.insert(path, content);
    }

    pub fn create(&mut self, path: PathBuf) {
        self.content.insert(path.clone(), String::new());
        self.created.push(path);
    }

    /// Returns a unique list of all files affected (touched or changed) by the snapshot.
    pub fn affected(&self) -> Vec<PathBuf> {
        let mut affected = BTreeSet::new();
        for path in self.content.keys() {
            affected.insert(path.clone());
        }
        for path in &self.created {
            affected.insert(path.clone());
        }
        affected.into_iter().collect()
    }
}

/// The state underlying a session. This is the set of resources that our models are editing. State
/// presents a unified interface over an optional filesystem directory and a memory store.
/// In-memory file names are prefixed with "::"
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct State {
    directory: Option<directory::Directory>,
    memory: memory::Memory,
    snapshots: Vec<(u64, Snapshot)>,
    next_snapshot_id: u64,
    /// Track which snapshot IDs represent actual modifications (not just views)
    modification_ids: HashSet<u64>,
}

impl State {
    /// Generate a diff of changes made to a file since the first snapshot.
    ///
    /// If the file has changed significantly (more than 50% of lines), a single
    /// WriteFile operation will be used instead of multiple Replace operations.
    pub fn diff_path(&self, path: impl AsRef<Path>) -> Result<Patch> {
        // Convert to PathBuf to ensure consistency
        let path_buf = path.as_ref().to_path_buf();

        // Get original and current content
        let original_content = self.original(path_buf.as_path()).unwrap_or_default();
        let current_content = self.read(path_buf.as_path())?;

        // Generate a diff using diffy
        let diff = diffy::create_patch(&original_content, &current_content);

        let mut changes = Vec::new();
        let mut total_replaced_lines = 0;
        let total_lines = current_content.lines().count();

        // Convert diff hunks to Change::Replace operations
        for hunk in diff.hunks() {
            let mut old_content = String::new();
            let mut new_content = String::new();
            let mut hunk_replaced_lines = 0;

            for line in hunk.lines() {
                match line {
                    diffy::Line::Context(text) => {
                        old_content.push_str(text);
                        new_content.push_str(text);
                    }
                    diffy::Line::Delete(text) => {
                        old_content.push_str(text);
                        hunk_replaced_lines += 1;
                    }
                    diffy::Line::Insert(text) => {
                        new_content.push_str(text);
                        hunk_replaced_lines += 1;
                    }
                }
            }

            total_replaced_lines += hunk_replaced_lines;

            changes.push(Operation::ReplaceFuzzy(patch::ReplaceFuzzy {
                path: path_buf.clone(),
                old: old_content,
                new: new_content,
            }));
        }

        // If more than half the file has changed, or if file is being completely emptied or filled,
        // use a single Write operation instead
        if total_lines == 0
            || original_content.is_empty()
            || current_content.is_empty()
            || (total_lines > 0 && (total_replaced_lines as f64) / (total_lines as f64) > 0.5)
        {
            changes = vec![Operation::Write(WriteFile {
                path: path_buf,
                content: current_content,
            })];
        }

        Ok(Patch { ops: changes })
    }

    /// Set the directory path and glob patterns for file operations.
    ///
    /// Glob patterns can be positive (equivalent to --include) or negative (prefixed with `!`,
    /// equivalent to --exclude). If no glob patterns are provided, all files are included.
    pub fn with_directory<P>(mut self, root: P, globs: Vec<String>) -> Result<Self>
    where
        P: abspath::IntoAbsPath,
    {
        let abs = root.into_abs_path()?;
        self.directory = Some(directory::Directory::new(abs, globs)?);
        Ok(self)
    }

    /// Initialize the state with pre-populated memory contents.
    ///
    /// This method takes a HashMap mapping file paths to their contents and
    /// adds these files to the memory store of the State.
    pub fn with_memory(mut self, files: HashMap<PathBuf, String>) -> Result<Self> {
        for (path, content) in files {
            self.memory.write(&path, &content)?;
        }
        Ok(self)
    }

    /// Dispatches an operation to the appropriate immutable store based on the path prefix.
    fn dispatch_ro<T, F>(&self, path: &Path, f: F) -> Result<T>
    where
        F: FnOnce(&dyn SubStore) -> Result<T>,
    {
        if path.to_string_lossy().starts_with(MEM_PREFIX) {
            f(&self.memory)
        } else if let Some(ref fs) = self.directory {
            f(fs)
        } else {
            Err(Error::NotFound {
                msg: "No matching store".to_string(),
                path: path.display().to_string(),
            })
        }
    }

    /// Dispatches an operation to the appropriate mutable store based on the path prefix.
    fn dispatch_mut<T, F>(&mut self, path: &Path, f: F) -> Result<T>
    where
        F: FnOnce(&mut dyn SubStore) -> Result<T>,
    {
        if path.to_string_lossy().starts_with(MEM_PREFIX) {
            f(&mut self.memory)
        } else if let Some(ref mut fs) = self.directory {
            f(fs)
        } else {
            Err(Error::NotFound {
                msg: "No matching store".to_string(),
                path: path.display().to_string(),
            })
        }
    }

    /// Retrieves the content associated with the given path.
    pub fn read(&self, path: &Path) -> Result<String> {
        self.dispatch_ro(path, |store| store.read(path))
    }

    /// Writes content to a path.
    fn write(&mut self, path: &Path, content: &str) -> Result<()> {
        self.dispatch_mut(path, |store| store.write(path, content))
    }

    /// Removes a file or memory entry for the given path.
    fn remove(&mut self, path: &Path) -> Result<()> {
        self.dispatch_mut(path, |store| store.remove(path))
    }

    /// Creates a snapshot of the given list of paths. For each path, if the file exists, its content is captured;
    /// otherwise, the path is marked as created.
    fn create_snapshot(&self, paths: &[PathBuf]) -> Result<Snapshot> {
        let mut snap = Snapshot::default();
        for p in paths {
            match self.read(p) {
                Ok(content) => snap.insert(p.clone(), content),
                Err(Error::NotFound { .. }) => snap.create(p.clone()),
                Err(e) => return Err(e),
            }
        }
        Ok(snap)
    }

    /// Reverts the state to the given snapshot.
    /// Restores content for files or memory entries that existed and removes those that were created.
    fn revert_snapshot(&mut self, snapshot: Snapshot) -> Result<()> {
        for path in snapshot.created.iter() {
            self.remove(path)?;
        }
        for (path, content) in snapshot.content.iter() {
            if !snapshot.created.contains(path) {
                self.write(path, content)?;
            }
        }
        Ok(())
    }

    fn push_snapshot(&mut self, snapshot: Snapshot) -> u64 {
        let id = self.next_snapshot_id;
        self.snapshots.push((self.next_snapshot_id, snapshot));
        self.next_snapshot_id += 1;
        id
    }

    /// Creates a snapshot from the provided paths, appends it to the snapshots list, and returns its identifier.
    pub fn snapshot(&mut self, paths: &[PathBuf]) -> Result<u64> {
        let snap = self.create_snapshot(paths)?;
        Ok(self.push_snapshot(snap))
    }

    /// Applies a patch by taking a snapshot of all files to be modified, then attempts to apply each change in the patch.
    /// If any change fails, the error is collected in a vector of (change, error) tuples.
    /// Returns a tuple containing the snapshot ID and a vector of failed changes.
    pub fn patch(&mut self, patch: &Patch) -> Result<PatchInfo> {
        let snap = self.create_snapshot(&patch.affected_files())?;
        let mut pinfo = PatchInfo {
            rollback_id: 0,
            succeeded: 0,
            failures: Vec::new(),
        };

        // Track if this patch contains any successfully applied modifying operations
        let mut has_modifications = false;

        for change in &patch.ops {
            match change {
                Operation::Write(write_file) => {
                    if let Err(e) = self.write(write_file.path.as_path(), &write_file.content) {
                        pinfo.add_failure(change.clone(), e)?;
                    } else {
                        pinfo.succeeded += 1;
                        if change.is_modification() {
                            has_modifications = true;
                        }
                    }
                }
                Operation::ReplaceFuzzy(replace) => {
                    let res = (|| {
                        let original = self.read(replace.path.as_path())?;
                        let new_content = replace.apply(&original)?;
                        self.write(replace.path.as_path(), &new_content)
                    })();
                    if let Err(e) = res {
                        pinfo.add_failure(change.clone(), e)?;
                    } else {
                        pinfo.succeeded += 1;
                        if change.is_modification() {
                            has_modifications = true;
                        }
                    }
                }
                Operation::Replace(replace) => {
                    let res = (|| {
                        let original = self.read(replace.path.as_path())?;
                        let new_content = replace.apply(&original)?;
                        self.write(replace.path.as_path(), &new_content)
                    })();
                    if let Err(e) = res {
                        pinfo.add_failure(change.clone(), e)?;
                    } else {
                        pinfo.succeeded += 1;
                        if change.is_modification() {
                            has_modifications = true;
                        }
                    }
                }
                Operation::Insert(insert) => {
                    let res = (|| {
                        let original = self.read(insert.path.as_path())?;
                        let new_content = insert.apply(&original)?;
                        self.write(insert.path.as_path(), &new_content)
                    })();
                    if let Err(e) = res {
                        pinfo.add_failure(change.clone(), e)?;
                    } else {
                        pinfo.succeeded += 1;
                        if change.is_modification() {
                            has_modifications = true;
                        }
                    }
                }
                Operation::View(_) => {
                    pinfo.succeeded += 1;
                }
                Operation::Undo(path) => {
                    let res = (|| {
                        if let Some(previous_content) = self.last_original(path) {
                            self.write(path, &previous_content)?;
                            Ok(())
                        } else {
                            let msg =
                                format!("No previous version found for undo: {}", path.display());
                            Err(Error::Patch {
                                user: msg.clone(),
                                model: msg,
                            })
                        }
                    })();
                    if let Err(e) = res {
                        pinfo.add_failure(change.clone(), e)?;
                    } else {
                        pinfo.succeeded += 1;
                        if change.is_modification() {
                            has_modifications = true;
                        }
                    }
                }
                Operation::ViewRange(_path, _, _) => {
                    pinfo.succeeded += 1;
                }
            }
        }
        pinfo.rollback_id = self.push_snapshot(snap);

        // Track this snapshot as a modification if it contained any modifying operations
        if has_modifications {
            self.modification_ids.insert(pinfo.rollback_id);
        }

        Ok(pinfo)
    }

    /// Reverts all snapshots up to and including the given ID in reverse order, then removes them from the snapshots list.
    pub fn revert(&mut self, id: u64) -> Result<()> {
        let mut to_revert = Vec::new();
        let mut remaining = Vec::new();
        for pair in self.snapshots.drain(..) {
            if pair.0 <= id {
                to_revert.push(pair);
            } else {
                remaining.push(pair);
            }
        }
        if to_revert.is_empty() {
            return Err(Error::Internal(format!("Snapshot id {id} not found")));
        }

        // Clean up modification IDs for reverted snapshots
        for (snap_id, _snap) in to_revert.iter() {
            self.modification_ids.remove(snap_id);
        }

        for (_id, snap) in to_revert.into_iter().rev() {
            self.revert_snapshot(snap)?;
        }
        self.snapshots = remaining;
        Ok(())
    }

    /// Lists all files from both the memory and directory stores.
    pub fn list(&self) -> Result<Vec<PathBuf>> {
        let mut files = self.memory.list()?;
        if let Some(ref fs) = self.directory {
            files.extend(fs.list()?);
        }
        Ok(files)
    }

    /// Returns the files that were last changed between the given snapshot ids, inclusive. Returns
    /// an empty list if no snapshots exist.
    pub fn last_changed_between(
        &self,
        start: Option<u64>,
        end: Option<u64>,
    ) -> Result<Vec<PathBuf>> {
        if self.snapshots.is_empty() {
            return Ok(vec![]);
        }
        let min_id = start.unwrap_or_else(|| self.snapshots.first().unwrap().0);
        let max_id = end.unwrap_or_else(|| self.snapshots.last().unwrap().0);
        let mut latest: HashMap<PathBuf, u64> = HashMap::new();
        for (snap_id, snap) in &self.snapshots {
            for path in snap.affected() {
                latest
                    .entry(path)
                    .and_modify(|e| {
                        if *snap_id > *e {
                            *e = *snap_id
                        }
                    })
                    .or_insert(*snap_id);
            }
        }
        let mut result: Vec<PathBuf> = latest
            .into_iter()
            .filter_map(|(path, id)| {
                if id >= min_id && id <= max_id {
                    Some(path)
                } else {
                    None
                }
            })
            .collect();
        result.sort();
        Ok(result)
    }

    /// Returns the content of the file in the most recent snapshot prior to the current revision.
    /// This is used to support Undo operations.
    pub fn last_original(&self, path: &Path) -> Option<String> {
        if self.snapshots.is_empty() {
            return None;
        }
        let snap = self
            .snapshots
            .iter()
            .filter(|(_, s)| s.content.contains_key(path))
            .next_back();

        if let Some((_, snap)) = snap {
            snap.content.get(path).cloned()
        } else {
            None
        }
    }

    /// Returns a unique, sorted list of all files touched, changed or created in the current
    /// session. This includes all files that have been modified in any snapshot.
    pub fn changed(&self) -> Result<Vec<PathBuf>> {
        if self.snapshots.is_empty() {
            return Ok(vec![]);
        }

        let mut files = std::collections::BTreeSet::new();

        for (_, snap) in &self.snapshots {
            for path in snap.affected() {
                files.insert(path);
            }
        }

        Ok(files.into_iter().collect())
    }

    /// Returns the original content of a file from the first snapshot where it appears.
    /// If the file was created in the first snapshot, returns an empty string.
    /// If the file does not occur in any snapshot, returns None.
    pub fn original(&self, path: &Path) -> Option<String> {
        if self.snapshots.is_empty() {
            return None;
        }

        // Sort snapshots by ID to find the first one
        let mut sorted_snapshots: Vec<&(u64, Snapshot)> = self.snapshots.iter().collect();
        sorted_snapshots.sort_by_key(|(id, _)| *id);

        for (_, snap) in sorted_snapshots {
            // First check if there's content for this file
            if let Some(content) = snap.content.get(path) {
                return Some(content.clone());
            }
            // If there's no content but file is in created list, it was a new empty file
            else if snap.created.contains(&path.to_path_buf()) {
                return Some(String::new());
            }
        }

        None
    }

    /// Matches files in both the memory and directory stores based on the provided patterns.
    /// The patterns are normalized using the substore's root (empty for memory) and the given current
    /// working directory, and matched using globset.
    pub fn find<T>(&self, cwd: T, patterns: Vec<String>) -> Result<Vec<PathBuf>>
    where
        T: abspath::IntoAbsPath,
    {
        let cwd = cwd.into_abs_path()?;
        let mut results = HashSet::new();

        // First, handle memory store with path cleaning
        let mem_files = self.memory.list()?;
        for pattern in &patterns {
            let cleaned = path_clean::clean(pattern);
            let pattern_str = cleaned.to_str().ok_or_else(|| {
                Error::Internal("Failed to convert cleaned path to string".to_string())
            })?;
            let glob = Glob::new(pattern_str).map_err(|e| Error::Path(e.to_string()))?;
            let matcher = glob.compile_matcher();
            for file in &mem_files {
                if matcher.is_match(file) {
                    results.insert(file.clone());
                }
            }
        }

        // Then handle directory store with path normalization for non-memory patterns
        if let Some(ref dir) = self.directory {
            let dir_files = dir.list()?;
            for pattern in &patterns {
                if pattern.starts_with(MEM_PREFIX) {
                    continue;
                }
                let normalized = files::normalize_path(dir.root.clone(), cwd.clone(), pattern)?;
                let pattern_str = normalized.to_str().ok_or_else(|| {
                    Error::Internal("Failed to convert normalized path to string".to_string())
                })?;
                let glob = Glob::new(pattern_str).map_err(|e| Error::Path(e.to_string()))?;
                let matcher = glob.compile_matcher();
                for file in &dir_files {
                    if matcher.is_match(file) {
                        results.insert(file.clone());
                    }
                }
            }
        }

        let mut result_vec: Vec<_> = results.into_iter().collect();
        result_vec.sort();
        Ok(result_vec)
    }

    /// Creates and dispatches a touch patch for files matching the provided patterns. Expands the
    /// patterns using the current working directory, creates a `Operatiion::View` for each matched
    /// path, and applies the patch. Returns a tuple of (snapshot ID, file count) from applying the
    /// patch.
    pub fn view<P>(&mut self, cwd: P, patterns: Vec<String>) -> Result<(u64, usize)>
    where
        P: abspath::IntoAbsPath,
    {
        let paths = self.find(cwd, patterns)?;
        let file_count = paths.len();
        let changes: Vec<Operation> = paths.into_iter().map(patch::Operation::View).collect();
        let patch = Patch { ops: changes };
        let patch_info = self.patch(&patch)?;
        // Failures for touch changes should always be empty.
        debug_assert!(patch_info.failures.is_empty());
        Ok((patch_info.rollback_id, file_count))
    }

    /// Add an empty patch to the snapshot sequence and return a snapshot ID. Useful as a markder.
    pub fn mark(&mut self) -> Result<u64> {
        let patch = Patch { ops: vec![] };
        let patch_info = self.patch(&patch)?;
        // Failures for mark changes should always be empty.
        debug_assert!(patch_info.failures.is_empty());
        Ok(patch_info.rollback_id)
    }

    /// Checks if the state has been modified since the given rollback ID.
    /// Returns true if any modifications have been made since that ID, false otherwise.
    /// View operations are not considered modifications.
    pub fn was_modified_since(&self, rollback_id: u64) -> bool {
        // Check if any modification IDs exist after the given rollback_id
        self.modification_ids.iter().any(|&id| id > rollback_id)
    }
}

#[cfg(test)]
mod tests {
    use super::abspath::AbsPath;
    use super::*;
    // test imports are used below
    use crate::patch::Operation;
    use std::{collections::HashMap, path::PathBuf};
    use tempfile::TempDir;

    /// Function type used for making assertions on the state
    type StateAssertionFn = Box<dyn Fn(&State)>;

    struct StateTestCase {
        name: &'static str,
        patches: Vec<Patch>,
        initial_content: HashMap<PathBuf, String>,
        expected_final_content: Vec<(PathBuf, String)>,
        expect_patch_failure: Option<String>,
        state_assertion: Option<StateAssertionFn>,
    }

    impl StateTestCase {
        /// Create a new test case with a name and set of patches to apply
        pub fn new<S>(name: S, patches: Vec<Patch>) -> Self
        where
            S: Into<&'static str>,
        {
            Self {
                name: name.into(),
                patches,
                initial_content: HashMap::new(),
                expected_final_content: Vec::new(),
                expect_patch_failure: None,
                state_assertion: None,
            }
        }

        pub fn expect_patch_failure<S>(mut self, msg: S) -> Self
        where
            S: Into<String>,
        {
            self.expect_patch_failure = Some(msg.into());
            self
        }

        /// Add initial content for a file
        pub fn with_content<P, C>(mut self, path: P, content: C) -> Self
        where
            P: AsRef<Path>,
            C: Into<String>,
        {
            self.initial_content
                .insert(path.as_ref().to_path_buf(), content.into());
            self
        }

        /// Add expected final content for a file
        pub fn expect_content<P, C>(mut self, path: P, content: C) -> Self
        where
            P: AsRef<Path>,
            C: Into<String>,
        {
            self.expected_final_content
                .push((path.as_ref().to_path_buf(), content.into()));
            self
        }

        /// Add a closure to perform custom assertions on the final state
        pub fn expect_state<F>(mut self, f: F) -> Self
        where
            F: Fn(&State) + 'static,
        {
            self.state_assertion = Some(Box::new(f));
            self
        }
    }

    /// A testing framework for state operations
    struct StateTest {
        state: State,
        _temp_dir: Option<TempDir>,
    }

    impl StateTest {
        /// Create a new state test framework backed by a temporary directory
        fn new() -> Result<Self> {
            let temp_dir = TempDir::new().expect("failed to create temporary directory");
            let root = temp_dir.path().to_path_buf();
            let state =
                State::default().with_directory(AbsPath::new(root)?, vec!["*".to_string()])?;

            Ok(Self {
                state,
                _temp_dir: Some(temp_dir),
            })
        }

        /// Write directly to a file in the state to initialize test data
        fn write<P, C>(&mut self, path: P, content: C) -> Result<()>
        where
            P: AsRef<Path>,
            C: AsRef<str>,
        {
            self.state.write(path.as_ref(), content.as_ref())
        }

        /// Read content from a file in the state
        fn read<P>(&self, path: P) -> Result<String>
        where
            P: AsRef<Path>,
        {
            self.state.read(path.as_ref())
        }

        /// Assert that a file contains the expected content
        fn assert_content<P, E>(&self, path: P, expected: E, test_name: &str) -> Result<()>
        where
            P: AsRef<Path>,
            E: AsRef<str>,
        {
            let content = self.read(&path)?;
            assert_eq!(
                content,
                expected.as_ref(),
                "[{}] Content mismatch for {}",
                test_name,
                path.as_ref().display()
            );
            Ok(())
        }

        /// Run a table test with a StateTestCase containing patches and expected file states
        fn run_test(&mut self, test_case: StateTestCase) -> Result<()> {
            // Initialize with the test case's initial content
            for (path, content) in &test_case.initial_content {
                self.write(path, content)?;
            }

            // Apply all patches
            let mut patchinfos = vec![];
            for patch in test_case.patches {
                patchinfos.push(self.state.patch(&patch)?);
            }

            if test_case.expect_patch_failure.is_none() {
                // Verify that all patches succeeded
                for info in &patchinfos {
                    assert_eq!(
                        info.failures.len(),
                        0,
                        "[{}] Patch application had failures: {:?}",
                        test_case.name,
                        info.failures
                    );
                }
            }

            // Verify expected content
            for (path, expected_content) in test_case.expected_final_content {
                self.assert_content(&path, &expected_content, test_case.name)?;
            }

            if let Some(msg) = test_case.expect_patch_failure {
                let info = patchinfos.last().expect("No patch info found");
                assert_eq!(
                    info.failures.len(),
                    1,
                    "[{}] Expected 1 patch failure but got {}",
                    test_case.name,
                    info.failures.len()
                );
                assert! {
                    info.failures[0].user.to_lowercase().contains(&msg.to_lowercase()),
                    "[{}] Expected patch failure message to contain '{}', got: {}",
                    test_case.name,
                    msg,
                    info.failures[0].user
                }
            }

            // Run custom state assertions if provided
            if let Some(assertion) = &test_case.state_assertion {
                // Use catch_unwind to catch any assertion panics
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    assertion(&self.state);
                }));

                // If there was a panic, re-emit it with the test name
                if let Err(panic_info) = result {
                    if let Some(msg) = panic_info.downcast_ref::<String>() {
                        panic!("[{}] {}", test_case.name, msg);
                    } else if let Some(msg) = panic_info.downcast_ref::<&str>() {
                        panic!("[{}] {}", test_case.name, msg);
                    } else {
                        panic!(
                            "[{}] Assertion failed with unknown panic info",
                            test_case.name
                        );
                    }
                }
            }

            Ok(())
        }

        /// Run multiple test cases in sequence
        fn run_tests(test_cases: Vec<StateTestCase>) {
            for test_case in test_cases {
                let mut test = Self::new().unwrap();
                test.run_test(test_case).unwrap()
            }
        }
    }

    #[test]
    fn test_basic_state_operations() {
        let fs_file = "test.txt";
        let mem_file = "::mem.txt";

        let test_cases = vec![StateTestCase::new(
            "Basic filesystem and memory operations",
            vec![
                Patch::default().with_write(fs_file, "filesystem content"),
                Patch::default().with_write(mem_file, "memory content"),
                Patch::default().with_write(fs_file, "updated filesystem content"),
                Patch::default().with_write(mem_file, "updated memory content"),
            ],
        )
        .expect_content(fs_file, "updated filesystem content")
        .expect_content(mem_file, "updated memory content")];

        StateTest::run_tests(test_cases);
    }

    #[test]
    fn test_last_changed_between() {
        // Helper function to create an assertion function for last_changed_between
        fn assert_changed_between(
            start: Option<u64>,
            end: Option<u64>,
            expected_paths: Vec<&'static str>,
        ) -> StateAssertionFn {
            Box::new(move |state| {
                let result = state.last_changed_between(start, end).unwrap();
                let paths: Vec<&str> = result.iter().map(|p| p.to_str().unwrap()).collect();
                assert_eq!(paths, expected_paths);
            })
        }

        let test_cases = vec![
            StateTestCase::new("empty snapshots list", vec![])
                .expect_state(assert_changed_between(None, None, vec![])),
            StateTestCase::new(
                "single snapshot",
                vec![Patch::default()
                    .with_write("::a.txt", "A0")
                    .with_write("::b.txt", "B0")],
            )
            .expect_state(assert_changed_between(
                Some(0),
                Some(0),
                vec!["::a.txt", "::b.txt"],
            )),
            StateTestCase::new(
                "overlapping changes in range",
                vec![
                    Patch::default()
                        .with_write("::a.txt", "A0")
                        .with_write("::b.txt", "B0"),
                    Patch::default().with_write("::b.txt", "B1"),
                ],
            )
            .expect_state(assert_changed_between(Some(0), Some(0), vec!["::a.txt"])),
            StateTestCase::new(
                "full range with implicit boundaries",
                vec![
                    Patch::default().with_write("::a.txt", "A0"),
                    Patch::default().with_write("::b.txt", "B0"),
                    Patch::default().with_write("::c.txt", "C0"),
                ],
            )
            .expect_state(assert_changed_between(
                None,
                None,
                vec!["::a.txt", "::b.txt", "::c.txt"],
            )),
            StateTestCase::new(
                "middle range",
                vec![
                    Patch::default().with_write("::a.txt", "A0"),
                    Patch::default().with_write("::b.txt", "B0"),
                    Patch::default().with_write("::c.txt", "C0"),
                ],
            )
            .expect_state(assert_changed_between(Some(1), Some(1), vec!["::b.txt"])),
            StateTestCase::new(
                "changes outside range excluded",
                vec![
                    Patch::default().with_write("::a.txt", "A0"),
                    Patch::default().with_write("::b.txt", "B0"),
                    Patch::default().with_write("::a.txt", "A1"),
                ],
            )
            .expect_state(assert_changed_between(Some(1), Some(1), vec!["::b.txt"])),
            StateTestCase::new(
                "multiple files in multiple snapshots",
                vec![
                    Patch::default()
                        .with_write("::a.txt", "A0")
                        .with_write("::b.txt", "B0"),
                    Patch::default()
                        .with_write("::c.txt", "C0")
                        .with_write("::d.txt", "D0"),
                    Patch::default()
                        .with_write("::b.txt", "B1")
                        .with_write("::d.txt", "D1"),
                ],
            )
            .expect_state(assert_changed_between(
                Some(0),
                Some(1),
                vec!["::a.txt", "::c.txt"],
            )),
        ];

        StateTest::run_tests(test_cases);
    }

    /// Unit test for multiple snapshot layers.
    #[test]
    fn test_multiple_snapshot_layers() -> Result<()> {
        let mut state = State::default();

        let key_a = "::a.txt";
        let key_x = "::x.txt";
        state.dispatch_mut(Path::new(key_a), |store| {
            store.write(Path::new(key_a), "A0")
        })?;
        state.dispatch_mut(Path::new(key_x), |store| {
            store.write(Path::new(key_x), "X0")
        })?;

        let paths = vec![PathBuf::from(key_a), PathBuf::from(key_x)];
        let snap_id0 = state.snapshot(&paths)?;
        assert_eq!(snap_id0, 0);

        state.write(Path::new(key_a), "A1")?;
        state.write(Path::new(key_x), "X1")?;

        let snap_id1 = state.snapshot(&paths)?;
        assert_eq!(snap_id1, 1);

        state.write(Path::new(key_a), "A2")?;
        state.write(Path::new(key_x), "X2")?;

        assert_eq!(state.read(Path::new(key_a))?, "A2");
        assert_eq!(state.read(Path::new(key_x))?, "X2");

        state.revert(1)?;

        assert_eq!(state.read(Path::new(key_a))?, "A0");
        assert_eq!(state.read(Path::new(key_x))?, "X0");

        Ok(())
    }

    #[test]
    fn test_find() {
        // Helper function to create an assertion function for find
        fn assert_find_results(
            patterns: Vec<&'static str>,
            expected_paths: Vec<&'static str>,
        ) -> StateAssertionFn {
            Box::new(move |state| {
                let cwd = AbsPath::new(std::path::PathBuf::from("/")).unwrap();
                let pattern_strings: Vec<String> = patterns.iter().map(|s| s.to_string()).collect();
                let result = state.find(cwd, pattern_strings).unwrap();
                let result_strs: Vec<&str> = result.iter().map(|p| p.to_str().unwrap()).collect();
                assert_eq!(result_strs, expected_paths);
            })
        }

        let test_cases = vec![
            StateTestCase::new("memory only - exact match", vec![])
                .with_content("::foo.txt", "foo")
                .with_content("::bar.txt", "bar")
                .expect_state(assert_find_results(vec!["::foo.txt"], vec!["::foo.txt"])),
            StateTestCase::new("memory only - dupes", vec![])
                .with_content("::foo.txt", "foo")
                .with_content("::bar.txt", "bar")
                .expect_state(assert_find_results(
                    vec!["::foo.txt", "::foo.txt"],
                    vec!["::foo.txt"],
                )),
            StateTestCase::new("memory only - glob match", vec![])
                .with_content("::foo.txt", "foo")
                .with_content("::bar.txt", "bar")
                .expect_state(assert_find_results(
                    vec!["::*.txt"],
                    vec!["::bar.txt", "::foo.txt"],
                )),
            // Note: filesystem cases can't be easily tested with the StateTestCase framework
            // as it requires creating real files in a temporary directory
            StateTestCase::new("no matches", vec![])
                .with_content("::foo.txt", "foo")
                .expect_state(assert_find_results(vec!["::nonexistent.txt"], vec![])),
            StateTestCase::new("multiple patterns", vec![])
                .with_content("::foo.txt", "foo")
                .with_content("::bar.rs", "bar")
                .expect_state(assert_find_results(
                    vec!["::*.txt", "::*.rs"],
                    vec!["::bar.rs", "::foo.txt"],
                )),
        ];

        StateTest::run_tests(test_cases);
    }

    #[test]
    fn test_original() {
        // Helper function to create an assertion function for original()
        fn assert_original(path: &'static str, expected: Option<&'static str>) -> StateAssertionFn {
            Box::new(move |state| {
                let result = state.original(Path::new(path));

                match (result, expected) {
                    (Some(got), Some(expected)) => {
                        assert_eq!(got, expected, "Original content mismatch for {path}");
                    }
                    (None, None) => {
                        // Both are None, that's correct
                    }
                    (got, expected) => {
                        panic!("For {path}: got {got:?}, expected {expected:?}");
                    }
                }
            })
        }

        let test_cases = vec![
            StateTestCase::new("empty snapshots list", vec![])
                .expect_state(assert_original("::nonexistent.txt", None)),
            StateTestCase::new(
                "newly created file in patch",
                vec![
                    Patch::default().with_write("::a.txt", "A0"),
                    Patch::default().with_write("::a.txt", "A1"),
                ],
            )
            .expect_state(assert_original("::a.txt", Some(""))),
            StateTestCase::new(
                "file with initial content modified in patches",
                vec![
                    Patch::default().with_write("::a.txt", "A0"),
                    Patch::default().with_write("::a.txt", "A1"),
                ],
            )
            .with_content("::a.txt", "Original")
            .expect_state(assert_original("::a.txt", Some("Original"))),
            StateTestCase::new(
                "file in second snapshot only",
                vec![
                    Patch::default().with_write("::a.txt", "A0"),
                    Patch::default().with_write("::b.txt", "B0"),
                ],
            )
            .expect_state(assert_original("::b.txt", Some(""))),
            StateTestCase::new(
                "file created in first snapshot",
                vec![Patch::default().with_view("::created.txt")],
            )
            .expect_state(assert_original("::created.txt", Some(""))),
            StateTestCase::new(
                "file not in any snapshot",
                vec![Patch::default().with_write("::a.txt", "A1")],
            )
            .with_content("::a.txt", "A0")
            .expect_state(assert_original("::nonexistent.txt", None)),
        ];

        StateTest::run_tests(test_cases);
    }

    #[test]
    fn test_changed() {
        // Helper function to create an assertion function for changed()
        fn assert_changed(expected_paths: Vec<&'static str>) -> StateAssertionFn {
            Box::new(move |state| {
                let result = state.changed().unwrap();
                let paths: Vec<&str> = result.iter().map(|p| p.to_str().unwrap()).collect();
                assert_eq!(paths, expected_paths);
            })
        }

        let test_cases = vec![
            StateTestCase::new("empty snapshots list", vec![]).expect_state(assert_changed(vec![])),
            StateTestCase::new(
                "single snapshot with multiple files",
                vec![Patch::default()
                    .with_write("::a.txt", "A0")
                    .with_write("::b.txt", "B0")],
            )
            .expect_state(assert_changed(vec!["::a.txt", "::b.txt"])),
            StateTestCase::new(
                "multiple snapshots with unique files",
                vec![
                    Patch::default().with_write("::a.txt", "A0"),
                    Patch::default().with_write("::b.txt", "B0"),
                    Patch::default().with_write("::c.txt", "C0"),
                ],
            )
            .expect_state(assert_changed(vec!["::a.txt", "::b.txt", "::c.txt"])),
            StateTestCase::new(
                "multiple snapshots with overlapping files",
                vec![
                    Patch::default().with_write("::a.txt", "A0"),
                    Patch::default().with_write("::b.txt", "B0"),
                    Patch::default().with_write("::a.txt", "A1"),
                ],
            )
            .expect_state(assert_changed(vec!["::a.txt", "::b.txt"])),
            StateTestCase::new(
                "multiple snapshots with multiple files per snapshot",
                vec![
                    Patch::default()
                        .with_write("::a.txt", "A0")
                        .with_write("::b.txt", "B0"),
                    Patch::default()
                        .with_write("::c.txt", "C0")
                        .with_write("::d.txt", "D0"),
                    Patch::default()
                        .with_write("::b.txt", "B1")
                        .with_write("::d.txt", "D1"),
                ],
            )
            .expect_state(assert_changed(vec![
                "::a.txt", "::b.txt", "::c.txt", "::d.txt",
            ])),
            StateTestCase::new(
                "view changes included",
                vec![
                    Patch::default().with_view("::view1.txt"),
                    Patch::default()
                        .with_view("::view2.txt")
                        .with_write("::a.txt", "A0"),
                ],
            )
            .expect_state(assert_changed(vec![
                "::a.txt",
                "::view1.txt",
                "::view2.txt",
            ])),
        ];

        StateTest::run_tests(test_cases);
    }

    #[test]
    fn test_last_original() {
        // Helper function to create an assertion function for last_original()
        fn assert_last_original(
            path: &'static str,
            expected: Option<&'static str>,
        ) -> StateAssertionFn {
            Box::new(move |state| {
                let result = state.last_original(Path::new(path));

                match (result, expected) {
                    (Some(got), Some(expected)) => {
                        assert_eq!(got, expected, "Last original content mismatch for {path}");
                    }
                    (None, None) => {
                        // Both are None, that's correct
                    }
                    (got, expected) => {
                        panic!("For {path}: got {got:?}, expected {expected:?}");
                    }
                }
            })
        }

        let test_cases = vec![
            StateTestCase::new("empty snapshots list", vec![])
                .expect_state(assert_last_original("::test.txt", None)),
            StateTestCase::new(
                "single snapshot with file",
                vec![Patch::default().with_write("::test.txt", "Modified")],
            )
            .with_content("::test.txt", "Original")
            .expect_state(assert_last_original("::test.txt", Some("Original"))),
            StateTestCase::new(
                "multiple snapshots with file modifications",
                vec![
                    Patch::default().with_write("::test.txt", "Version 1"),
                    Patch::default().with_write("::test.txt", "Version 2"),
                ],
            )
            .with_content("::test.txt", "Original")
            .expect_state(assert_last_original("::test.txt", Some("Version 1"))),
            StateTestCase::new(
                "file not modified in second snapshot",
                vec![
                    Patch::default()
                        .with_write("::a.txt", "A-Version 1")
                        .with_write("::b.txt", "B-Version 1"),
                    Patch::default().with_write("::a.txt", "A-Version 2"), // b.txt not modified
                ],
            )
            .with_content("::a.txt", "A-Original")
            .with_content("::b.txt", "B-Original")
            .expect_state(assert_last_original("::b.txt", Some("B-Original"))),
            StateTestCase::new(
                "file created in second snapshot",
                vec![
                    Patch::default().with_write("::a.txt", "A-Version 1"),
                    Patch::default().with_write("::b.txt", "B-Version 1"), // New file
                ],
            )
            .expect_state(assert_last_original("::b.txt", Some(""))),
            StateTestCase::new(
                "file not in any snapshot",
                vec![Patch::default().with_write("::a.txt", "A-Version 1")],
            )
            .with_content("::a.txt", "A-Original")
            .expect_state(assert_last_original("::nonexistent.txt", None)),
        ];

        StateTest::run_tests(test_cases);
    }

    #[test]
    fn test_undo() {
        let p = "::test.txt";

        let test_cases = vec![
            StateTestCase::new("Nonexistent", vec![Patch::default().with_undo(p)])
                .expect_patch_failure("No previous version"),
            StateTestCase::new(
                "Undo a single change",
                vec![
                    Patch::default().with_write(p, "Modified content"),
                    Patch::default().with_undo(p),
                ],
            )
            .with_content(p, "Original content")
            .expect_content(p, "Original content"),
            StateTestCase::new(
                "Multiple changes and undo",
                vec![
                    Patch::default().with_write(p, "First modification"),
                    Patch::default().with_write(p, "Second modification"),
                    Patch::default().with_undo(p),
                ],
            )
            .with_content(p, "Original content")
            .expect_content(p, "First modification"),
            StateTestCase::new(
                "Double undo to return to original",
                vec![
                    Patch::default().with_write(p, "First modification"),
                    Patch::default().with_write(p, "Second modification"),
                    Patch::default().with_undo(p),
                    Patch::default().with_undo(p),
                ],
            )
            .with_content(p, "Original content")
            .expect_content(p, "Second modification"),
        ];

        StateTest::run_tests(test_cases);
    }

    #[test]
    fn test_insert() {
        let p = "::test.txt";

        let test_cases = vec![
            StateTestCase::new(
                "Insert at beginning",
                vec![Patch::default().with_insert(p, 0, "Inserted line\n")],
            )
            .with_content(p, "Line 1\nLine 2\nLine 3")
            .expect_content(p, "Inserted line\nLine 1\nLine 2\nLine 3"),
            StateTestCase::new(
                "Insert in middle",
                vec![Patch::default().with_insert(p, 1, "Inserted line\n")],
            )
            .with_content(p, "Line 1\nLine 2\nLine 3")
            .expect_content(p, "Line 1\nInserted line\nLine 2\nLine 3"),
            StateTestCase::new(
                "Insert at end",
                vec![Patch::default().with_insert(p, 3, "Inserted line\n")],
            )
            .with_content(p, "Line 1\nLine 2\nLine 3")
            .expect_content(p, "Line 1\nLine 2\nLine 3\nInserted line\n"),
            StateTestCase::new(
                "Insert into empty file",
                vec![Patch::default().with_insert(p, 0, "First line\n")],
            )
            .with_content(p, "")
            .expect_content(p, "First line\n"),
            StateTestCase::new(
                "Insert past end of file",
                vec![Patch::default().with_insert(p, 10, "New line\n")],
            )
            .with_content(p, "Line 1\nLine 2")
            .expect_patch_failure("out of bounds"),
            StateTestCase::new(
                "Multiple inserts",
                vec![
                    Patch::default().with_insert(p, 0, "First insert\n"),
                    Patch::default().with_insert(p, 1, "Second insert\n"),
                ],
            )
            .with_content(p, "Original line")
            .expect_content(p, "First insert\nSecond insert\nOriginal line"),
            StateTestCase::new(
                "Insert with no newline",
                vec![Patch::default().with_insert(p, 1, "Inserted")],
            )
            .with_content(p, "Line 1\nLine 2")
            .expect_content(p, "Line 1\nInsertedLine 2"),
        ];

        StateTest::run_tests(test_cases);
    }

    #[test]
    fn test_was_modified_since() {
        let mut state = State::default();

        // Initialize with some test files
        let mut initial_files = HashMap::new();
        initial_files.insert(PathBuf::from("::test1.txt"), "Initial content".to_string());
        initial_files.insert(
            PathBuf::from("::test2.txt"),
            "Initial content 2".to_string(),
        );
        state = state.with_memory(initial_files).unwrap();

        // Take initial snapshot
        let initial_id = state.mark().unwrap();
        assert_eq!(initial_id, 0);

        // No modifications yet
        assert!(!state.was_modified_since(initial_id));

        // Apply a View operation - should NOT count as modification
        let view_patch = Patch::default().with_view("::test1.txt");
        let view_info = state.patch(&view_patch).unwrap();
        assert!(!state.was_modified_since(initial_id));
        assert!(!state.was_modified_since(view_info.rollback_id));

        // Apply a ViewRange operation - should NOT count as modification
        let view_range_patch = Patch::default().with_view_range("::test1.txt", 0, Some(10));
        let view_range_info = state.patch(&view_range_patch).unwrap();
        assert!(!state.was_modified_since(initial_id));
        assert!(!state.was_modified_since(view_range_info.rollback_id));

        // Apply a Write operation - SHOULD count as modification
        let write_patch = Patch::default().with_write("::test1.txt", "Modified content");
        let write_info = state.patch(&write_patch).unwrap();
        assert!(state.was_modified_since(initial_id));
        assert!(state.was_modified_since(view_info.rollback_id));
        assert!(state.was_modified_since(view_range_info.rollback_id));
        assert!(!state.was_modified_since(write_info.rollback_id));

        // Mark a checkpoint
        let checkpoint_id = state.mark().unwrap();
        assert!(!state.was_modified_since(checkpoint_id));

        // Apply a Replace operation - SHOULD count as modification
        let replace_patch = Patch::default().with_replace("::test2.txt", "Initial", "Modified");
        let replace_info = state.patch(&replace_patch).unwrap();
        assert!(state.was_modified_since(checkpoint_id));
        assert!(!state.was_modified_since(replace_info.rollback_id));

        // Apply an Insert operation - SHOULD count as modification
        let insert_patch = Patch::default().with_insert("::test1.txt", 0, "Inserted line\n");
        let insert_info = state.patch(&insert_patch).unwrap();
        assert!(state.was_modified_since(checkpoint_id));
        assert!(state.was_modified_since(replace_info.rollback_id));
        assert!(!state.was_modified_since(insert_info.rollback_id));

        // Apply an Undo operation - SHOULD count as modification
        let undo_patch = Patch::default().with_undo("::test1.txt");
        let undo_info = state.patch(&undo_patch).unwrap();
        assert!(state.was_modified_since(insert_info.rollback_id));
        assert!(!state.was_modified_since(undo_info.rollback_id));

        // Test with file creation
        let create_patch = Patch::default().with_write("::new_file.txt", "New content");
        let create_info = state.patch(&create_patch).unwrap();
        assert!(state.was_modified_since(undo_info.rollback_id));
        assert!(!state.was_modified_since(create_info.rollback_id));

        // Test mixed patch with both View and Write operations
        let mixed_patch = Patch::default()
            .with_view("::test1.txt")
            .with_write("::test2.txt", "Another modification");
        let mixed_info = state.patch(&mixed_patch).unwrap();
        assert!(state.was_modified_since(create_info.rollback_id));
        assert!(!state.was_modified_since(mixed_info.rollback_id));
    }

    #[test]
    fn test_was_modified_since_edge_cases() {
        let mut state = State::default();

        // Edge case: Empty state
        assert!(!state.was_modified_since(0));
        assert!(!state.was_modified_since(100));

        // Initialize with a test file
        let mut initial_files = HashMap::new();
        initial_files.insert(PathBuf::from("::test.txt"), "Initial".to_string());
        state = state.with_memory(initial_files).unwrap();

        // Edge case: Rollback ID doesn't exist yet
        assert!(!state.was_modified_since(999));

        // Apply a modification
        let write_patch = Patch::default().with_write("::test.txt", "Modified");
        let write_info = state.patch(&write_patch).unwrap();

        // Check that it's not modified since a future ID
        assert!(!state.was_modified_since(999)); // Future ID that doesn't exist

        // Revert the modification
        state.revert(write_info.rollback_id).unwrap();

        // After revert, no modifications should be tracked
        assert!(!state.was_modified_since(0));

        // Apply multiple operations in one patch
        let multi_patch = Patch::default()
            .with_view("::test.txt")
            .with_view_range("::test.txt", 0, Some(10))
            .with_write("::test2.txt", "New file");
        let multi_info = state.patch(&multi_patch).unwrap();

        // Should be considered modified because of the write operation
        assert!(state.was_modified_since(0));

        // Edge case: Check modification since the same ID
        assert!(!state.was_modified_since(multi_info.rollback_id));

        // Edge case: Empty patch (mark operation)
        let mark_id = state.mark().unwrap();
        assert!(!state.was_modified_since(multi_info.rollback_id));
        assert!(!state.was_modified_since(mark_id));
    }

    #[test]
    fn test_diff_path() {
        // Test diff_path directly without relying on the StateTest framework
        let mut state = State::default();

        // Initialize memory store with some files
        let mut initial_files = HashMap::new();

        // Test case 1: Small change (single line)
        let path1 = PathBuf::from("::test1.txt");
        initial_files.insert(path1.clone(), "Hello world".to_string());

        // Test case 2: Multiple small changes
        let path2 = PathBuf::from("::test2.txt");
        initial_files.insert(
            path2.clone(),
            "Line 1\nLine 2\nLine 3\nLine 4\n".to_string(),
        );

        // Test case 3: Majority of lines changed
        let path3 = PathBuf::from("::test3.txt");
        initial_files.insert(
            path3.clone(),
            "Line 1\nLine 2\nLine 3\nLine 4\n".to_string(),
        );

        // Test case 4: Completely different content
        let path4 = PathBuf::from("::test4.txt");
        initial_files.insert(path4.clone(), "Original content".to_string());

        // Test case 5: Empty to non-empty
        let path5 = PathBuf::from("::test5.txt");
        initial_files.insert(path5.clone(), "".to_string());

        // Test case 6: Non-empty to empty
        let path6 = PathBuf::from("::test6.txt");
        initial_files.insert(path6.clone(), "Content to be removed".to_string());

        // Initialize the state with these files
        state = state.with_memory(initial_files).unwrap();

        // Take snapshot to record the original state
        state
            .snapshot(&[
                path1.clone(),
                path2.clone(),
                path3.clone(),
                path4.clone(),
                path5.clone(),
                path6.clone(),
            ])
            .unwrap();

        // Make changes to the files
        state.write(&path1, "Hello there").unwrap();
        state
            .write(&path2, "Line 1\nLine 2 modified\nLine 3\nLine 4 modified\n")
            .unwrap();
        state
            .write(
                &path3,
                "Line 1 changed\nLine 2 changed\nLine 3 changed\nLine 4\n",
            )
            .unwrap();
        state.write(&path4, "Totally different content").unwrap();
        state.write(&path5, "New content added").unwrap();
        state.write(&path6, "").unwrap();

        // Test case 1: Small change should generate a Replace operation
        let patch1 = state.diff_path(&path1).unwrap();
        assert!(!patch1.ops.is_empty());
        match &patch1.ops[0] {
            Operation::Write(w) => {
                assert_eq!(w.path, path1);
                assert_eq!(w.content, "Hello there");
            }
            _ => panic!("Expected Write operation for test1.txt"),
        }

        // Test case 2: Multiple small changes should generate Replace operations
        let patch2 = state.diff_path(&path2).unwrap();
        assert!(!patch2.ops.is_empty());
        match &patch2.ops[0] {
            Operation::Write(w) => {
                assert_eq!(w.path, path2);
                assert_eq!(
                    w.content,
                    "Line 1\nLine 2 modified\nLine 3\nLine 4 modified\n"
                );
            }
            _ => panic!("Expected Write operation for test2.txt"),
        }

        // Test case 3: Major changes should generate a Write operation
        let patch3 = state.diff_path(&path3).unwrap();
        assert!(!patch3.ops.is_empty());
        match &patch3.ops[0] {
            Operation::Write(w) => {
                assert_eq!(w.path, path3);
                assert_eq!(
                    w.content,
                    "Line 1 changed\nLine 2 changed\nLine 3 changed\nLine 4\n"
                );
            }
            _ => panic!("Expected Write operation for test3.txt"),
        }

        // Test case 4: Completely different content should generate a Write operation
        let patch4 = state.diff_path(&path4).unwrap();
        assert!(!patch4.ops.is_empty());
        match &patch4.ops[0] {
            Operation::Write(w) => {
                assert_eq!(w.path, path4);
                assert_eq!(w.content, "Totally different content");
            }
            _ => panic!("Expected Write operation for test4.txt"),
        }

        // Test case 5: Empty to non-empty should generate a Write operation
        let patch5 = state.diff_path(&path5).unwrap();
        assert!(!patch5.ops.is_empty());
        match &patch5.ops[0] {
            Operation::Write(w) => {
                assert_eq!(w.path, path5);
                assert_eq!(w.content, "New content added");
            }
            _ => panic!("Expected Write operation for test5.txt"),
        }

        // Test case 6: Non-empty to empty should generate a Write operation
        let patch6 = state.diff_path(&path6).unwrap();
        assert!(!patch6.ops.is_empty());
        match &patch6.ops[0] {
            Operation::Write(w) => {
                assert_eq!(w.path, path6);
                assert_eq!(w.content, "");
            }
            _ => panic!("Expected Write operation for test6.txt"),
        }

        // Verify patches can be applied
        for (i, patch) in [patch1, patch2, patch3, patch4, patch5, patch6]
            .iter()
            .enumerate()
        {
            let mut test_state = State::default();
            let path = PathBuf::from(format!("::test{}.txt", i + 1));
            let original = state.original(&path).unwrap_or_default();

            let mut initial_files = HashMap::new();
            initial_files.insert(path.clone(), original);
            test_state = test_state.with_memory(initial_files).unwrap();

            let patch_info = test_state.patch(patch).unwrap();
            assert!(
                patch_info.failures.is_empty(),
                "Failed to apply patch for test{}.txt",
                i + 1
            );

            let expected = state.read(&path).unwrap();
            let actual = test_state.read(&path).unwrap();
            assert_eq!(
                actual,
                expected,
                "Patched content doesn't match for test{}.txt",
                i + 1
            );
        }
    }
}
