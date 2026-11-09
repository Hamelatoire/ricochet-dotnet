//! Port of src/Ricochet.Core/ResponseStatus.cs (ADR 0091).
//!
//! Classifies a status code into the colour family of its badge, and suggests a file name for saving a
//! response body from its media type.

/// How a response's status code is shown: the colour family of its status badge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseStatusClass {
    /// 1xx, or any code outside 100–599: shown without a colour.
    Neutral,
    Success,
    Redirect,
    ClientError,
    ServerError,
}

/// Classifies a status code by its family. Codes outside 200–599 that are not success, redirect or
/// error codes (1xx, 0, 600 and above, negatives) are `Neutral`.
pub fn classify(status_code: i32) -> ResponseStatusClass {
    match status_code {
        200..=299 => ResponseStatusClass::Success,
        300..=399 => ResponseStatusClass::Redirect,
        400..=499 => ResponseStatusClass::ClientError,
        500..=599 => ResponseStatusClass::ServerError,
        _ => ResponseStatusClass::Neutral,
    }
}

/// A file name for saving the body, chosen from its media type so that the file opens in the right
/// application. Parameters after `;` are ignored and the match is case-insensitive. Unknown or missing
/// types fall back to `bin`, giving `response.bin`.
pub fn suggested_body_file_name(media_type: Option<&str>) -> String {
    let media_type = media_type.unwrap_or("");
    let type_name = media_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase();

    let extension = match type_name.as_str() {
        "image/svg+xml" => "svg",
        t if t.ends_with("json") => "json",
        t if t.ends_with("xml") => "xml",
        "text/html" => "html",
        "text/css" => "css",
        "text/javascript" | "application/javascript" => "js",
        "text/csv" => "csv",
        "text/plain" => "txt",
        "application/pdf" => "pdf",
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "application/zip" => "zip",
        _ => "bin",
    };

    format!("response.{extension}")
}
