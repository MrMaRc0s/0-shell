//! Raw terminal mode, spoken to the kernel directly.
//!
//! A terminal starts in *canonical* mode: the kernel buffers a whole line
//! before any of it reaches the program, echoes it, handles backspace itself,
//! and turns Ctrl+C into a `SIGINT` that kills the process. None of the three
//! bonus features can exist under those rules. Completion needs to see Tab the
//! moment it is pressed, history needs the arrow keys, and Ctrl+C has to arrive
//! as something this shell can decide about rather than a signal that ends it.
//!
//! Switching modes is the `TCSETS` ioctl, and the standard library has no
//! binding for it. Two ways to reach one: link `libc`, or issue the system call
//! here. This module does the latter, so `Cargo.toml` stays empty and the
//! project keeps resolving everything itself, the way `ls -l` already parses
//! `/etc/passwd` and computes dates by hand.
//!
//! Nothing here spawns a process. `stty` would be the easy way to set these
//! bits and is exactly what the subject forbids.

use std::io;
use std::os::fd::{AsRawFd, RawFd};

/// `TCGETS`, "read the terminal settings" (`asm-generic/ioctls.h`).
const TCGETS: usize = 0x5401;
/// `TCSETS`, "write the terminal settings, immediately".
const TCSETS: usize = 0x5402;

/// Signal generation: Ctrl+C raises `SIGINT`, Ctrl+Z raises `SIGTSTP`.
const ISIG: u32 = 0o000001;
/// Canonical mode: hold input back until a whole line is ready.
const ICANON: u32 = 0o000002;
/// Echo input characters back to the screen.
const ECHO: u32 = 0o000010;
/// Extended input processing, which includes Ctrl+V.
const IEXTEN: u32 = 0o100000;
/// Start/stop flow control, which is Ctrl+S and Ctrl+Q.
const IXON: u32 = 0o002000;
/// Translate carriage return to newline on input.
const ICRNL: u32 = 0o000400;

/// `c_cc` index: how many bytes must arrive before `read` returns.
const VMIN: usize = 6;
/// `c_cc` index: how long `read` waits, in tenths of a second.
const VTIME: usize = 5;

/// The kernel's `struct termios`, as `TCGETS` fills it in.
///
/// This is the kernel layout from `asm-generic/termbits.h`, which is what the
/// ioctl reads and writes. It is not glibc's `struct termios`: that one carries
/// two extra speed fields on the end, and passing it here would have the kernel
/// write past what it was promised.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Termios {
    input_flags: u32,
    output_flags: u32,
    control_flags: u32,
    local_flags: u32,
    line_discipline: u8,
    control_characters: [u8; 19],
}

/// Issues a three-argument system call.
///
/// # Safety
///
/// The caller promises that `number` names a system call taking three
/// arguments and that the arguments are valid for it, pointers included. Every
/// call in this module passes `SYS_ioctl` with a file descriptor, a request,
/// and a pointer to a `Termios` this module owns.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
unsafe fn syscall3(number: usize, first: usize, second: usize, third: usize) -> isize {
    let result: isize;

    // The kernel takes its arguments in fixed registers and answers in one, so
    // the register names below are the calling convention rather than a choice.
    #[cfg(target_arch = "x86_64")]
    unsafe {
        std::arch::asm!(
            "syscall",
            inlateout("rax") number as isize => result,
            in("rdi") first,
            in("rsi") second,
            in("rdx") third,
            // `syscall` clobbers these two, so they are declared dead rather
            // than left for the compiler to assume it can still use them.
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack, preserves_flags),
        );
    }

    #[cfg(target_arch = "aarch64")]
    unsafe {
        std::arch::asm!(
            "svc 0",
            in("x8") number,
            inlateout("x0") first => result,
            in("x1") second,
            in("x2") third,
            options(nostack, preserves_flags),
        );
    }

    result
}

/// `SYS_ioctl`, which differs per architecture.
#[cfg(target_arch = "x86_64")]
const SYS_IOCTL: usize = 16;
#[cfg(target_arch = "aarch64")]
const SYS_IOCTL: usize = 29;

