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
//! The system call numbers, ioctl requests, and `struct termios` layout are
//! different on Linux and macOS, and the constants below are taken from each
//! kernel's own headers. They are compiled in only for the systems and
//! architectures they belong to, because a number meant for one kernel is
//! garbage to the other: issuing Linux's ioctl numbers on macOS would raise a
//! bad system call instead of an error. On any other system the editor is
//! unavailable and the shell falls back to plain whole-line reading. Every
//! required behaviour is unchanged on the fallback path; only the bonus
//! features are absent.
//!
//! Nothing here spawns a process. `stty` would be the easy way to set these
//! bits and is exactly what the subject forbids.

use std::io;
use std::os::fd::{AsRawFd, RawFd};

/// The ioctl that reads the terminal settings.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
const TCGETS: usize = 0x5401; // `asm-generic/ioctls.h`
#[cfg(all(
    target_os = "macos",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
const TCGETS: usize = 0x4048_7413; // `TIOCGETA`, `sys/ttycom.h`

/// The ioctl that writes the terminal settings, immediately.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
const TCSETS: usize = 0x5402; // `asm-generic/ioctls.h`
#[cfg(all(
    target_os = "macos",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
const TCSETS: usize = 0x8048_7414; // `TIOCSETA`, `sys/ttycom.h`

/// The ioctl that reads the terminal's size, in rows and columns.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
const TIOCGWINSZ: usize = 0x5413; // `asm-generic/ioctls.h`
#[cfg(all(
    target_os = "macos",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
const TIOCGWINSZ: usize = 0x4008_7468; // `sys/ttycom.h`

/// `SYS_ioctl`, which differs per system and architecture.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const SYS_IOCTL: usize = 16;
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
const SYS_IOCTL: usize = 29;
#[cfg(all(
    target_os = "macos",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
const SYS_IOCTL: usize = 54; // `sys/syscall.h`, the same number on both

/// Signal generation: Ctrl+C raises `SIGINT`, Ctrl+Z raises `SIGTSTP`.
#[cfg(target_os = "linux")]
const ISIG: u32 = 0o000001;
#[cfg(target_os = "macos")]
const ISIG: u64 = 0x0000_0080;
/// Canonical mode: hold input back until a whole line is ready.
#[cfg(target_os = "linux")]
const ICANON: u32 = 0o000002;
#[cfg(target_os = "macos")]
const ICANON: u64 = 0x0000_0100;
/// Echo input characters back to the screen.
#[cfg(target_os = "linux")]
const ECHO: u32 = 0o000010;
#[cfg(target_os = "macos")]
const ECHO: u64 = 0x0000_0008;
/// Extended input processing, which includes Ctrl+V.
#[cfg(target_os = "linux")]
const IEXTEN: u32 = 0o100000;
#[cfg(target_os = "macos")]
const IEXTEN: u64 = 0x0000_0400;
/// Start/stop flow control, which is Ctrl+S and Ctrl+Q.
#[cfg(target_os = "linux")]
const IXON: u32 = 0o002000;
#[cfg(target_os = "macos")]
const IXON: u64 = 0x0000_0200;
/// Translate carriage return to newline on input.
#[cfg(target_os = "linux")]
const ICRNL: u32 = 0o000400;
#[cfg(target_os = "macos")]
const ICRNL: u64 = 0x0000_0100;

/// `c_cc` index: how many bytes must arrive before `read` returns.
#[cfg(target_os = "linux")]
const VMIN: usize = 6;
#[cfg(target_os = "macos")]
const VMIN: usize = 16;
/// `c_cc` index: how long `read` waits, in tenths of a second.
#[cfg(target_os = "linux")]
const VTIME: usize = 5;
#[cfg(target_os = "macos")]
const VTIME: usize = 17;

/// Linux's kernel `struct termios`, as `TCGETS` fills it in.
///
/// This is the kernel layout from `asm-generic/termbits.h`, which is what the
/// ioctl reads and writes. It is not glibc's `struct termios`: that one carries
/// two extra speed fields on the end, and passing it here would have the kernel
/// write past what it was promised.
#[cfg(target_os = "linux")]
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

/// macOS's `struct termios`, as `TIOCGETA` fills it in.
///
/// This is the layout from `sys/termios.h`: four `unsigned long` flag fields,
/// the twenty control characters, then the two speeds, also `unsigned long`.
/// The encoded ioctl sizes confirm it — `TIOCGETA` names a 72-byte structure,
/// which is exactly what this is on 64-bit systems, ispeed and ospeed aligned
/// after the character array.
#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Termios {
    input_flags: u64,
    output_flags: u64,
    control_flags: u64,
    local_flags: u64,
    control_characters: [u8; 20],
    ispeed: u64,
    ospeed: u64,
}

/// The kernel's `struct winsize`, as `TIOCGWINSZ` fills it in: four 16-bit
/// fields, rows and columns first, then a pixel size neither system fills in
/// over a real terminal and this module has no use for. Linux and macOS
/// agree on this one, unlike `termios` above.
#[repr(C)]
#[derive(Default)]
struct WindowSize {
    rows: u16,
    columns: u16,
    _x_pixels: u16,
    _y_pixels: u16,
}

