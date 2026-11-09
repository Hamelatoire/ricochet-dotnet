use pretty_assertions::assert_eq;
use ricochet_core::data_file::{DataFileFormat, Row, format_for, parse};

fn csv(text: &str) -> Result<Vec<Row>, String> {
    parse(text, DataFileFormat::Csv).map_err(|e| e.message().to_string())
}

fn json(text: &str) -> Result<Vec<Row>, String> {
    parse(text, DataFileFormat::Json).map_err(|e| e.message().to_string())
}

fn pairs(row: &Row) -> Vec<String> {
    row.iter().map(|kv| format!("{}={}", kv.key, kv.value)).collect()
}

fn ok<T, E: std::fmt::Debug>(result: Result<T, E>) -> T {
    result.expect("expected Ok")
}

fn err_message<T: std::fmt::Debug>(result: Result<T, String>) -> String {
    result.expect_err("expected an error")
}

// ---- CSV ----

#[test]
fn csv_header_names_the_variables_and_each_line_is_a_row() {
    let rows = ok(csv("name,region\nann,eu\nbob,us\n"));

    assert_eq!(rows.len(), 2);
    assert_eq!(pairs(&rows[0]), ["name=ann", "region=eu"]);
    assert_eq!(pairs(&rows[1]), ["name=bob", "region=us"]);
}

#[test]
fn csv_quoted_fields_keep_commas_doubled_quotes_and_line_breaks() {
    let rows = ok(csv("name,note\r\n\"Ann, Jr.\",\"say \"\"hi\"\"\nnext\"\r\n"));

    assert_eq!(pairs(&rows[0]), ["name=Ann, Jr.", "note=say \"hi\"\nnext"]);
}

#[test]
fn csv_blank_lines_and_comma_only_lines_are_skipped() {
    let rows = ok(csv("name,region\n\nann,eu\n  \n,\nbob,us\n\n"));

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][0].value, "ann");
    assert_eq!(rows[1][0].value, "bob");
}

#[test]
fn csv_byte_order_mark_and_spaces_around_header_names_are_ignored() {
    let rows = ok(csv("\u{feff} name , region \nann,eu"));

    assert_eq!(pairs(&rows[0]), ["name=ann", "region=eu"]);
}

#[test]
fn csv_a_line_with_the_wrong_number_of_values_names_the_line() {
    let message = err_message(csv("name,region\nann,eu\nbob\n"));

    assert!(message.contains("Line 3"), "{message}");
}

#[test]
fn csv_row_missing_a_column_refuses_the_whole_file_and_gives_both_counts() {
    let message = err_message(csv("name,region,age\nann,eu,32\nbob,us\n"));

    assert!(message.contains("Line 3"), "{message}");
    assert!(message.contains("has 2 values"), "{message}");
    assert!(message.contains("the header has 3"), "{message}");
}

#[test]
fn csv_with_an_unclosed_quote_names_the_line_it_started_on() {
    let message = err_message(csv("name\n\"ann\n"));

    assert!(message.contains("line 2"), "{message}");
}

#[test]
fn csv_with_a_repeated_header_name_is_refused() {
    let message = err_message(csv("name,name\na,b\n"));

    assert!(message.contains("name"), "{message}");
    assert!(message.contains("more than once"), "{message}");
}

#[test]
fn csv_with_no_header_is_refused() {
    let message = err_message(csv(""));
    assert!(message.contains("header"), "{message}");

    assert!(csv("\n\n").is_err());
}

#[test]
fn csv_header_with_no_name_is_refused() {
    let message = err_message(csv("name,\na,b\n"));

    assert!(message.contains("Column 2"), "{message}");
}

#[test]
fn csv_with_only_a_header_has_no_rows() {
    assert!(ok(csv("name,region\n")).is_empty());
}

#[test]
fn csv_quoted_field_with_commas_and_crlf_line_breaks_keeps_the_next_row_aligned() {
    let rows = ok(csv("name,address,age\r\n\"Ann\",\"1 Main St, Apt 2\r\nSpringfield\",32\r\nbob,\"x\",41\r\n"));

    assert_eq!(rows.len(), 2);
    assert_eq!(pairs(&rows[0]), ["name=Ann", "address=1 Main St, Apt 2\r\nSpringfield", "age=32"]);
    assert_eq!(pairs(&rows[1]), ["name=bob", "address=x", "age=41"]);
}

#[test]
fn csv_error_after_a_multi_line_quoted_value_names_the_physical_line() {
    // Line 2 is one record that spans lines 2 and 3; line 4 is short.
    let message = err_message(csv("name,note\n\"ann\",\"one\ntwo\"\nbob\n"));

    assert!(message.contains("Line 4"), "{message}");
}

#[test]
fn csv_final_record_without_a_line_break_is_kept() {
    let rows = ok(csv("name,region\nann,eu"));

    assert_eq!(pairs(&rows[0]), ["name=ann", "region=eu"]);
}

