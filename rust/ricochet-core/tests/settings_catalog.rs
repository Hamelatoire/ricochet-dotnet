use pretty_assertions::assert_eq;
use ricochet_core::settings_catalog::*;
use std::collections::HashSet;

fn keys(items: &[&SettingItem]) -> Vec<&'static str> {
    items.iter().map(|item| item.key).collect()
}

#[test]
fn the_catalog_matches_the_csharp_settings_page() {
    assert_eq!(SECTIONS.len(), 5);
    assert_eq!(ITEMS.len(), 10);
    assert_eq!(SECTIONS[0].title, "Requests");
    assert_eq!(SECTIONS[4].key, "workspace");
}

#[test]
fn every_setting_belongs_to_a_listed_section() {
    for item in ITEMS {
        assert!(SECTIONS.contains(&item.section), "{} is in an unlisted section", item.key);
    }
}

#[test]
fn every_section_has_at_least_one_setting() {
    for section in SECTIONS {
        assert!(
            ITEMS.iter().any(|item| item.section == *section),
            "{} has no settings",
            section.key
        );
    }
}

#[test]
fn setting_keys_are_unique() {
    let unique: HashSet<&str> = ITEMS.iter().map(|item| item.key).collect();
    assert_eq!(unique.len(), ITEMS.len());
}

#[test]
fn a_blank_query_returns_every_setting_in_order() {
    let all: Vec<&str> = ITEMS.iter().map(|item| item.key).collect();

    assert_eq!(keys(&search(Some("   "))), all);
    assert_eq!(keys(&search(None)), all);
    assert_eq!(keys(&search(Some(""))), all);
}

#[test]
fn search_finds_a_setting_by_help_text_in_another_section() {
    // "certificate" is in the name of one Network setting and in the help text of Requests' TLS setting.
    let found = keys(&search(Some("certificate")));

    assert!(found.contains(&"client-certificate-file"));
    assert!(found.contains(&"verify-tls"));
}

#[test]
fn search_ignores_case() {
    assert!(keys(&search(Some("PROXY"))).contains(&"proxy-address"));
}

#[test]
fn every_word_must_match() {
    assert!(search(Some("proxy timeout")).is_empty());
}

#[test]
fn words_are_split_on_any_whitespace() {
    assert_eq!(keys(&search(Some("  wrap\tlong "))), vec!["wrap-response"]);
}

#[test]
fn search_with_no_match_returns_nothing() {
    assert!(search(Some("kubernetes")).is_empty());
}
