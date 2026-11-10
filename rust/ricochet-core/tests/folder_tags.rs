use pretty_assertions::assert_eq;
use ricochet_core::folder_tags::*;

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn normalize_trims_drops_blanks_and_repeats_ignoring_case() {
    assert_eq!(
        normalize(Some([" smoke ", "", "SMOKE", "slow"])),
        Some(strings(&["smoke", "slow"]))
    );
}

#[test]
fn normalize_keeps_the_first_spelling() {
    assert_eq!(normalize(Some(["Smoke", "smoke", "SMOKE"])), Some(strings(&["Smoke"])));
}

#[test]
fn normalize_returns_none_when_no_tags_are_left() {
    assert_eq!(normalize(Some([" ", ""])), None);
    assert_eq!(normalize(Some(Vec::<&str>::new())), None);
}

#[test]
fn normalize_returns_none_for_absent_input() {
    assert_eq!(normalize::<[&str; 0], &str>(None), None);
}

#[test]
fn parse_splits_typed_text_on_commas() {
    assert_eq!(parse(" smoke, slow ,,"), strings(&["smoke", "slow"]));
}

#[test]
fn parse_of_blank_text_is_empty() {
    assert!(parse("  ").is_empty());
}

#[test]
fn parse_drops_repeats_ignoring_case() {
    assert_eq!(parse("Smoke, smoke, SLOW"), strings(&["Smoke", "SLOW"]));
}

#[test]
fn contains_ignores_case() {
    assert!(contains(&["Smoke"], "smoke"));
    assert!(contains(&["smoke", "slow"], "SLOW"));
    assert!(!contains(&["api"], "smoke"));
    assert!(!contains::<&str>(&[], "smoke"));
}

#[test]
fn selects_with_no_filters_selects_everything() {
    let none: &[&str] = &[];
    assert!(selects::<&str, &str, &str>(&[], none, none));
    assert!(selects(&["api"], none, none));
}

#[test]
fn selects_with_include_needs_one_of_the_tags() {
    let none: &[&str] = &[];
    assert!(selects(&["Smoke"], &["smoke"], none));
    assert!(!selects(&["api"], &["smoke"], none));
    assert!(!selects::<&str, &str, &str>(&[], &["smoke"], none));
}

#[test]
fn selects_with_exclude_rejects_any_match() {
    let none: &[&str] = &[];
    assert!(!selects(&["smoke", "slow"], none, &["SLOW"]));
    assert!(selects(&["smoke"], none, &["slow"]));
}

#[test]
fn exclude_wins_over_include() {
    assert!(!selects(&["smoke", "slow"], &["smoke"], &["SLOW"]));
}

#[test]
fn a_request_with_inherited_tags_is_selected_by_any_of_them() {
    // A Request in a tagged Folder carries the Folder's tags; the selection sees the merged list.
    let inherited = ["smoke", "api"];
    assert!(selects(&inherited, &["api"], &[] as &[&str]));
    assert!(!selects(&inherited, &["api"], &["smoke"]));
}
