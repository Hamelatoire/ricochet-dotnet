use pretty_assertions::assert_eq;
use ricochet_core::name_filter::{matches, normalize, shows};

#[test]
fn empty_or_blank_query_matches_everything() {
    assert!(matches("Anything", ""));
    assert!(matches("Anything", "   "));
}

#[test]
fn matches_a_substring_ignoring_case_and_surrounding_spaces() {
    assert!(matches("Get Users", "users"));
    assert!(matches("Get Users", "  GET "));
    assert!(!matches("Get Users", "orders"));
}

#[test]
fn normalize_trims_the_query() {
    assert_eq!(normalize("  users \t"), "users");
    assert_eq!(normalize("   "), "");
}

#[test]
fn a_node_is_shown_when_a_child_is_shown_even_if_its_own_name_does_not_match() {
    assert!(shows("Shop", "users", [false, true, false]));
    assert!(!shows("Shop", "users", [false, false]));
    assert!(!shows("Shop", "users", [false; 0]));
}

#[test]
fn a_matching_name_is_shown_without_any_child() {
    assert!(shows("Users", "users", [false; 0]));
}

#[test]
fn children_are_checked_lazily_and_not_at_all_when_the_name_matches() {
    let mut checked = 0;
    let shown = shows(
        "Users",
        "users",
        std::iter::from_fn(|| {
            checked += 1;
            Some(true)
        }),
    );
    assert!(shown);
    assert_eq!(checked, 0);

    let mut checked = 0;
    let shown = shows(
        "Shop",
        "users",
        std::iter::from_fn(|| {
            checked += 1;
            Some(checked == 2)
        }),
    );
    assert!(shown);
    assert_eq!(checked, 2, "stops at the first child shown");
}

#[test]
fn off_filter_shows_every_node() {
    assert!(shows("Shop", "", [false; 0]));
}
