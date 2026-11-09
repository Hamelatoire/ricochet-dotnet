//! Port of src/Ricochet.Core/DataFileParser.cs (ADR 0018). Reads a CSV or JSON data file into rows of
//! KeyValues. A CSV file has a header row of names; a JSON file is an array of objects. Each returned row
//! is one run's values. Empty rows are skipped.
//!
//! One run per row is in `data_rows`, and the CLI's `--data` option is in the ricochet-cli crate.

use std::fmt;

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::value::RawValue;

use crate::models::KeyValue;

/// One run's values, in column or property order.
pub type Row = Vec<KeyValue>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataFileFormat {
    Csv,
    Json,
}

/// The file is not in the expected shape. The message names the problem, as the C# RunSetupException does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataFileError {
    message: String,
}

impl DataFileError {
    fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for DataFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for DataFileError {}

/// Takes the format from the file extension, ignoring case.
pub fn format_for(file_name: &str) -> Result<DataFileFormat, DataFileError> {
    match extension_of(file_name).to_lowercase().as_str() {
        ".csv" => Ok(DataFileFormat::Csv),
        ".json" => Ok(DataFileFormat::Json),
        _ => Err(DataFileError::new(format!(
            "\"{file_name}\" is not a data file. Use a .csv or .json file."
        ))),
    }
}

/// Same as Path.GetExtension: the text from the last dot of the file name, including the dot. A name
/// such as ".csv" counts as an extension, and a name with no dot has none.
fn extension_of(file_name: &str) -> &str {
    let name_start = file_name.rfind(['/', '\\']).map_or(0, |i| i + 1);
    let name = &file_name[name_start..];
    match name.rfind('.') {
        Some(dot) => &name[dot..],
        None => "",
    }
}

/// Parses the text of a data file into rows.
pub fn parse(text: &str, format: DataFileFormat) -> Result<Vec<Row>, DataFileError> {
    match format {
        DataFileFormat::Csv => parse_csv(text),
        DataFileFormat::Json => parse_json(text),
    }
}

fn parse_csv(text: &str) -> Result<Vec<Row>, DataFileError> {
    let records = read_csv_records(text)?;
    let Some((_, header_fields)) = records.first() else {
        return Err(DataFileError::new("The CSV file has no header row."));
    };

    let headers: Vec<String> = header_fields.iter().map(|h| h.trim().to_string()).collect();
    for (i, name) in headers.iter().enumerate() {
        if name.is_empty() {
            return Err(DataFileError::new(format!(
                "Column {} of the CSV header has no name.",
                i + 1
            )));
        }
        if headers.iter().position(|h| h == name) != Some(i) {
            return Err(DataFileError::new(format!(
                "The CSV header names \"{name}\" more than once."
            )));
        }
    }

    let mut rows = Vec::new();
    for (line, fields) in records.iter().skip(1) {
        if fields.len() != headers.len() {
            return Err(DataFileError::new(format!(
                "Line {line} has {} values, but the header has {}.",
                fields.len(),
                headers.len()
            )));
        }
        rows.push(
            headers
                .iter()
                .zip(fields)
                .map(|(name, value)| KeyValue::new(name.clone(), value.clone()))
                .collect(),
        );
    }

    Ok(rows)
}

/// A record and the physical line it starts on.
type CsvRecord = (usize, Vec<String>);

/// Reads RFC 4180 records: quoted fields may hold commas, line breaks and doubled quotes. Blank lines
/// and lines of nothing but commas and spaces are dropped. A leading byte-order mark is ignored.
fn read_csv_records(text: &str) -> Result<Vec<CsvRecord>, DataFileError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let chars: Vec<char> = text.chars().collect();

    let mut records: Vec<CsvRecord> = Vec::new();
    let mut fields: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut line = 1usize;
    let mut record_line = 1usize;
    let mut quote_line = 1usize;

    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_quotes {
            if c == '"' && chars.get(i + 1) == Some(&'"') {
                field.push('"');
                i += 1;
            } else if c == '"' {
                in_quotes = false;
            } else {
                if c == '\n' {
                    line += 1;
                }
                field.push(c);
            }
        } else {
            match c {
                '"' if field.is_empty() => {
                    in_quotes = true;
                    quote_line = line;
                }
                ',' => fields.push(std::mem::take(&mut field)),
                '\r' => {}
                '\n' => {
                    end_csv_record(&mut records, &mut fields, &mut field, record_line);
                    line += 1;
                    record_line = line;
                }
                _ => field.push(c),
            }
        }
        i += 1;
    }

    if in_quotes {
        return Err(DataFileError::new(format!(
            "A quoted value on line {quote_line} is never closed."
        )));
    }

    if !fields.is_empty() || !field.is_empty() {
        end_csv_record(&mut records, &mut fields, &mut field, record_line);
    }

    Ok(records)
}

fn end_csv_record(
    records: &mut Vec<CsvRecord>,
    fields: &mut Vec<String>,
    field: &mut String,
    record_line: usize,
) {
    fields.push(std::mem::take(field));
    let row = std::mem::take(fields);
    // A row of nothing but blanks is an empty row, and is skipped.
    if row.iter().any(|f| !f.trim().is_empty()) {
        records.push((record_line, row));
    }
}

fn parse_json(text: &str) -> Result<Vec<Row>, DataFileError> {
    let invalid = |e: serde_json::Error| {
        DataFileError::new(format!("The JSON file is not valid JSON: {e}"))
    };

    // Validates the whole document before its shape is checked, as JsonDocument.Parse does.
    let document: &RawValue = serde_json::from_str(text).map_err(invalid)?;
    if !document.get().starts_with('[') {
        return Err(DataFileError::new(
            "The JSON file must hold an array with one object per row.",
        ));
    }

    let elements: Vec<&RawValue> = serde_json::from_str(document.get()).map_err(invalid)?;
    let mut rows = Vec::new();
    for (index, element) in elements.iter().enumerate() {
        let raw = element.get();
        if !raw.starts_with('{') {
            return Err(DataFileError::new(format!(
                "Row {} of the JSON file is not an object.",
                index + 1
            )));
        }

        let Entries(entries): Entries = serde_json::from_str(raw).map_err(invalid)?;
        if entries.is_empty() {
            continue;
        }
        let row = entries
            .into_iter()
            .map(|(key, value)| Ok(KeyValue::new(key, value_text(value).map_err(invalid)?)))
            .collect::<Result<Row, DataFileError>>()?;
        rows.push(row);
    }

    Ok(rows)
}

/// Strings are used as written (escapes decoded). Numbers and booleans keep their exact text, null is
/// empty, and nested values are kept as JSON text.
fn value_text(raw: &RawValue) -> Result<String, serde_json::Error> {
    let text = raw.get();
    if text.starts_with('"') {
        serde_json::from_str::<String>(text)
    } else if text == "null" {
        Ok(String::new())
    } else {
        Ok(text.to_string())
    }
}

/// The members of one JSON object, in document order, with each value's raw text. Duplicate names are
/// kept, as they are in the C# EnumerateObject.
struct Entries<'a>(Vec<(String, &'a RawValue)>);

impl<'de> Deserialize<'de> for Entries<'de> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(EntriesVisitor)
    }
}

struct EntriesVisitor;

impl<'de> Visitor<'de> for EntriesVisitor {
    type Value = Entries<'de>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut entries = Vec::new();
        while let Some((key, value)) = map.next_entry::<String, &'de RawValue>()? {
            entries.push((key, value));
        }
        Ok(Entries(entries))
    }
}
