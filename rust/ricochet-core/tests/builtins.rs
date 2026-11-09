use pretty_assertions::assert_eq;
use ricochet_core::builtins::*;
use ricochet_core::models::ApiRequest;
use ricochet_core::variables::resolve;

#[test]
fn uuid_is_a_fresh_v4_each_time() {
    let first = try_evaluate("uuid.v4()").unwrap();
    let second = try_evaluate("uuid.v4()").unwrap();

    assert_eq!(first.len(), 36);
    assert_ne!(first, second);
    assert!(uuid::Uuid::parse_str(&first).is_ok());
}

#[test]
fn unix_timestamps_are_numbers() {
    let seconds: i64 = try_evaluate("timestamp.unix()").unwrap().parse().unwrap();
    let millis: i64 = try_evaluate("timestamp.unixMillis()").unwrap().parse().unwrap();

    assert!(seconds > 1_700_000_000);
    assert!(millis / 1000 >= seconds);
}

#[test]
fn iso8601_ends_with_z_and_millisecond_precision() {
    let text = try_evaluate("timestamp.iso8601()").unwrap();

    assert!(text.ends_with('Z'));
    assert_eq!(text.len(), "2026-11-09T12:00:00.000Z".len());
}

#[test]
fn format_supports_the_common_tokens() {
    let text = try_evaluate("timestamp.format('yyyy-MM-dd')").unwrap();

    assert_eq!(text.len(), 10);
    assert_eq!(text.as_bytes()[4], b'-');
    assert_eq!(text.as_bytes()[7], b'-');
}

#[test]
fn format_rejects_unsupported_tokens_so_they_stay_as_written() {
    assert_eq!(try_evaluate("timestamp.format('dddd')"), None);
}

#[test]
fn random_int_stays_inside_the_range() {
    for _ in 0..50 {
        let value: i64 = try_evaluate("random.int(3, 5)").unwrap().parse().unwrap();
        assert!((3..=5).contains(&value));
    }
}

#[test]
fn random_int_with_low_above_high_is_rejected() {
    assert_eq!(try_evaluate("random.int(9, 1)"), None);
}

#[test]
fn unknown_functions_and_bad_arguments_are_rejected() {
    assert_eq!(try_evaluate("file.read('/etc/passwd')"), None);
    assert_eq!(try_evaluate("uuid.v4('extra')"), None);
    assert_eq!(try_evaluate("random.int(1, {{x}})"), None);
    assert_eq!(try_evaluate("not a function"), None);
}

#[test]
fn postman_aliases_act_as_their_functions() {
    assert!(is_postman_alias("$guid"));
    assert!(try_evaluate("$guid").is_some());
    assert!(try_evaluate("$randomInt").is_some());
    assert!(!is_postman_alias("guid"));
}

#[test]
fn a_function_in_a_request_is_filled_in_and_shown_as_the_same_value() {
    let request = ApiRequest { url: "https://a.test/{{uuid.v4()}}".into(), ..Default::default() };
    let resolved = resolve(&request, &[]);

    let sent_id = resolved.sent.url.trim_start_matches("https://a.test/");
    let history_id = resolved.history.url.trim_start_matches("https://a.test/");
    assert!(uuid::Uuid::parse_str(sent_id).is_ok());
    assert_eq!(sent_id, history_id);
}

#[test]
fn a_user_variable_wins_over_a_function_of_the_same_name() {
    let request = ApiRequest { url: "{{uuid.v4()}}".into(), ..Default::default() };
    let variables = [ricochet_core::models::KeyValue::new("uuid.v4()", "fixed")];
    let resolved = resolve(&request, &variables);

    assert_eq!(resolved.sent.url, "fixed");
}
