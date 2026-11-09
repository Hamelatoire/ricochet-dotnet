use pretty_assertions::assert_eq;
use ricochet_core::key_value_columns::*;

const EPS: f64 = 1e-10;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < EPS
}

#[test]
fn share_inside_the_range_is_kept() {
    assert_eq!(clamp(0.4, 600.0), 0.4);
}

#[test]
fn key_column_cannot_shrink_below_the_minimum_width() {
    // 80 of 600 is 13.3%: a smaller share is raised to that.
    assert!(close(clamp(0.01, 600.0), 80.0 / 600.0));
}

#[test]
fn value_column_cannot_shrink_below_the_minimum_width() {
    assert!(close(clamp(0.99, 600.0), 1.0 - 80.0 / 600.0));
}

#[test]
fn non_finite_share_gives_the_default() {
    assert_eq!(clamp(f64::NAN, 600.0), DEFAULT_KEY_SHARE);
    assert_eq!(clamp(f64::INFINITY, 600.0), DEFAULT_KEY_SHARE);
    assert_eq!(clamp(f64::NEG_INFINITY, 600.0), DEFAULT_KEY_SHARE);
}

#[test]
fn non_finite_total_width_gives_the_default() {
    assert_eq!(clamp(0.5, f64::NAN), DEFAULT_KEY_SHARE);
    assert_eq!(clamp(0.5, f64::INFINITY), DEFAULT_KEY_SHARE);
}

#[test]
fn space_too_small_for_both_minimums_gives_the_default() {
    assert_eq!(clamp(0.5, 100.0), DEFAULT_KEY_SHARE);
}

#[test]
fn space_exactly_fitting_both_minimums_is_used() {
    // 2 * 80 = 160: the only valid share is exactly one half.
    assert!(close(clamp(0.3, 160.0), 0.5));
}

#[test]
fn only_proportions_between_zero_and_one_are_saveable() {
    assert!(!is_saveable(0.0));
    assert!(!is_saveable(1.0));
    assert!(!is_saveable(-0.2));
    assert!(!is_saveable(f64::NAN));
    assert!(!is_saveable(f64::INFINITY));
    assert!(is_saveable(0.5));
}

#[test]
fn default_key_share_is_one_third() {
    assert!(close(DEFAULT_KEY_SHARE, 1.0 / 3.0));
    assert!(is_saveable(DEFAULT_KEY_SHARE));
}