/// Turns the kernel's answer into an `io::Result`.
///
/// A system call reports failure as the negated error number, so `-2` is the
/// `ENOENT` that `io::Error` would call `No such file or directory`.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn checked(result: isize) -> io::Result<()> {
    if (-4095..0).contains(&result) {
        return Err(io::Error::from_raw_os_error(-result as i32));
    }

    Ok(())
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn get_settings(fd: RawFd) -> io::Result<Termios> {
    let mut settings = Termios::default();

    // SAFETY: `SYS_ioctl` takes three arguments. `fd` is a descriptor the
    // caller holds open, `TCGETS` expects a `struct termios` to write into, and
    // `settings` is exactly that struct, owned here and pointed to for the
    // duration of the call.
    let result = unsafe {
        syscall3(
            SYS_IOCTL,
            fd as usize,
            TCGETS,
            std::ptr::from_mut(&mut settings) as usize,
        )
    };

    checked(result).map(|()| settings)
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn set_settings(fd: RawFd, settings: &Termios) -> io::Result<()> {
    // SAFETY: as above, except that `TCSETS` only reads the struct.
    let result = unsafe {
        syscall3(
            SYS_IOCTL,
            fd as usize,
            TCSETS,
            std::ptr::from_ref(settings) as usize,
        )
    };

    checked(result)
}

/// On an architecture without a syscall stub here, raw mode is unavailable.
///
/// Reporting "not a terminal" is the same answer a pipe gives, so the shell
/// falls back to reading whole lines and every required behaviour is unchanged.
/// Only the three bonus features are missing.
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn get_settings(_fd: RawFd) -> io::Result<Termios> {
    Err(io::Error::from_raw_os_error(25))
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn set_settings(_fd: RawFd, _settings: &Termios) -> io::Result<()> {
    Err(io::Error::from_raw_os_error(25))
}

/// Raw mode for as long as this value is alive.
///
/// Dropping it puts the terminal back exactly as it was found, which is the
/// whole reason the original settings are carried around. A shell that exits
/// without restoring them leaves the user typing blind at an unresponsive
/// prompt, so the restore is tied to a value the compiler will not let us
/// forget rather than to a line at the end of a function.
pub struct RawMode {
    fd: RawFd,
    original: Termios,
}

impl RawMode {
    /// Puts `terminal` into raw mode, or reports why it cannot be done.
    ///
    /// The error is the useful part when it fails: a pipe answers `ENOTTY`,
    /// which is how the shell learns it is not talking to a person without
    /// having to ask a second question.
    pub fn enter<F>(terminal: &F) -> io::Result<Self>
    where
        F: AsRawFd,
    {
        let fd = terminal.as_raw_fd();
        let original = get_settings(fd)?;
        let mut raw = original;

        // Ctrl+C stops raising `SIGINT` and starts arriving as byte 3, which is
        // what lets this shell answer it rather than be killed by it. The rest
        // hands over the job the kernel would otherwise do for us: no line
        // buffering, no echo, no flow control stealing Ctrl+S.
        raw.local_flags &= !(ISIG | ICANON | ECHO | IEXTEN);
        raw.input_flags &= !(IXON | ICRNL);

        // Return from `read` as soon as one byte is there, and never on a timer.
        raw.control_characters[VMIN] = 1;
        raw.control_characters[VTIME] = 0;

        set_settings(fd, &raw)?;

        Ok(Self { fd, original })
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        // Nothing useful is left to do if this fails: the shell is on its way
        // out of the editor either way, and a message written to a terminal
        // that will not echo helps nobody.
        set_settings(self.fd, &self.original).ok();
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use super::RawMode;
    use crate::testing::TempDir;

    #[test]
    fn a_file_is_not_a_terminal_and_says_so() {
        // The answer the shell relies on to choose the plain reading path. A
        // test harness and a piped audit session both land here, so this is
        // the branch that keeps every required behaviour unchanged.
        let temp = TempDir::new("raw_not_a_terminal");
        let path = temp.arg("regular");
        let file = File::create(&path).unwrap();

        // Matched rather than unwrapped: the guard restores a terminal on
        // drop and has no business being printable.
        let error = match RawMode::enter(&file) {
            Ok(_) => panic!("a file reported terminal settings"),
            Err(error) => error,
        };

        // ENOTTY, which the C library calls "Inappropriate ioctl for device".
        assert_eq!(error.raw_os_error(), Some(25));
    }
}
