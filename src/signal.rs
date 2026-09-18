//! Handling Ctrl+C while a command is running.
//!
//! The prompt is protected twice over: it reads in raw mode, which stops the
//! kernel raising a signal for Ctrl+C, and the line editor sees that release
//! as byte 3 and simply abandons the line. A *command* has no such shell. The
//! terminal runs in its original settings while the command does its work, so
//! there ISIG is switched back on and Ctrl+C raises `SIGINT` — and a fresh
//! process without a handler dies of it, which is the old behaviour this shell
//! is trying to leave behind.
//!
//! The way out is a handler of the shell's own, installed through the `sigaction`
//! system call. The handler does the one thing a signal handler may safely do:
//! it records "a Ctrl+C happened" in an atomic flag. Commands that stream work
//! for a while — `cat`, `cp`, `mv`, `rm`, `ls` — look at the flag in their
//! loops and stop when it is set, so `cat` on a huge file does not have to run
//! to the end before the interrupt is honoured. Once the command is done the
//! shell clears the flag and asks for the next line, unharmed.
//!
//! Like `crate::terminal::raw`, this speaks to the kernel directly and leaves
//! `Cargo.toml` empty. The system calls and structures differ between Linux and
//! macOS, so each is compiled only for the system that owns its numbers.
//!
//! One deliberate detail: the handler does *not* ask the kernel to restart
//! interrupted system calls. A command blocked reading is exactly the one a
//! Ctrl+C must get out of, and the read returns `EINTR`, which the command's
//! own loop turns into "stop".

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};

/// The signal Ctrl+C raises. Signal 2 on both systems.
const SIGINT: usize = 2;

/// The `sigaction` system call, which Linux puts behind `rt_sigaction`.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const SYS_SIGACTION: usize = 13;
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
const SYS_SIGACTION: usize = 134;
#[cfg(all(
    target_os = "macos",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
const SYS_SIGACTION: usize = 46; // `sys/syscall.h`, the same number on both

/// The kernel's `struct sigaction` on Linux: handler, flags, restorer, mask.
///
/// This is the `k_sigaction` layout from the Linux headers, not glibc's, which
/// is what the `rt_sigaction` system call expects and writes. The mask is the
/// 64-bit kernel `sigset_t`; empty means the kernel blocks only `SIGINT` while
/// the handler runs, which is the default for a handled signal.
#[cfg(target_os = "linux")]
#[repr(C)]
struct SignalAction {
    handler: usize,
    flags: usize,
    restorer: usize,
    mask: usize,
}

/// macOS's `struct sigaction`: handler, mask, flags, as `sys/signal.h` orders
/// them. `sigset_t` is 32 bits there, so the mask needs only a quarter of the
/// word Linux gives it.
#[cfg(target_os = "macos")]
#[repr(C)]
struct SignalAction {
    handler: usize,
    mask: u32,
    flags: u32,
}

/// The record of a Ctrl+C, written by the handler and read by the commands.
///
/// The flag settles exactly what a signal handler must leave alone: nothing is
/// allocated, nothing is locked, and the store is one atomic instruction on
/// every architecture this shell runs on.
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// The handler the kernel calls. It cannot do much: a signal may interrupt any
/// other thread at any instruction, so only async-signal-safe operations are
/// allowed, and storing to an atomic bool is one of them.
extern "C" fn record(_signal: i32) {
    INTERRUPTED.store(true, Ordering::SeqCst);
}

/// Whether a Ctrl+C has arrived since the last clear.
///
/// Commands consult this without disturbing it, so a single interrupt stops
/// every loop it touches: `cat` reads no further blocks, and `cp` copies no
/// further entries, each seeing the same standing flag.
pub fn is_pending() -> bool {
    INTERRUPTED.load(Ordering::SeqCst)
}

/// Consumes the pending Ctrl+C, so the signal is not carried into the next
/// command.
pub fn take() -> bool {
    INTERRUPTED.swap(false, Ordering::SeqCst)
}

/// Forgets any pending Ctrl+C, used before a command so a stale interrupt from
/// the previous line cannot stop this one.
pub fn reset() {
    INTERRUPTED.store(false, Ordering::SeqCst);
}

