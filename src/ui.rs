/// User interface and status output utilities
///
/// This module handles:
/// - Thread-safe console output
/// - Colored terminal text
/// - Status message formatting

use lazy_static::lazy_static;
use std::io::Write;
use std::sync::Mutex;

/// Execute a function with exclusive access to console output
/// Prevents interleaved output from multiple threads
fn status_lock<F>(f: F)
where
    F: FnOnce() -> (),
{
    lazy_static! {
        static ref LOCK: Mutex<()> = Mutex::new(());
    }
    let _guard = LOCK.lock();
    f();
}

/// Print the "copter: " prefix for status messages
fn print_status_header() {
    print!("copter: ");
}

/// Print colored text to terminal, with fallback to plain text
fn print_color(s: &str, fg: term::color::Color) {
    if !really_print_color(s, fg) {
        print!("{}", s);
    }

    fn really_print_color(s: &str, fg: term::color::Color) -> bool {
        if let Some(ref mut t) = term::stdout() {
            if t.fg(fg).is_err() {
                return false;
            }
            let _ = t.attr(term::Attr::Bold);
            if write!(t, "{}", s).is_err() {
                return false;
            }
            let _ = t.reset();
        }

        true
    }
}

/// Print a status message with "copter: " prefix (thread-safe)
pub fn status(s: &str) {
    status_lock(|| {
        print_status_header();
        println!("{}", s);
    });
}

/// Print an error message with colored "error" prefix
pub fn print_error(msg: &str) {
    println!("");
    print_color("error", term::color::BRIGHT_RED);
    println!(": {}", msg);
    println!("");
}
