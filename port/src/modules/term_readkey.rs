//! Term::ReadKey, the part the exiftool script uses for -k (-pause):
//! `ReadMode('cbreak')`, `ReadKey(0)` and `ReadMode(0)` on STDIN.
//!
//! ReadMode sets the terminal's mode as Term::ReadKey 2.38 does: with
//! termios on Unix (the original settings kept on the first change and
//! put back by mode 0), with the console modes on Windows. When STDIN is
//! not a terminal (a pipe, a file, /dev/null) the mode changes fail and
//! nothing happens, as there. ReadKey(0) is perl's `getc STDIN`: the next
//! character, or undef at the end of the input.

use std::cell::Cell;

use crate::interp::eval::die;
use crate::interp::Interp;
use crate::perl::Value;
use crate::pl::arg;

/// The modes by name (%Term::ReadKey::modes; lowercase is canonical).
pub fn mode_number(name: &[u8]) -> Option<i64> {
    Some(match name.to_ascii_lowercase().as_slice() {
        b"original" | b"restore" => 0,
        b"normal" => 1,
        b"noecho" => 2,
        b"cbreak" => 3,
        b"raw" => 4,
        b"ultra-raw" => 5,
        _ => return None,
    })
}

#[cfg(unix)]
mod term {
    use std::cell::RefCell;

    thread_local! {
        /// The terminal's settings before the first change.
        static ORIGINAL: RefCell<Option<libc::termios>> = const { RefCell::new(None) };
    }

    /// SetReadMode(mode) on descriptor 0, as ReadKey.xs's termios branch.
    pub fn set_mode(mode: i64) {
        let fd = 0;
        // SAFETY: termios is plain data that tcgetattr fills in
        let mut cur: libc::termios = unsafe { std::mem::zeroed() };
        // (not a terminal: nothing to change, and no error)
        if unsafe { libc::tcgetattr(fd, &mut cur) } != 0 {
            return;
        }
        let orig = ORIGINAL.with(|o| *o.borrow_mut().get_or_insert(cur));
        let mut work = orig;
        match mode {
            0 => {
                ORIGINAL.with(|o| *o.borrow_mut() = None);
            }
            1 => {
                work.c_lflag |= libc::ICANON | libc::ECHO | libc::ISIG | libc::IEXTEN;
            }
            2 => {
                work.c_lflag &= !(libc::ECHO | libc::ECHOE | libc::ECHOK | libc::ECHONL);
                work.c_lflag |= libc::ICANON | libc::ISIG | libc::IEXTEN;
            }
            3 => {
                work.c_lflag &= !(libc::ICANON | libc::ECHO | libc::ECHOE | libc::ECHOK | libc::ECHONL);
                work.c_lflag |= libc::ISIG | libc::IEXTEN;
                work.c_cc[libc::VMIN] = 1;
                work.c_cc[libc::VTIME] = 0;
            }
            4 => {
                work.c_lflag &= !(libc::ICANON | libc::ISIG | libc::ECHO | libc::ECHOE | libc::ECHOK | libc::ECHONL | libc::IEXTEN);
                work.c_iflag &= !(libc::BRKINT | libc::IXON);
                work.c_cc[libc::VMIN] = 1;
                work.c_cc[libc::VTIME] = 0;
            }
            _ => {
                work.c_lflag &= !(libc::ICANON | libc::ISIG | libc::ECHO | libc::ECHOE | libc::ECHOK | libc::ECHONL | libc::IEXTEN);
                work.c_iflag &= !(libc::BRKINT | libc::ICRNL | libc::INLCR | libc::IGNCR | libc::IXON | libc::IXOFF | libc::ISTRIP | libc::INPCK);
                work.c_oflag &= !libc::OPOST;
                work.c_cc[libc::VMIN] = 1;
                work.c_cc[libc::VTIME] = 0;
            }
        }
        // SAFETY: fd 0 is a terminal (tcgetattr succeeded); work is valid
        unsafe { libc::tcsetattr(fd, libc::TCSANOW, &work) };
    }
}

#[cfg(windows)]
mod term {
    use std::cell::Cell;

    use windows_sys::Win32::System::Console::{GetConsoleMode, GetStdHandle, SetConsoleMode, CONSOLE_MODE, ENABLE_ECHO_INPUT, ENABLE_LINE_INPUT, ENABLE_PROCESSED_INPUT, STD_INPUT_HANDLE};

    thread_local! {
        /// The console's input mode before the first change.
        static ORIGINAL: Cell<Option<CONSOLE_MODE>> = const { Cell::new(None) };
    }

    /// SetReadMode(mode) on the console's input, as ReadKey.xs's Win32
    /// branch.
    pub fn set_mode(mode: i64) {
        // SAFETY: plain console calls on the process's own input handle
        unsafe {
            let h = GetStdHandle(STD_INPUT_HANDLE);
            let mut cur: CONSOLE_MODE = 0;
            // (not a console: nothing to change, and no error)
            if GetConsoleMode(h, &mut cur) == 0 {
                return;
            }
            let orig = ORIGINAL.with(|o| {
                let v = o.get().unwrap_or(cur);
                o.set(Some(v));
                v
            });
            let keep = orig & !(ENABLE_ECHO_INPUT | ENABLE_LINE_INPUT | ENABLE_PROCESSED_INPUT);
            let work = match mode {
                0 => {
                    ORIGINAL.with(|o| o.set(None));
                    orig
                }
                1 => keep | ENABLE_ECHO_INPUT | ENABLE_LINE_INPUT | ENABLE_PROCESSED_INPUT,
                2 => keep | ENABLE_LINE_INPUT | ENABLE_PROCESSED_INPUT,
                3 => keep | ENABLE_PROCESSED_INPUT,
                _ => keep,
            };
            SetConsoleMode(h, work);
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod term {
    pub fn set_mode(_mode: i64) {}
}

thread_local! {
    /// $Term::ReadKey::CurrentMode
    static CURRENT: Cell<i64> = const { Cell::new(0) };
}

pub fn register(it: &mut Interp) {
    it.set_scalar("Term::ReadKey::VERSION", Value::str("2.38"));
    // ReadMode(MODE [, FH]): a mode by name or number
    it.define_native("Term::ReadKey::ReadMode", |_, a, _| {
        let m = arg(&a, 0);
        let b = m.to_bytes();
        let mode = match mode_number(&b) {
            Some(n) => n,
            None if b.first().is_some_and(|c| c.is_ascii_digit()) => m.to_i64(),
            None => return die(format!("Unknown terminal mode `{}'", m.to_text())),
        };
        CURRENT.with(|c| c.set(mode));
        term::set_mode(mode);
        Ok(vec![])
    });
    // ReadKey(0 [, FH]): `getc FH` (STDIN by default)
    it.define_native("Term::ReadKey::ReadKey", |it, a, _| {
        let fh = a.get(1).cloned().unwrap_or_else(|| Value::str("STDIN"));
        Ok(vec![it.getc(&fh).unwrap_or_default()])
    });
}

#[cfg(test)]
mod tests {
    use super::mode_number;

    #[test]
    fn modes_by_name() {
        assert_eq!(mode_number(b"cbreak"), Some(3));
        assert_eq!(mode_number(b"Normal"), Some(1));
        assert_eq!(mode_number(b"restore"), Some(0));
        assert_eq!(mode_number(b"ultra-raw"), Some(5));
        assert_eq!(mode_number(b"0"), None);
    }
}