/// Issues a four-argument system call, the shape `rt_sigaction` takes.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
unsafe fn syscall4(
    number: usize,
    first: usize,
    second: usize,
    third: usize,
    fourth: usize,
) -> isize {
    let result: isize;
    unsafe {
        std::arch::asm!(
            "syscall",
            inlateout("rax") number as isize => result,
            in("rdi") first,
            in("rsi") second,
            in("rdx") third,
            in("r10") fourth,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack, preserves_flags),
        );
    }
    result
}

/// As above, with the `x8` number and `svc 0` trap Linux's arm64 uses.
#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
unsafe fn syscall4(
    number: usize,
    first: usize,
    second: usize,
    third: usize,
    fourth: usize,
) -> isize {
    let result: isize;
    unsafe {
        std::arch::asm!(
            "svc 0",
            in("x8") number,
            inlateout("x0") first => result,
            in("x1") second,
            in("x2") third,
            in("x3") fourth,
            options(nostack, preserves_flags),
        );
    }
    result
}

/// Issues a three-argument system call, the shape macOS's `sigaction` takes.
#[cfg(all(target_arch = "x86_64", target_os = "macos"))]
unsafe fn syscall3(number: usize, first: usize, second: usize, third: usize) -> isize {
    let result: isize;
    unsafe {
        std::arch::asm!(
            "syscall",
            inlateout("rax") number as isize => result,
            in("rdi") first,
            in("rsi") second,
            in("rdx") third,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack, preserves_flags),
        );
    }
    result
}

/// As above, with the `x16` number and `svc 0x80` trap macOS's arm64 uses.
#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
unsafe fn syscall3(number: usize, first: usize, second: usize, third: usize) -> isize {
    let result: isize;
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

/// Turns the kernel's answer into an `io::Result`. Linux reports failure as
/// the negated errno; macOS returns the positive errno itself.
#[cfg(target_os = "linux")]
fn checked(result: isize) -> io::Result<()> {
    if (-4095..0).contains(&result) {
        return Err(io::Error::from_raw_os_error(-result as i32));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn checked(result: isize) -> io::Result<()> {
    if (1..4095).contains(&result) {
        return Err(io::Error::from_raw_os_error(result as i32));
    }
    Ok(())
}

/// Installs the interrupt handler for the life of the shell.
///
/// The failure a syscall genuinely reports is passed on, but the caller treats
/// the whole result as optional: a system this module does not know how to
/// address keeps the old behaviour, where an interrupt ends the shell, rather
/// than refusing to start at all.
pub fn install() -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: `rt_sigaction` takes the four arguments given here. `SIGINT`
        // is the signal number, `action` is a kernel-layout `k_sigaction`
        // owned and initialized in this module, the old action may be null,
        // and the last argument is the size of the kernel's `sigset_t`.
        let action = SignalAction {
            handler: record as *const () as usize,
            flags: 0,
            restorer: 0,
            mask: 0,
        };
        let result = unsafe {
            syscall4(
                SYS_SIGACTION,
                SIGINT,
                std::ptr::from_ref(&action) as usize,
                0,
                8,
            )
        };
        checked(result)
    }

    #[cfg(target_os = "macos")]
    {
        // SAFETY: `sigaction` takes the three arguments given here: the
        // signal, a `struct sigaction` of the layout macOS's kernel reads, and
        // null for the old action.
        let action = SignalAction {
            handler: record as *const () as usize,
            mask: 0,
            flags: 0,
        };
        let result = unsafe {
            syscall3(
                SYS_SIGACTION,
                SIGINT,
                std::ptr::from_ref(&action) as usize,
                0,
            )
        };
        checked(result)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        // The system call numbers and layouts above exist only for the two
        // systems this shell targets. Elsewhere the default SIGINT behaviour
        // stands in, which is the old behaviour before handling existed.
        Err(io::Error::from_raw_os_error(95)) // ENOTSUP
    }
}

#[cfg(test)]
mod tests {
    use std::mem::size_of;

    #[test]
    fn the_signal_action_layout_matches_the_systems() {
        #[cfg(target_os = "linux")]
        assert_eq!(size_of::<super::SignalAction>(), 32);
        #[cfg(target_os = "macos")]
        assert_eq!(size_of::<super::SignalAction>(), 16);
    }

    #[test]
    fn the_flag_starts_clear_and_is_consumed() {
        super::reset();
        assert!(!super::is_pending());
        assert!(!super::take());
    }
}
