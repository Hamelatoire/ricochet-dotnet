//! Tests for colouring a line of pretty-printed JSON.

use pretty_assertions::assert_eq;
use ricochet_app::json_color::{SpanKind, color_line};

fn kinds(line: &str) -> Vec<(SpanKind, String)> {
    color_line(line).spans.into_iter().map(|span| (span.kind, span.text)).collect()
}

#[test]
fn a_key_and_a_string_value_get_different_kinds() {
    assert_eq!(
        kinds(r#"    "item": "kettle","#),
        vec![
            (SpanKind::Key, r#""item""#.to_string()),
            (SpanKind::Plain, ": ".to_string()),
            (SpanKind::String, r#""kettle""#.to_string()),
            (SpanKind::Plain, ",".to_string()),
        ]
    );
    assert_eq!(color_line(r#"    "item": "kettle","#).indent, 4);
}

#[test]
fn numbers_and_literals_are_recognised() {
    assert_eq!(kinds("-12.5e3,")[0], (SpanKind::Number, "-12.5e3".to_string()));
    assert_eq!(kinds("true")[0], (SpanKind::Literal, "true".to_string()));
    assert_eq!(kinds("null,")[0], (SpanKind::Literal, "null".to_string()));
}

#[test]
fn an_escaped_quote_does_not_end_a_string() {
    assert_eq!(kinds(r#""a \" b""#), vec![(SpanKind::String, r#""a \" b""#.to_string())]);
}

#[test]
fn a_colon_inside_a_string_value_does_not_make_it_a_key() {
    assert_eq!(kinds(r#""url": "http://a.test""#)[2], (SpanKind::String, r#""http://a.test""#.to_string()));
}

#[test]
fn text_that_is_not_json_is_kept_whole() {
    let line = "  <html>Not Found</html>";
    let colored = color_line(line);
    let joined: String = colored.spans.iter().map(|span| span.text.as_str()).collect();
    assert_eq!(joined, line.trim_start());
    assert!(colored.spans.iter().all(|span| span.kind == SpanKind::Plain));
}

#[test]
fn an_unclosed_string_runs_to_the_end_of_the_line() {
    assert_eq!(kinds(r#""cut off"#), vec![(SpanKind::String, r#""cut off"#.to_string())]);
}
