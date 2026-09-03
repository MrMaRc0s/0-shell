//! Test-only scratch directories for the filesystem commands.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// A unique directory that removes itself and its contents when dropped.
///
/// Tests use absolute paths inside the directory rather than changing the
/// working directory, because the working directory is process-wide and the
/// test harness runs tests in parallel.
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// Creates a scratch directory named after the calling test.
    pub fn new(label: &str) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);

        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "zero_shell_{}_{}_{unique}",
            std::process::id(),
            label
        ));

        fs::remove_dir_all(&path).ok();
        fs::create_dir_all(&path).expect("failed to create the test directory");

        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// An absolute path inside the directory, as commands receive it.
    pub fn arg(&self, name: &str) -> String {
        self.path
            .join(name)
            .to_str()
            .expect("test paths are valid UTF-8")
            .to_string()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.path).ok();
    }
}
