use pretty_assertions::assert_eq;
use ricochet_core::response_status::{ResponseStatusClass, classify, suggested_body_file_name};

#[test]
fn classifies_status_codes_by_family() {
    let cases = [
        (100, ResponseStatusClass::Neutral),
        (101, ResponseStatusClass::Neutral),
        (200, ResponseStatusClass::Success),
        (204, ResponseStatusClass::Success),
        (299, ResponseStatusClass::Success),
        (301, ResponseStatusClass::Redirect),
        (304, ResponseStatusClass::Redirect),
        (399, ResponseStatusClass::Redirect),
        (400, ResponseStatusClass::ClientError),
        (404, ResponseStatusClass::ClientError),
        (499, ResponseStatusClass::ClientError),
        (500, ResponseStatusClass::ServerError),
        (503, ResponseStatusClass::ServerError),
        (599, ResponseStatusClass::ServerError),
        (600, ResponseStatusClass::Neutral),
        (0, ResponseStatusClass::Neutral),
    ];

    for (status_code, expected) in cases {
        assert_eq!(expected, classify(status_code), "status {status_code}");
    }
}

#[test]
fn suggests_a_file_name_from_the_media_type() {
    let cases = [
        (Some("application/json"), "response.json"),
        (Some("application/json; charset=utf-8"), "response.json"),
        (Some("Application/JSON"), "response.json"),
        (Some("application/problem+json"), "response.json"),
        (Some("application/xml"), "response.xml"),
        (Some("text/xml"), "response.xml"),
        (Some("image/svg+xml"), "response.svg"),
        (Some("text/html"), "response.html"),
        (Some("text/plain"), "response.txt"),
        (Some("text/csv"), "response.csv"),
        (Some("application/pdf"), "response.pdf"),
        (Some("image/jpeg"), "response.jpg"),
        (Some("application/octet-stream"), "response.bin"),
        (Some(""), "response.bin"),
        (None, "response.bin"),
    ];

    for (media_type, expected) in cases {
        assert_eq!(expected, suggested_body_file_name(media_type), "media type {media_type:?}");
    }
}
