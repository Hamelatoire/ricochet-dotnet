use chrono::{TimeZone, Utc};
use pretty_assertions::assert_eq;
use ricochet_core::auth_signing::*;
use ricochet_core::models::{KeyValue, RequestAuth, AuthDetails, auth_kinds};
use url::Url;

// The worked example of RFC 2617, section 3.5 (MD5, qop auth).
const RFC2617_CHALLENGE: &str =
    "realm=\"testrealm@host.com\", qop=\"auth,auth-int\", nonce=\"dcd98b7102dd2f0e8b11d0f600bfb0c093\", opaque=\"5ccc069c403ebaf9f0171e9517f40e41\"";

fn digest_auth(username: &str, password: &str) -> RequestAuth {
    RequestAuth {
        kind: auth_kinds::DIGEST.into(),
        username: username.into(),
        password: password.into(),
        ..Default::default()
    }
}

fn header_value<'a>(headers: &'a [KeyValue], key: &str) -> &'a str {
    &headers.iter().find(|h| h.key == key).unwrap_or_else(|| panic!("no {key} header")).value
}

// Encoding and query parsing

#[test]
fn encode_keeps_only_unreserved_characters() {
    assert_eq!(encode("AZaz09-._~"), "AZaz09-._~");
    assert_eq!(encode("a b/c=d&e"), "a%20b%2Fc%3Dd%26e");
}

#[test]
fn encode_escapes_utf8_bytes_in_uppercase_hex() {
    assert_eq!(encode("é"), "%C3%A9");
}

#[test]
fn parse_query_decodes_pairs_and_gives_bare_keys_an_empty_value() {
    let pairs = parse_query("?a=1&b=two+words&c&d=%2F&&e=x%3Dy");

    assert_eq!(
        pairs,
        vec![
            ("a".to_string(), "1".to_string()),
            ("b".to_string(), "two words".to_string()),
            ("c".to_string(), String::new()),
            ("d".to_string(), "/".to_string()),
            ("e".to_string(), "x=y".to_string()),
        ]
    );
}

// Digest

#[test]
fn digest_answer_matches_the_rfc_2617_example() {
    let auth = digest_auth("Mufasa", "Circle Of Life");

    let header = digest_authorization(&auth, "GET", "/dir/index.html", RFC2617_CHALLENGE, "0a4f113b").unwrap();

    assert!(header.contains("response=\"6629fae49393a05397450978507c4ef1\""), "{header}");
    assert!(header.contains("qop=auth, nc=00000001, cnonce=\"0a4f113b\""), "{header}");
    assert!(header.contains("opaque=\"5ccc069c403ebaf9f0171e9517f40e41\""), "{header}");
}

#[test]
fn digest_challenge_parsing_keeps_commas_inside_quotes() {
    let parsed = parse_digest_challenge("realm=\"a,b\", qop=\"auth,auth-int\", nonce=c, stale");

    assert_eq!(parsed.get("realm").map(String::as_str), Some("a,b"));
    assert_eq!(parsed.get("qop").map(String::as_str), Some("auth,auth-int"));
    assert_eq!(parsed.get("nonce").map(String::as_str), Some("c"));
    assert!(!parsed.contains_key("stale"));
}

#[test]
fn digest_challenge_parsing_ignores_name_case() {
    let parsed = parse_digest_challenge("Realm=\"r\", NONCE=n");

    assert_eq!(parsed.get("realm").map(String::as_str), Some("r"));
    assert_eq!(parsed.get("nonce").map(String::as_str), Some("n"));
}

#[test]
fn digest_refuses_an_unsupported_algorithm() {
    let auth = digest_auth("u", "p");

    let err = digest_authorization(&auth, "GET", "/", "realm=\"r\", nonce=\"n\", algorithm=SHA-512-256", "cn").unwrap_err();

    assert!(err.message().contains("algorithm"), "{err}");
}

