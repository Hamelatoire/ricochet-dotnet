//! Port of src/Ricochet.Core/CrashLog.cs (ADR 0088). Writes a local crash log to the personal folder.
//! Nothing is sent anywhere (ADR 0005): the person decides whether to attach the log to a bug report.
//!
//! The log holds the error kinds, the stack traces and the app and OS details. It never holds request data, and
//! error messages are not part of the input at all, because they can carry URLs, Variable values or Secrets.
//! Stack traces are scrubbed before writing: the user's profile folder becomes "~", and anything that looks like a
//! credential is masked.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use chrono::{DateTime, Utc};
use regex::Regex;

/// How many crash logs are kept. Older ones are deleted when a new one is written.
pub const MAX_LOGS: usize = 20;

const FOLDER_NAME: &str = "crashes";
const FILE_PREFIX: &str = "crash-";
const FILE_SUFFIX: &str = ".log";

/// Matches `token=value`, `password: value`, `api_key=value` and similar, optionally followed by `Bearer `.
/// The value stops at whitespace or at a URL or list separator, so `&other=1` after a token is kept.
static SECRET_PAIR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)\b(token|access[_-]?token|password|passwd|pwd|secret|client[_-]?secret|api[_-]?key|authorization)(\s*[=:]\s*)(?:bearer\s+)?[^\s&"',;]+"#,
    )
    .expect("secret pattern is valid")
});

/// Matches a bare `Bearer <credential>` anywhere in the text.
static BEARER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bbearer\s+[A-Za-z0-9._~+/=-]+").expect("bearer pattern is valid"));

/// One error in a crash: its kind (the type name) and its stack trace, if it has one.
/// The first entry is the error itself; the rest are the inner errors, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrashEntry {
    pub kind: String,
    pub trace: Option<String>,
}

/// The app and OS details written into the header of a crash log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrashContext {
    pub app_version: String,
    pub os_description: String,
    pub is_64_bit: bool,
}

/// The folder crash logs are written to, inside the personal directory.
pub fn directory_for(personal_directory: &Path) -> PathBuf {
    personal_directory.join(FOLDER_NAME)
}

/// Writes one crash log and removes the oldest ones beyond [`MAX_LOGS`]. Returns the path written, or `None` if
/// writing failed. It never panics, because it runs while the app is already crashing.
pub fn write(
    personal_directory: &Path,
    entries: &[CrashEntry],
    context: &CrashContext,
    now: Option<DateTime<Utc>>,
) -> Option<PathBuf> {
    let directory = directory_for(personal_directory);
    fs::create_dir_all(&directory).ok()?;

    let stamp = now.unwrap_or_else(Utc::now);
    let path = directory.join(format!("{FILE_PREFIX}{}{FILE_SUFFIX}", stamp.format("%Y%m%d-%H%M%S-%3f")));

    let text = format(entries, context, stamp, &user_profile());
    fs::write(&path, text).ok()?;
    prune(&directory);
    Some(path)
}

/// Builds the text of a crash log. `profile` is the user's profile folder, which is shortened to "~" in the traces.
pub fn format(entries: &[CrashEntry], context: &CrashContext, utc: DateTime<Utc>, profile: &str) -> String {
    let mut out = String::new();
    out.push_str("Ricochet crash log\n");
    out.push_str("Written only on this machine. Nothing is sent anywhere.\n");
    out.push_str("Exception messages and request data are left out. Stack traces may include file paths.\n");
    out.push('\n');
    let _ = writeln!(out, "Time (UTC): {}", utc.format("%Y-%m-%d %H:%M:%S"));
    let _ = writeln!(out, "App version: {}", context.app_version);
    let bits = if context.is_64_bit { "64-bit" } else { "32-bit" };
    let _ = writeln!(out, "Operating system: {} ({bits})", context.os_description);
    out.push('\n');

    for (depth, entry) in entries.iter().enumerate() {
        out.push_str(if depth == 0 { "Exception:\n" } else { "Inner exception:\n" });
        let _ = writeln!(out, "  {}", entry.kind);
        if let Some(trace) = entry.trace.as_deref().filter(|t| !t.is_empty()) {
            let scrubbed = mask_secrets(&redact_profile(trace, profile));
            for line in scrubbed.lines() {
                let _ = writeln!(out, "  {line}");
            }
        }
    }

    out
}

/// Replaces the user's profile folder with "~" in a path or stack trace. Matching ignores case, as Windows paths do.
/// An empty `profile` leaves the text unchanged.
pub fn redact_profile(text: &str, profile: &str) -> String {
    if profile.is_empty() {
        return text.to_string();
    }

    let pattern = Regex::new(&format!("(?i){}", regex::escape(profile))).expect("escaped profile is a valid pattern");
    pattern.replace_all(text, "~").into_owned()
}

/// Masks credentials in a stack trace or path: `token=...`, `password: ...`, `Authorization: Bearer ...` and similar
/// keep their name and lose their value, which becomes `***`.
pub fn mask_secrets(text: &str) -> String {
    let masked = SECRET_PAIR.replace_all(text, "${1}${2}***");
    BEARER.replace_all(&masked, "Bearer ***").into_owned()
}

/// Deletes the oldest crash logs beyond [`MAX_LOGS`]. Only files this module names (`crash-*.log`) are touched.
/// A file that cannot be deleted, for example because it is in use, stays, and the next write tries again.
pub fn prune(directory: &Path) {
    let Ok(read) = fs::read_dir(directory) else {
        return;
    };

    let mut logs: Vec<String> = read
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(FILE_PREFIX) && name.ends_with(FILE_SUFFIX))
        .collect();
    logs.sort();

    let excess = logs.len().saturating_sub(MAX_LOGS);
    for name in &logs[..excess] {
        let _ = fs::remove_file(directory.join(name));
    }
}

/// The user's profile folder, from the environment. Empty when it is not set.
fn user_profile() -> String {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default()
}
