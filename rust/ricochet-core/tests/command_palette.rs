use pretty_assertions::assert_eq;
use ricochet_core::command_palette::filter;

const NAMES: [&str; 5] = [
    "Send request",
    "Save request",
    "New request",
    "Get hello",
    "Import Postman files",
];

#[test]
fn an_empty_query_keeps_every_entry_in_order() {
    let result = filter(NAMES, "   ", |n| n);

    assert_eq!(result, NAMES.to_vec());
}

#[test]
fn matching_ignores_case_and_needs_every_word() {
    assert!(filter(NAMES, "REQUEST hello", |n| n).is_empty());
    assert_eq!(filter(NAMES, "get HELLO", |n| n), vec!["Get hello"]);
}

#[test]
fn names_that_start_with_the_first_word_come_first() {
    let result = filter(NAMES, "request", |n| n);

    assert_eq!(result, vec!["Send request", "Save request", "New request"]);
}

#[test]
fn a_name_that_starts_with_the_first_word_ranks_above_one_that_only_contains_it() {
    let result = filter(["Open a request", "Request list"], "request", |n| n);

    assert_eq!(result, vec!["Request list", "Open a request"]);
}

#[test]
fn a_query_with_no_match_gives_nothing() {
    assert!(filter(NAMES, "delete", |n| n).is_empty());
}

#[test]
fn entries_other_than_strings_are_filtered_by_their_name() {
    struct Command {
        name: &'static str,
        id: u32,
    }

    let commands = vec![
        Command { name: "Open a request", id: 1 },
        Command { name: "Request list", id: 2 },
    ];
    let result = filter(commands, "request", |c| c.name);

    assert_eq!(result.iter().map(|c| c.id).collect::<Vec<_>>(), vec![2, 1]);
}
