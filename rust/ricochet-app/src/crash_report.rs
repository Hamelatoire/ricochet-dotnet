//! Catches panics and writes them to the local crash log (ADR 0088), as `App.axaml.cs` does for unhandled exceptions.
//! Nothing is sent anywhere (ADR 0005). The log is built by `ricochet_core::crash_log`, which redacts the profile
//! folder and masks credentials in the trace.
//!
//! The panic message is never written: like an exception message in the C# app, it can carry URLs, Variable values or
//! Secrets. Only where the panic happened, the thread and the backtrace are kept.

use std::backtrace::Backtrace;
use std::panic::{self, PanicHookInfo};
use std::path::{Path, PathBuf};

use ricochet_core::crash_log::{self, CrashContext, CrashEntry};

/// The app version written into crash logs.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The app and OS details for the log header.
pub fn context() -> CrashContext {
    CrashContext {
        app_version: APP_VERSION.to_string(),
        os_description: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        is_64_bit: cfg!(target_pointer_width = "64"),
    }
}

/// Builds the log entry for one panic. `location` is `file:line:column`, `thread` the panicking thread's name, and
/// `backtrace` the captured trace. The panic message is not an input, so it can never reach the log.
pub fn crash_entry(location: Option<&str>, thread: Option<&str>, backtrace: &str) -> CrashEntry {
    let mut trace = String::new();
    if let Some(location) = location {
        trace.push_str("at ");
        trace.push_str(location);
        trace.push('\n');
    }
    trace.push_str(backtrace.trim_end());

    CrashEntry {
        kind: format!("panic on thread '{}'", thread.unwrap_or("unnamed")),
        trace: Some(trace),
    }
}

/// Writes the log for one panic and returns the path written, or `None` if writing failed.
pub fn write_panic_log(personal_directory: &Path, info: &PanicHookInfo<'_>) -> Option<PathBuf> {
    let location = info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
    let current = std::thread::current();
    let backtrace = Backtrace::force_capture().to_string();
    let entry = crash_entry(location.as_deref(), current.name(), &backtrace);
    crash_log::write(personal_directory, &[entry], &context(), None)
}

/// Installs the panic hook. Set up first in `main`, so early crashes are caught too. Each panic writes a log, then the
/// hook that was there before runs, so the usual message still goes to stderr. Without a personal folder nothing is
/// written.
pub fn install(personal_directory: Option<PathBuf>) {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        if let Some(directory) = personal_directory.as_deref() {
            write_panic_log(directory, info);
        }
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_keeps_location_thread_and_trace() {
        let entry = crash_entry(Some("src/main.rs:10:5"), Some("main"), "   0: ricochet_app::main\n");
        assert_eq!(entry.kind, "panic on thread 'main'");
        assert_eq!(entry.trace.as_deref(), Some("at src/main.rs:10:5\n   0: ricochet_app::main"));
    }

    #[test]
    fn entry_names_an_unnamed_thread() {
        let entry = crash_entry(None, None, "");
        assert_eq!(entry.kind, "panic on thread 'unnamed'");
        assert_eq!(entry.trace.as_deref(), Some(""));
    }

    #[test]
    fn context_has_the_app_version_and_os() {
        let context = context();
        assert_eq!(context.app_version, APP_VERSION);
        assert!(context.os_description.contains(std::env::consts::OS));
    }
}
