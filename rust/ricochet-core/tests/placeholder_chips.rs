use pretty_assertions::assert_eq;
use ricochet_core::models::KeyValue;
use ricochet_core::placeholder_chips::*;

fn disabled(key: &str, value: &str) -> KeyValue {
    KeyValue { enabled: false, ..KeyValue::new(key, value) }
}

fn secret(key: &str, value: &str) -> KeyValue {
    KeyValue { is_secret: true, ..KeyValue::new(key, value) }
}

#[test]
fn finds_each_placeholder_with_its_position_and_name() {
    let chips = find("https://{{ host }}/users/{{id}}", &[KeyValue::new("host", "api.test"), KeyValue::new("id", "7")]);

    assert_eq!(chips.len(), 2);
    assert_eq!(chips[0], Chip { start: 8, length: 10, name: "host".into(), is_function: false, resolves: true });
    assert_eq!(chips[1], Chip { start: 25, length: 6, name: "id".into(), is_function: false, resolves: true });
}

#[test]
fn a_name_with_no_value_in_scope_does_not_resolve() {
    let chips = find("{{token}}", &[]);

    assert!(!chips[0].resolves);
}

#[test]
fn disabled_variables_and_secrets_without_a_value_do_not_resolve() {
    let variables = [disabled("off", "x"), secret("key", "")];

    let chips = find("{{off}} {{key}}", &variables);

    assert!(!chips[0].resolves);
    assert!(!chips[1].resolves);
}

#[test]
fn functions_are_chips_and_resolve_only_when_known() {
    let chips = find("{{uuid.v4()}} {{unknown.call()}} {{$guid}} {{$other}}", &[]);

    assert!(chips[0].is_function);
    assert!(chips[0].resolves);
    assert!(chips[1].is_function);
    assert!(!chips[1].resolves);
    assert!(chips[2].is_function);
    assert!(chips[2].resolves);
    assert!(!chips[3].is_function);
    assert!(!chips[3].resolves);
}

#[test]
fn a_variable_named_like_a_function_takes_priority_as_the_send_does() {
    let chips = find("{{uuid.v4()}}", &[KeyValue::new("uuid.v4()", "fixed")]);

    assert!(!chips[0].is_function);
    assert!(chips[0].resolves);
}

#[test]
fn a_placeholder_does_not_span_a_line_break() {
    let chips = find("{{a\nb}}", &[KeyValue::new("a", "1")]);

    assert!(chips.is_empty());
}

#[test]
fn find_returns_nothing_for_plain_text() {
    assert!(find("https://example.test/{x}", &[]).is_empty());
}

#[test]
fn tip_shows_a_value_but_never_a_secret() {
    let variables = [
        KeyValue::new("host", "old.test"),
        KeyValue::new("host", "api.test"),
        secret("password", "hunter2"),
    ];

    let chips = find("{{host}} {{password}} {{missing}}", &variables);

    assert_eq!(tip(&chips[0], &variables), "{{host}} = api.test");
    assert_eq!(tip(&chips[1], &variables), "{{password}}: Secret, hidden");
    assert_eq!(tip(&chips[2], &variables), "{{missing}}: no value");
}

#[test]
fn tip_describes_functions() {
    let chips = find("{{uuid.v4()}} {{unknown.call()}}", &[]);

    assert_eq!(tip(&chips[0], &[]), "{{uuid.v4()}}: built-in function, a new value on each send");
    assert_eq!(tip(&chips[1], &[]), "{{unknown.call()}}: unknown function");
}

#[test]
fn suggest_offers_variables_in_scope_then_functions_matching_what_is_typed() {
    let variables = [
        KeyValue::new("base_url", "http://x"),
        KeyValue::new("Base_token", "t"),
        disabled("off", "x"),
    ];

    let all = suggest(&variables, "");
    let bas = suggest(&variables, "BA");
    let time = suggest(&variables, "timestamp.");

    let variable_names = |s: &[ChipSuggestion]| -> Vec<String> {
        s.iter().filter(|c| !c.is_function).map(|c| c.name.clone()).collect()
    };
    assert_eq!(variable_names(&all), vec!["base_url", "Base_token"]);
    assert!(all.iter().any(|s| s.is_function && s.name == "uuid.v4()"));
    assert_eq!(variable_names(&bas), vec!["base_url", "Base_token"]);
    assert!(time.iter().all(|s| s.name.starts_with("timestamp.")));
    assert_eq!(time.len(), 4);
}

#[test]
fn suggestion_kind_names_the_source() {
    let variable = ChipSuggestion { name: "host".into(), is_function: false };
    let function = ChipSuggestion { name: "uuid.v4()".into(), is_function: true };

    assert_eq!(variable.kind(), "Variable");
    assert_eq!(function.kind(), "function");
}

#[test]
fn suggest_leaves_out_variables_that_would_not_resolve() {
    let variables = [secret("token", ""), disabled("off", "x")];

    assert!(suggest(&variables, "").iter().all(|s| s.is_function));
}

#[test]
fn caret_helpers_find_the_chip_on_each_side_of_an_offset() {
    let chips = find("a{{b}}c", &[KeyValue::new("b", "1")]);

    assert_eq!(covering(&chips, 3).map(|c| c.name.as_str()), Some("b"));
    assert_eq!(covering(&chips, 1), None);
    assert_eq!(starting_at(&chips, 1).map(|c| c.name.as_str()), Some("b"));
    assert_eq!(ending_at(&chips, 6).map(|c| c.name.as_str()), Some("b"));
    assert_eq!(ending_at(&chips, 5), None);
}
