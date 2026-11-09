use pretty_assertions::assert_eq;
use ricochet_core::git_status::*;

fn branch(name: &str, ahead: Option<i32>, behind: Option<i32>) -> Option<GitStatus> {
    Some(GitStatus { branch: name.to_string(), ahead, behind })
}

#[test]
fn a_branch_with_an_upstream_shows_its_counts() {
    // Fixture: upstream has 1 commit this branch lacks (behind), this branch has 2 the upstream lacks (ahead).
    let status = status_from_output(Some("main\n"), None, Some("1\t2\n"));

    assert_eq!(status, branch("main", Some(2), Some(1)));
}

#[test]
fn counts_are_behind_then_ahead() {
    assert_eq!(parse_counts("3\t0\n"), Some((3, 0)));
    assert_eq!(parse_counts("0\t7\n"), Some((0, 7)));
}

#[test]
fn a_branch_without_an_upstream_shows_its_name_and_no_counts() {
    // git rev-list fails with no upstream, so the caller passes None.
    let status = status_from_output(Some("feature\n"), None, None);

    assert_eq!(status, branch("feature", None, None));
}

#[test]
fn an_unreadable_count_keeps_the_branch_and_drops_both_counts() {
    assert_eq!(status_from_output(Some("main"), None, Some("1 2")), branch("main", None, None));
    assert_eq!(status_from_output(Some("main"), None, Some("1\t")), branch("main", None, None));
    assert_eq!(status_from_output(Some("main"), None, Some("1\t2\t3")), branch("main", None, None));
    assert_eq!(status_from_output(Some("main"), None, Some("")), branch("main", None, None));
}

#[test]
fn counts_with_a_sign_or_spaces_are_rejected() {
    // NumberStyles.None accepts digits only.
    assert_eq!(parse_counts("+1\t2"), None);
    assert_eq!(parse_counts("-1\t2"), None);
    assert_eq!(parse_counts("1\t 2"), None);
    assert_eq!(parse_counts("1a\t2"), None);
}

#[test]
fn counts_that_overflow_an_int_are_rejected() {
    assert_eq!(parse_counts("2147483647\t0"), Some((2147483647, 0)));
    assert_eq!(parse_counts("2147483648\t0"), None);
    assert_eq!(parse_counts("0\t99999999999999999999"), None);
}

#[test]
fn leading_zeros_are_fine() {
    assert_eq!(parse_counts("007\t010"), Some((7, 10)));
}

#[test]
fn surrounding_whitespace_and_empty_tab_fields_are_ignored() {
    // The C# trims the whole output and drops empty fields before counting parts.
    assert_eq!(parse_counts("\n  4\t5 \n"), Some((4, 5)));
    assert_eq!(parse_counts("\t4\t\t5\t"), Some((4, 5)));
}

#[test]
fn a_detached_head_shows_the_short_hash_and_no_counts() {
    // With HEAD detached, the caller reads the short hash and never runs rev-list.
    let status = status_from_output(Some("HEAD\n"), Some("a1b2c3d\n"), None);

    assert_eq!(status, branch("a1b2c3d", None, None));
}

#[test]
fn a_detached_head_ignores_any_counts_it_was_given() {
    let status = status_from_output(Some("HEAD"), Some("a1b2c3d"), Some("1\t2"));

    assert_eq!(status, branch("a1b2c3d", None, None));
}

#[test]
fn a_branch_named_head_prefix_is_not_detached() {
    // Only the exact name HEAD means detached.
    let status = status_from_output(Some("HEAD-fix"), None, Some("1\t2"));

    assert_eq!(status, branch("HEAD-fix", Some(2), Some(1)));
}

#[test]
fn a_failed_branch_read_gives_nothing() {
    // Not a repository, or git is missing: the C# returns null.
    assert_eq!(status_from_output(None, None, None), None);
    assert_eq!(status_from_output(None, Some("a1b2c3d"), Some("1\t2")), None);
}

#[test]
fn a_detached_head_whose_hash_failed_gives_nothing() {
    assert_eq!(status_from_output(Some("HEAD"), None, None), None);
}

#[test]
fn a_branch_name_is_trimmed() {
    let status = status_from_output(Some("  release/1.0  \n"), None, Some("0\t0\n"));

    assert_eq!(status, branch("release/1.0", Some(0), Some(0)));
}

#[test]
fn an_empty_branch_output_is_kept_as_an_empty_name_like_the_csharp() {
    // The C# only checks for null, so a successful empty answer still gives a status.
    assert_eq!(status_from_output(Some(""), None, None), branch("", None, None));
}
