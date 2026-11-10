use pretty_assertions::assert_eq;
use ricochet_app::exports::{ExportFormat, export};
use ricochet_app::runner_results::{failing_labels, rows};
use ricochet_core::assertions::AssertionResult;
use ricochet_core::collection_runner::{RequestRunResult, RunReport};
use ricochet_core::models::{ApiRequest, Collection, CollectionItem};

fn collection() -> Collection {
    let request = CollectionItem { id: "r".into(), name: "Status".into(), request: Some(ApiRequest { name: "Status".into(), url: "https://a.test/status".into(), ..Default::default() }), ..Default::default() };
    Collection { id: "c".into(), name: "Shop API".into(), items: vec![request], ..Default::default() }
}

#[test]
fn a_collection_exports_to_postman_and_openapi_json() {
    let postman = export(&collection(), ExportFormat::Postman);
    assert!(postman.json.contains("\"info\""), "{}", postman.json);
    assert!(postman.json.contains("https://a.test/status"));
    let openapi = export(&collection(), ExportFormat::OpenApi);
    assert!(openapi.json.contains("\"openapi\""), "{}", openapi.json);
}

#[test]
fn the_suggested_file_name_follows_the_collection_name() {
    let mut weird = collection();
    weird.name = "Shop / API!".into();
    assert_eq!(ExportFormat::Postman.suggested_file_name(&collection()), "Shop-API.postman_collection.json");
    assert_eq!(ExportFormat::OpenApi.suggested_file_name(&weird), "Shop-API.openapi.json");
    weird.name = "!!!".into();
    assert_eq!(ExportFormat::OpenApi.suggested_file_name(&weird), "collection.openapi.json");
}

fn result(path: &str, error: Option<&str>, checks: Vec<AssertionResult>) -> RequestRunResult {
    RequestRunResult {
        collection: "Shop API".into(),
        path: path.into(),
        method: "GET".into(),
        status_code: Some(200),
        elapsed_ms: 3,
        assertions: checks,
        error: error.map(str::to_string),
        has_scripts: false,
        captured: vec![],
    }
}

fn check(passed: bool) -> AssertionResult {
    AssertionResult { description: "status is 200".into(), passed, message: if passed { "ok".into() } else { "got 500".into() } }
}

#[test]
fn each_request_gets_a_row_with_its_first_problem() {
    let report = RunReport {
        results: vec![
            result("Status", None, vec![check(true)]),
            result("Orders", None, vec![check(false)]),
            result("Create", Some("connection refused"), vec![]),
        ],
        stopped: false,
        skipped_hidden: vec![],
        certificates_unverified: false,
    };
    let shown = rows(&report);
    assert_eq!(shown[0].passed, true);
    assert_eq!(shown[0].detail, "");
    assert_eq!(shown[1].detail, "status is 200: got 500");
    assert_eq!(shown[2].detail, "connection refused");
    assert_eq!(failing_labels(&report), vec!["GET Orders".to_string(), "GET Create".to_string()]);
}
