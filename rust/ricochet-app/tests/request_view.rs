//! Tests for the request pane's data: URL segments, query parameters and the recent sends list.

use pretty_assertions::assert_eq;
use ricochet_app::request_view::{MAX_RECENT_SENDS, RecentSend, RecentSends, UrlSegment, query_params, url_segments};
use ricochet_core::models::KeyValue;

fn text(value: &str) -> UrlSegment {
    UrlSegment::Text(value.to_string())
}

fn chip(name: &str, resolves: bool) -> UrlSegment {
    UrlSegment::Chip { name: name.to_string(), resolves }
}

#[test]
fn a_url_is_split_around_its_placeholders() {
    let variables = [KeyValue::new("baseUrl", "http://a.test")];
    assert_eq!(
        url_segments("{{baseUrl}}/{{resource}}.json", &variables),
        vec![chip("baseUrl", true), text("/"), chip("resource", false), text(".json")]
    );
}

#[test]
fn a_url_without_placeholders_is_one_text_segment() {
    assert_eq!(url_segments("https://a.test/x", &[]), vec![text("https://a.test/x")]);
    assert_eq!(url_segments("", &[]), vec![]);
}

#[test]
fn query_parameters_are_read_as_typed() {
    assert_eq!(
        query_params("https://a.test/orders?page=2&sort={{order}}&flag#top"),
        vec![
            ("page".to_string(), "2".to_string()),
            ("sort".to_string(), "{{order}}".to_string()),
            ("flag".to_string(), String::new()),
        ]
    );
    assert_eq!(query_params("https://a.test/orders"), vec![]);
    assert_eq!(query_params("https://a.test/orders?"), vec![]);
}

fn send(name: &str) -> RecentSend {
    RecentSend { request_id: name.into(), name: name.into(), method: "GET".into(), status_code: Some(200), elapsed_ms: 1 }
}

#[test]
fn recent_sends_are_newest_first_and_capped() {
    let mut recent = RecentSends::default();
    for i in 0..MAX_RECENT_SENDS + 5 {
        recent.record(send(&format!("r{i}")));
    }
    assert_eq!(recent.entries().len(), MAX_RECENT_SENDS);
    assert_eq!(recent.entries()[0].name, format!("r{}", MAX_RECENT_SENDS + 4));
    assert_eq!(recent.entries().last().unwrap().name, "r5");
}