#[test]
fn csv_empty_quoted_field_is_an_empty_value() {
    let rows = ok(csv("name,region\n\"\",eu\n"));

    assert_eq!(pairs(&rows[0]), ["name=", "region=eu"]);
}

#[test]
fn csv_values_are_not_trimmed() {
    let rows = ok(csv("name,region\n  ann , eu \n"));

    assert_eq!(pairs(&rows[0]), ["name=  ann ", "region= eu "]);
}

// ---- JSON ----

#[test]
fn json_array_of_objects_gives_one_row_per_object_with_values_as_text() {
    let rows = ok(json(r#"[{"name":"ann","age":32,"active":true,"score":1.50,"note":null,"tags":{"a":1}}]"#));

    assert_eq!(pairs(&rows[0]), ["name=ann", "age=32", "active=true", "score=1.50", "note=", "tags={\"a\":1}"]);
}

#[test]
fn json_nested_values_keep_their_source_text() {
    let rows = ok(json(r#"[{"tags": [1, 2, {"x": "y"}]}]"#));

    assert_eq!(pairs(&rows[0]), ["tags=[1, 2, {\"x\": \"y\"}]"]);
}

#[test]
fn json_string_escapes_are_decoded() {
    let rows = ok(json(r#"[{"note":"say \"hi\"\nnext \u00e9"}]"#));

    assert_eq!(pairs(&rows[0]), ["note=say \"hi\"\nnext é"]);
}

#[test]
fn json_numbers_and_booleans_keep_their_exact_text_and_strings_stay_as_written() {
    let rows = ok(json(
        r#"[{"id":-12,"ratio":1e3,"big":12345678901234567890,"ok":false,"text":"true","zero":0.0}]"#,
    ));

    assert_eq!(
        pairs(&rows[0]),
        ["id=-12", "ratio=1e3", "big=12345678901234567890", "ok=false", "text=true", "zero=0.0"]
    );
}

#[test]
fn json_empty_objects_are_skipped_but_still_count_as_rows_for_errors() {
    let rows = ok(json(r#"[{"name":"ann"}, {}, {"name":"bob"}]"#));

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1][0].value, "bob");

    let message = err_message(json(r#"[{}, {"name":"ann"}, "bob"]"#));
    assert!(message.contains("Row 3"), "{message}");
}

#[test]
fn json_row_may_leave_out_a_key_and_only_that_row_lacks_it() {
    let rows = ok(json(r#"[{"name":"ann","region":"eu"},{"name":"bob"}]"#));

    assert_eq!(pairs(&rows[0]), ["name=ann", "region=eu"]);
    assert_eq!(pairs(&rows[1]), ["name=bob"]);
}

#[test]
fn json_that_is_not_valid_is_refused_with_the_reason() {
    let message = err_message(json(r#"[{"name": "ann" "#));

    assert!(message.contains("not valid JSON"), "{message}");
}

#[test]
fn json_with_trailing_text_after_the_array_is_refused_as_not_valid() {
    let message = err_message(json(r#"[{"name":"ann"}] x"#));

    assert!(message.contains("not valid JSON"), "{message}");
}

#[test]
fn json_that_is_not_an_array_is_refused() {
    let message = err_message(json(r#"{"name":"ann"}"#));

    assert!(message.contains("must hold an array"), "{message}");
}

#[test]
fn json_row_that_is_not_an_object_names_the_row() {
    let message = err_message(json(r#"[{"name":"ann"}, "bob"]"#));

    assert!(message.contains("Row 2"), "{message}");
}

#[test]
fn json_empty_file_is_refused_as_not_valid_json() {
    let message = err_message(json(""));

    assert!(message.contains("not valid JSON"), "{message}");
}

#[test]
fn json_empty_array_has_no_rows() {
    assert!(ok(json("[]")).is_empty());
}

#[test]
fn json_duplicate_names_are_kept_in_order() {
    let rows = ok(json(r#"[{"a":"1","a":"2"}]"#));

    assert_eq!(pairs(&rows[0]), ["a=1", "a=2"]);
}

// ---- Format from the file name ----

#[test]
fn format_is_taken_from_the_file_extension_ignoring_case() {
    assert_eq!(ok(format_for("users.csv")), DataFileFormat::Csv);
    assert_eq!(ok(format_for("USERS.JSON")), DataFileFormat::Json);
    assert_eq!(ok(format_for("C:\\data\\run.Csv")), DataFileFormat::Csv);
    assert_eq!(ok(format_for("data/archive.v2/users.json")), DataFileFormat::Json);
}

#[test]
fn other_file_extensions_are_refused() {
    let message = format_for("users.xlsx").expect_err("should be refused").message().to_string();

    assert!(message.contains("users.xlsx"), "{message}");
    assert!(message.contains(".csv or .json"), "{message}");
}

#[test]
fn a_file_name_with_no_extension_is_refused() {
    assert!(format_for("users").is_err());
    assert!(format_for("users.csv/readme").is_err());
}
