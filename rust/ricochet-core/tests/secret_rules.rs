//! Tests for the Secret rules shared by the exporters, ported from `SecretRulesTests` in `SecretTests.cs`.
//! Only the rule that decides whether a value is a literal credential is ported here. `FindLikelyLiterals` is not
//! ported yet (see the note in `collection_export.rs`).

use pretty_assertions::assert_eq;
use ricochet_core::collection_export::is_literal;

#[test]
fn only_placeholders_are_safe_to_share() {
    let cases = [
        ("abc", true),
        ("Bearer {{token}}", true),
        ("{{token}}", false),
        (" {{a}}{{b}} ", false),
        ("", false),
    ];
    for (value, expected) in cases {
        assert_eq!(is_literal(value), expected, "is_literal({value:?})");
    }
}