/// Issues a three-argument system call.
///
/// # Safety
///
/// The caller promises that `number` names a system call taking three
/// arguments and that the arguments are valid for it, pointers included. Every
/// call in this module passes `SYS_ioctl` with a file descriptor, a request,
/// and a pointer to a `Termios` this module owns.
#[cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
unsafe fn syscall3(number: usize, first: usize, second: usize, third: usize) -> isize {
    let result: isize;

    // The kernel takes its arguments in fixed registers and answers in one, so
    // the register names below are the calling convention rather than a choice.
    // Both systems use the `syscall` instruction on x86_64, and on aarch64 both
    // commit with `svc`, but the two aarch64 kernels disagree on everything
    // else: Linux reads the number from `x8` and traps with `svc 0`, macOS
    // reads it from `x16` and traps with `svc 0x80`. The numbers and
    // instructions for the wrong system raise a bad system call, so the traps
    // are selected by the operating system, not by the architecture.
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

    #[cfg(all(target_arch = "aarch64", target_os = "linux"))]
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

    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    unsafe {
        std::arch::asm!(
            "svc 0x80",
            in("x16") number,
            inlateout("x0") first => result,
            in("x1") second,
            in("x2") third,
            options(nostack, preserves_flags),
        );
    }

    result
}

/// Turns the kernel's answer into an `io::Result`.
///
/// A Linux system call reports failure as the negated error number, so `-2`
/// is the `ENOENT` that `io::Error` would call `No such file or directory`.
/// macOS answers the other way around: it returns the positive error number
/// itself, so the same failure arrives as `2`, and `io::Error` reads it
/// unchanged.
#[cfg(target_os = "linux")]
fn checked(result: isize) -> io::Result<()> {
    if (-4095..0).contains(&result) {
        return Err(io::Error::from_raw_os_error(-result as i32));
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn checked(result: isize) -> io::Result<()> {
    // `ioctl` answers `0` for a successful setting transition and an errno on
    // failure. Everything `io::Error` knows how to spell lives far below 4095,
    // which is the range Linux reserves for its negatives, so any answer in
    // that band on macOS is a failure.
    if (1..4095).contains(&result) {
        return Err(io::Error::from_raw_os_error(result as i32));
    }

    Ok(())
}

#[cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
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

#[cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
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

#[cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn window_size(fd: RawFd) -> io::Result<WindowSize> {
    let mut size = WindowSize::default();

    // SAFETY: as `get_settings`, above: `TIOCGWINSZ` expects a `struct
    // winsize` to write into, and `size` is exactly that, owned here and
    // pointed to for the duration of the call.
    let result = unsafe {
        syscall3(
            SYS_IOCTL,
            fd as usize,
            TIOCGWINSZ,
            std::ptr::from_mut(&mut size) as usize,
        )
    };

    checked(result).map(|()| size)
}

/// The terminal's width in columns, or `None` when `fd` is not a real
/// terminal, this system has no ioctl stub here, or the kernel answered with
/// a width of zero, which some do over a terminal with no controlling size.
pub fn terminal_width<F>(terminal: &F) -> Option<usize>
where
    F: AsRawFd,
{
    let size = window_size(terminal.as_raw_fd()).ok()?;
    (size.columns > 0).then_some(size.columns as usize)
}

/// On a system or architecture without a syscall stub here, raw mode and the
/// terminal size are both unavailable.
///
/// Reporting "not a terminal" is the same answer a pipe gives, so the shell
/// falls back to reading whole lines and every required behaviour is unchanged.
/// Only the three bonus features are missing.
#[cfg(not(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "x86_64", target_arch = "aarch64")
)))]
fn get_settings(_fd: RawFd) -> io::Result<Termios> {
    Err(io::Error::from_raw_os_error(25))
}

#[cfg(not(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "x86_64", target_arch = "aarch64")
)))]
pub fn terminal_width<F>(_terminal: &F) -> Option<usize>
where
    F: AsRawFd,
{
    None
}

#[cfg(not(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "x86_64", target_arch = "aarch64")
)))]
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
    use std::mem::{offset_of, size_of};

    use super::{RawMode, TCGETS, TCSETS, TIOCGWINSZ, Termios, WindowSize, terminal_width};

    #[test]
    fn a_file_is_not_a_terminal_and_says_so() {
        // The answer the shell relies on to choose the plain reading path. A
        // test harness and a piped audit session both land here, so this is
        // the branch that keeps every required behaviour unchanged.
        let temp = crate::testing::TempDir::new("raw_not_a_terminal");
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

    #[test]
    fn the_termios_layout_matches_the_ioctl_sizes() {
        // The macOS ioctl request numbers carry the size of the struct they
        // expect in their high bits, so a layout that drifts would eventually
        // make the kernel read or write past a buffer this module sized by
        // hand. Linux's requests are the old style and carry no size.
        #[cfg(target_os = "macos")]
        {
            assert_eq!(size_of::<Termios>(), 72, "the encoded TIOCGETA size");
            assert_eq!((TCGETS >> 16) & 0x3fff, size_of::<Termios>());
            assert_eq!((TCSETS >> 16) & 0x3fff, size_of::<Termios>());
            assert_eq!(offset_of!(Termios, control_characters), 32);
            assert_eq!(offset_of!(Termios, ispeed), 56);
        }
        #[cfg(target_os = "linux")]
        {
            assert_eq!(size_of::<Termios>(), 36, "asm-generic termios");
        }
    }

    #[test]
    fn the_window_size_layout_matches_the_ioctl_size() {
        assert_eq!(size_of::<WindowSize>(), 8, "four u16 fields");

        #[cfg(target_os = "macos")]
        assert_eq!((TIOCGWINSZ >> 16) & 0x3fff, size_of::<WindowSize>());
    }

    #[test]
    fn a_file_has_no_width_to_report() {
        let temp = crate::testing::TempDir::new("raw_no_width");
        let file = File::create(temp.arg("regular")).unwrap();

        assert_eq!(terminal_width(&file), None);
    }
}
