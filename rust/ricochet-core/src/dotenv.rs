//! Port of src/Ricochet.Core/DotEnv.cs (ADR 0021). Reads a .env file in the workspace folder for local runs.
//! Its values are Secrets, so they never go into Workspace files and are masked in output.

use std::fs;
use std::io;
use std::path::Path;

use crate::models::KeyValue;

/// The name of the file read from the workspace folder.
pub const FILE_NAME: &str = ".env";

/// The variables in a .env file, and the 1-based line numbers of lines that could not be read.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DotEnvResult {
    pub variables: Vec<KeyValue>,
    pub skipped_lines: Vec<usize>,
}

/// Reads `.env` from the workspace folder. A missing file is not an error: most workspaces have none.
/// Bytes are decoded as the C# `File.ReadAllText` does: a UTF-8 or UTF-16 byte order mark is honoured and
/// invalid UTF-8 is replaced rather than failing.
pub fn read(workspace_folder: &Path) -> io::Result<DotEnvResult> {
    let path = workspace_folder.join(FILE_NAME);
    if !path.is_file() {
        return Ok(DotEnvResult::default());
    }
    let bytes = fs::read(&path)?;
    Ok(parse(&decode(&bytes)))
}

/// Accepts KEY=VALUE lines, with blank lines, # comments, an optional "export " prefix, and values in
/// matching single or double quotes. Anything else is skipped and reported by line number, never with its text.
pub fn parse(text: &str) -> DotEnvResult {
    let mut variables = Vec::new();
    let mut skipped_lines = Vec::new();

    for (index, raw) in text.replace("\r\n", "\n").split('\n').enumerate() {
        let mut line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some(rest) = line.strip_prefix("export ") {
            line = rest.trim_start();
        }

        let Some(equals) = line.find('=') else {
            skipped_lines.push(index + 1);
            continue;
        };

        let key = line[..equals].trim();
        if !is_name(key) {
            skipped_lines.push(index + 1);
            continue;
        }

        // Values are Secrets, so the app hides them and saved examples mask them (ADR 0021). An empty value
        // stays plain: a Secret with no value is skipped when a send resolves, and an empty .env value should still count.
        let value = unquote(line[equals + 1..].trim());
        let is_secret = !value.is_empty();
        variables.push(KeyValue {
            key: key.to_string(),
            value: value.to_string(),
            is_secret,
            ..KeyValue::default()
        });
    }

    DotEnvResult { variables, skipped_lines }
}

/// Matches `^[A-Za-z_][A-Za-z0-9_]*$`.
fn is_name(key: &str) -> bool {
    let mut chars = key.bytes();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == b'_' => {
            chars.all(|b| b.is_ascii_alphanumeric() || b == b'_')
        }
        _ => false,
    }
}

/// Removes one pair of matching single or double quotes. Both quote characters are ASCII, so slicing by byte is safe.
fn unquote(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 && (bytes[0] == b'"' || bytes[0] == b'\'') && bytes[bytes.len() - 1] == bytes[0] {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

/// Decodes file bytes the way `File.ReadAllText` does for the byte order marks it detects.
fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return decode_utf16(rest, u16::from_le_bytes);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return decode_utf16(rest, u16::from_be_bytes);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

fn decode_utf16(bytes: &[u8], to_unit: fn([u8; 2]) -> u16) -> String {
    let units = bytes.chunks_exact(2).map(|pair| to_unit([pair[0], pair[1]]));
    char::decode_utf16(units)
        .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}