#[test]
fn digest_refuses_a_qop_without_auth() {
    let auth = digest_auth("u", "p");

    let err = digest_authorization(&auth, "GET", "/", "realm=\"r\", nonce=\"n\", qop=\"auth-int\"", "cn").unwrap_err();

    assert!(err.message().contains("qop"), "{err}");
}

#[test]
fn digest_needs_a_realm_and_a_nonce() {
    let auth = digest_auth("u", "p");

    let err = digest_authorization(&auth, "GET", "/", "realm=\"r\"", "cn").unwrap_err();

    assert!(err.message().contains("no realm or nonce"), "{err}");
}

#[test]
fn digest_without_qop_omits_the_qop_fields() {
    let auth = digest_auth("u", "p");

    let header = digest_authorization(&auth, "GET", "/", "realm=\"r\", nonce=\"n\"", "cn").unwrap();

    assert!(!header.contains("qop="), "{header}");
    assert!(!header.contains("cnonce="), "{header}");
    assert!(header.contains("algorithm=MD5"), "{header}");
}

// OAuth 1.0

fn twitter_oauth() -> RequestAuth {
    RequestAuth {
        kind: auth_kinds::OAUTH1.into(),
        token: "370773112-GmHxMAgYyLbNEtIKZeRNFsMKPR9EyMZeS9weJAEb".into(),
        details: Some(AuthDetails {
            consumer_key: "xvz1evFS4wEEPTGEFPHBog".into(),
            consumer_secret: "kAcSOqF21Fu85e7zjz7ZN2U4ZRhfV3WpwPAoE3Z7kBw".into(),
            token_secret: "LswwdoUaIvS8ltyTt5jkRh4J50vUPVVHtR2YPi5kE".into(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn oauth1_signature_matches_the_twitter_example() {
    // The worked example in Twitter's OAuth documentation, with its fixed nonce and timestamp.
    let body = vec![("status".to_string(), "Hello Ladies + Gentlemen, a signed OAuth request!".to_string())];
    let uri = Url::parse("https://api.twitter.com/1/statuses/update.json?include_entities=true").unwrap();

    let header = oauth1_authorization(&twitter_oauth(), "POST", &uri, &body, "kYjzVBB8Y0ZFabxSWbWovY3uYSQ2pTgmZeNu2VS4cg", 1318622958)
        .unwrap();

    assert!(header.starts_with("OAuth "), "{header}");
    assert!(header.contains("oauth_signature=\"tnnArxj06cWHq44gCs1OSKk%2FjLY%3D\""), "{header}");
    assert!(header.contains("oauth_consumer_key=\"xvz1evFS4wEEPTGEFPHBog\""), "{header}");
}

#[test]
fn oauth1_header_never_contains_the_secrets() {
    let auth = RequestAuth {
        kind: auth_kinds::OAUTH1.into(),
        token: "tok".into(),
        details: Some(AuthDetails {
            consumer_key: "ck".into(),
            consumer_secret: "consumer-secret-value".into(),
            token_secret: "token-secret-value".into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    let uri = Url::parse("https://x.test/a").unwrap();

    let header = oauth1_authorization(&auth, "GET", &uri, &[], "n", 1).unwrap();

    assert!(!header.contains("consumer-secret-value"), "{header}");
    assert!(!header.contains("token-secret-value"), "{header}");
}

#[test]
fn oauth1_needs_a_consumer_key() {
    let auth = RequestAuth { kind: auth_kinds::OAUTH1.into(), ..Default::default() };
    let uri = Url::parse("https://x.test/").unwrap();

    let err = oauth1_authorization(&auth, "GET", &uri, &[], "n", 1).unwrap_err();

    assert_eq!(err.message(), "Enter the OAuth 1.0 consumer key.");
}

// AWS Signature Version 4

fn aws_auth(region: &str, service: &str) -> RequestAuth {
    RequestAuth {
        kind: auth_kinds::AWS_SIG_V4.into(),
        details: Some(AuthDetails {
            aws_access_key_id: "AKIDEXAMPLE".into(),
            aws_secret_access_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".into(),
            aws_region: region.into(),
            aws_service: service.into(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn aws_sigv4_signature_matches_the_get_vanilla_example() {
    // The get-vanilla test case of the AWS Signature Version 4 test suite.
    let uri = Url::parse("https://example.amazonaws.com/").unwrap();
    let now = Utc.with_ymd_and_hms(2015, 8, 30, 12, 36, 0).unwrap();

    let headers = aws_sig_v4_headers(&aws_auth("us-east-1", "service"), "GET", &uri, &[], now).unwrap();

    assert_eq!(header_value(&headers, "X-Amz-Date"), "20150830T123600Z");
    assert_eq!(
        header_value(&headers, "Authorization"),
        "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, SignedHeaders=host;x-amz-date, Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31",
    );
    assert!(!headers.iter().any(|h| h.key == "X-Amz-Security-Token"));
}

#[test]
fn aws_sigv4_adds_the_session_token_header_when_there_is_one() {
    let mut auth = aws_auth("eu-west-1", "execute-api");
    let details = auth.details.as_mut().unwrap();
    details.aws_access_key_id = "AKID".into();
    details.aws_secret_access_key = "secret".into();
    details.aws_session_token = "session".into();
    let uri = Url::parse("https://x.test/items?b=2&a=1").unwrap();
    let now = Utc.with_ymd_and_hms(2024, 1, 2, 3, 4, 5).unwrap();

    let headers = aws_sig_v4_headers(&auth, "POST", &uri, b"{}", now).unwrap();

    assert_eq!(header_value(&headers, "X-Amz-Security-Token"), "session");
    assert!(
        header_value(&headers, "Authorization").contains("SignedHeaders=host;x-amz-date;x-amz-security-token"),
        "{headers:?}"
    );
}

#[test]
fn aws_sigv4_keeps_a_non_default_port_in_the_host() {
    let uri = Url::parse("https://example.amazonaws.com:8443/").unwrap();
    let now = Utc.with_ymd_and_hms(2015, 8, 30, 12, 36, 0).unwrap();

    let with_port = aws_sig_v4_headers(&aws_auth("us-east-1", "service"), "GET", &uri, &[], now).unwrap();
    let without_port = aws_sig_v4_headers(
        &aws_auth("us-east-1", "service"),
        "GET",
        &Url::parse("https://example.amazonaws.com/").unwrap(),
        &[],
        now,
    )
    .unwrap();

    assert_ne!(header_value(&with_port, "Authorization"), header_value(&without_port, "Authorization"));
}

#[test]
fn aws_sigv4_needs_the_access_key_and_secret() {
    let auth = RequestAuth {
        kind: auth_kinds::AWS_SIG_V4.into(),
        details: Some(AuthDetails { aws_region: "us-east-1".into(), aws_service: "s3".into(), ..Default::default() }),
        ..Default::default()
    };
    let uri = Url::parse("https://x.test/").unwrap();

    let err = aws_sig_v4_headers(&auth, "GET", &uri, &[], Utc::now()).unwrap_err();

    assert_eq!(err.message(), "Enter the AWS access key id and secret access key.");
}

#[test]
fn aws_sigv4_needs_region_and_service() {
    let uri = Url::parse("https://x.test/").unwrap();

    for (region, service) in [("", "execute-api"), ("us-east-1", "")] {
        let mut auth = aws_auth(region, service);
        let details = auth.details.as_mut().unwrap();
        details.aws_access_key_id = "a".into();
        details.aws_secret_access_key = "b".into();

        let err = aws_sig_v4_headers(&auth, "GET", &uri, &[], Utc::now()).unwrap_err();

        assert_eq!(err.message(), "Enter the AWS region and service.", "region={region:?} service={service:?}");
    }
}
