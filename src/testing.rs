//! Test-only scratch directories and working-directory custody.
//!
//! Shared by the whole crate: the filesystem commands take their scratch
//! space from here, and the end-to-end shell tests take the working directory.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

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
        Self::new_in(std::env::temp_dir(), label)
    }

    /// Creates the scratch directory under a chosen root.
    ///
    /// `mv` needs this: its cross-device fallback only runs when the source and
    /// the target sit on different filesystems, which means putting one of them
    /// somewhere other than the temporary directory everything else uses.
    pub fn new_in<P>(root: P, label: &str) -> Self
    where
        P: AsRef<Path>,
    {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);

        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = root.as_ref().join(format!(
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

/// The one lock behind every test that touches the working directory.
static CURRENT_DIRECTORY: Mutex<()> = Mutex::new(());

/// Exclusive use of the process-wide working directory, restored on drop.
///
/// The working directory belongs to the process rather than to the test, and
/// the harness runs tests in parallel, so a test that reads or changes it has
/// to hold this lock or watch another test move the ground under it. Most
/// filesystem tests avoid the problem entirely by naming absolute paths inside
/// a [`TempDir`]; this exists for the ones that cannot, which are the `cd` and
/// `pwd` tests and the end-to-end shell script, whose whole point is the
/// relative paths an auditor types.
pub struct WorkingDirectory {
    original: PathBuf,
    // Restoring happens in `drop` below, which runs before any field is
    // dropped, so the directory is always back before the lock is released.
    _guard: MutexGuard<'static, ()>,
}

impl WorkingDirectory {
    /// Takes the lock without moving anywhere.
    ///
    /// For a test that only reads the working directory, or that expects a
    /// command to leave it alone.
    pub fn hold() -> Self {
        // A test that panics while holding the lock poisons it. The directory
        // is still restored by the guard's `Drop`, so the state the next test
        // depends on is sound, and honouring the poison would turn one real
        // failure into a cascade of unrelated ones.
        let guard = CURRENT_DIRECTORY
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        Self {
            original: env::current_dir().expect("failed to read the working directory"),
            _guard: guard,
        }
    }

    /// Takes the lock and moves into `path` for the life of the guard.
    pub fn enter<P>(path: P) -> Self
    where
        P: AsRef<Path>,
    {
        let held = Self::hold();
        env::set_current_dir(path).expect("failed to enter the test directory");
        held
    }

    /// The directory the process will be returned to.
    pub fn original(&self) -> &Path {
        &self.original
    }
}

impl Drop for WorkingDirectory {
    fn drop(&mut self) {
        env::set_current_dir(&self.original).expect("failed to restore the working directory");
    }
}

/// The one lock behind every test that touches `$HOME`.
static HOME: Mutex<()> = Mutex::new(());

/// Exclusive control of `$HOME`, restored on drop.
///
/// `$HOME` is process-wide, like the working directory above and for the same
/// reason: a test that points it at a scratch directory — to check history
/// persistence without touching a real person's `~/.0-shell_history` — has to
/// hold this lock rather than race another test doing the same.
pub struct EnvHome {
    original: Option<std::ffi::OsString>,
    _guard: MutexGuard<'static, ()>,
}

impl EnvHome {
    /// Takes the lock and points `$HOME` at `path` for the life of the guard.
    pub fn set<P>(path: P) -> Self
    where
        P: AsRef<Path>,
    {
        let (guard, original) = Self::take();

        // SAFETY: every reader and writer of `HOME` in this process is a test,
        // and every one of them holds the lock above before touching it, so
        // this is never concurrent with another thread's read of the same
        // variable.
        unsafe {
            env::set_var("HOME", path.as_ref());
        }

        Self {
            original,
            _guard: guard,
        }
    }

    /// Takes the lock and removes `$HOME` entirely for the life of the guard.
    pub fn unset() -> Self {
        let (guard, original) = Self::take();

        // SAFETY: see `set`, above.
        unsafe {
            env::remove_var("HOME");
        }

        Self {
            original,
            _guard: guard,
        }
    }

    fn take() -> (MutexGuard<'static, ()>, Option<std::ffi::OsString>) {
        let guard = HOME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        (guard, env::var_os("HOME"))
    }
}

impl Drop for EnvHome {
    fn drop(&mut self) {
        // SAFETY: see `set`, above; the same guard is still held here.
        unsafe {
            match &self.original {
                Some(value) => env::set_var("HOME", value),
                None => env::remove_var("HOME"),
            }
        }
    }
}
