use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::Ordering;
use std::thread;

use pretty_assertions::assert_eq;
use ricochet_app::bulk_page::*;
use ricochet_core::bulk_runner::{BulkItemResult, parse_ids};
use ricochet_core::models::{ApiRequest, Collection, CollectionItem, KeyValue, Workspace};
use ricochet_core::personal_state::BulkProgress;
use ricochet_core::send::RequestSender;

/// A server that answers each of `count` connections with 200, or 500 for a path that ends in "bad". The paths it
/// was asked for are sent back on the channel.
fn server(count: usize) -> (u16, std::sync::mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (paths, received) = std::sync::mpsc::channel();
    thread::spawn(move || {
        for _ in 0..count {
            let Ok((mut stream, _)) = listener.accept() else { return };
            let paths = paths.clone();
            thread::spawn(move || {
                let mut buffer = [0u8; 2048];
                let read = stream.read(&mut buffer).unwrap_or(0);
                let head = String::from_utf8_lossy(&buffer[..read]).to_string();
                let path = head.lines().next().unwrap_or("").split(' ').nth(1).unwrap_or("").to_string();
                let status = if path.ends_with("bad") { "500 Internal Server Error" } else { "200 OK" };
                let _ = paths.send(path);
                let body = "ok";
                let _ = write!(stream, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            });
        }
    });
    (port, received)
}

fn workspace(port: u16) -> Workspace {
    let request = ApiRequest { id: "r".into(), name: "Get".into(), url: format!("http://127.0.0.1:{port}/items/{{{{id}}}}"), ..Default::default() };
    let item = CollectionItem { id: "item".into(), name: "Get".into(), request: Some(request), ..Default::default() };
    Workspace { collections: vec![Collection { id: "c".into(), name: "C".into(), items: vec![item], ..Default::default() }], ..Default::default() }
}

#[test]
fn one_request_is_sent_for_each_pasted_id_and_the_results_are_in_pasted_order() {
    let (port, _) = server(3);
    let ids = parse_ids("a\n\n  bad \nc\n");
    assert_eq!(ids, vec!["a", "bad", "c"]);
    let shared = BulkShared::default();
    let completed = run(&workspace(port), "item", None, &ids, "id", 1, &[], &RequestSender::new(), &shared).unwrap();
    assert!(completed);
    assert!(shared.done.load(Ordering::SeqCst));

    let results = shared.results();
    assert_eq!(results.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["a", "bad", "c"]);
    assert_eq!(results.iter().map(|r| r.succeeded).collect::<Vec<_>>(), vec![true, false, true]);
}

#[test]
fn a_request_that_is_not_in_the_workspace_cannot_start_a_run_and_still_marks_it_done() {
    let shared = BulkShared::default();
    let outcome = run(&workspace(1), "missing", None, &["a".to_string()], "id", 1, &[], &RequestSender::new(), &shared);
    assert!(outcome.is_err());
    assert!(shared.done.load(Ordering::SeqCst));
    assert!(shared.results().is_empty());
}

#[test]
fn the_ids_go_out_in_parallel_with_the_typed_placeholder_and_the_dot_env_values() {
    let (port, paths) = server(4);
    let mut workspace = workspace(port);
    let request = workspace.collections[0].items[0].request.as_mut().unwrap();
    request.url = format!("http://127.0.0.1:{port}/{{{{base}}}}/{{{{order}}}}");
    let overrides = vec![KeyValue { key: "base".into(), value: "orders".into(), enabled: true, ..Default::default() }];
    let shared = BulkShared::default();
    let completed = run(&workspace, "item", None, &ids("1\n2\nbad\n4"), " order ", 3, &overrides, &RequestSender::new(), &shared).unwrap();
    assert!(completed);

    let results = shared.results();
    assert_eq!(results.iter().map(|r| r.succeeded).collect::<Vec<_>>(), vec![true, true, false, true]);
    let mut seen: Vec<String> = paths.try_iter().collect();
    seen.sort();
    assert_eq!(seen, vec!["/orders/1", "/orders/2", "/orders/4", "/orders/bad"]);
    assert_eq!(shared.results_since(0).len(), 4);
    assert_eq!(shared.results_since(3).len(), 1);
    assert!(shared.results_since(9).is_empty());
}

fn result(position: usize, status: Option<u16>) -> BulkItemResult {
    let succeeded = status.is_some_and(|code| (200..300).contains(&code));
    BulkItemResult { id: position.to_string(), status_code: status, succeeded, detail: String::new(), elapsed_ms: 1, position }
}

fn ids(text: &str) -> Vec<String> {
    parse_ids(text)
}

#[test]
fn the_parallel_sends_setting_stays_between_1_and_16() {
    assert_eq!((MIN_THREADS, MAX_THREADS, DEFAULT_THREADS), (1, 16, 4));
    assert_eq!(clamp_threads(0), 1);
    assert_eq!(clamp_threads(-3), 1);
    assert_eq!(clamp_threads(7), 7);
    assert_eq!(clamp_threads(40), 16);
    assert_eq!(step_threads(1, -1), 1);
    assert_eq!(step_threads(4, 1), 5);
    assert_eq!(step_threads(16, 1), 16);
}

#[test]
fn a_blank_placeholder_means_id_and_spaces_are_dropped() {
    assert_eq!(placeholder_or_default(""), "id");
    assert_eq!(placeholder_or_default("   "), "id");
    assert_eq!(placeholder_or_default(" orderId "), "orderId");
}

#[test]
fn ids_from_a_file_replace_the_box_and_the_status_names_the_file() {
    assert_eq!(load_ids("a\r\n\r\n b \nc", "ids.txt"), (ids("a\nb\nc"), "Loaded 3 id(s) from ids.txt.".to_string()));
    assert_eq!(load_ids("\n  \n", "empty.txt"), (Vec::new(), "empty.txt has no ids.".to_string()));
}

#[test]
fn a_large_file_loads_every_id_and_is_kept_out_of_the_box() {
    let text: String = (0..250_000).map(|n| format!("order-{n}\r\n")).collect();
    let (loaded, status) = load_ids(&text, "big.txt");
    assert_eq!(loaded.len(), 250_000);
    assert_eq!(loaded.last().map(String::as_str), Some("order-249999"));
    assert_eq!(status, "Loaded 250000 id(s) from big.txt.");
    assert!(fits_in_box(MAX_IDS_IN_BOX));
    assert!(!fits_in_box(MAX_IDS_IN_BOX + 1));
    assert!(kept_beside_box(250_000, "big.txt").starts_with("250000 ids from big.txt."));
    // A run over the long list plans and tracks like a short one.
    let plan = plan_run(loaded, None, false).unwrap();
    let mut tracker = RunTracker::new("r", plan.ids, 0);
    tracker.record(&result(0, Some(200)));
    assert_eq!(tracker.last_id(), Some("order-0"));
}

#[test]
fn the_opening_status_and_the_hint_say_what_was_saved() {
    let saved = BulkProgress { request_id: "r".into(), ids: "a\nb".into(), last_id: Some("a".into()), processed: 1 };
    assert_eq!(opening_status(None), "No ids processed yet for this request.");
    assert_eq!(opening_status(Some(&saved)), "Last id processed: a. 1 processed so far.");
    assert_eq!(action_hint(false, None), "Start sends every id from the first one.");
    assert_eq!(action_hint(false, Some(&saved)), "Resume continues after a. Start sends every id from the first one.");
    assert_eq!(action_hint(true, Some(&saved)), "Running. Stop finishes the ids already in flight, then the run ends.");
    let started = BulkProgress { last_id: None, ..saved };
    assert_eq!(action_hint(false, Some(&started)), "Start sends every id from the first one.");
}

#[test]
fn start_begins_at_the_first_id_and_resume_after_the_saved_last_id() {
    let saved = BulkProgress { request_id: "r".into(), ids: String::new(), last_id: Some("b".into()), processed: 2 };
    let start = plan_run(ids("a\nb\nc"), Some(&saved), false).unwrap();
    assert_eq!((start.ids.len(), start.start, start.notice), (3, 0, None));
    let resume = plan_run(ids("a\nb\nc"), Some(&saved), true).unwrap();
    assert_eq!((resume.start, resume.notice), (2, None));
    assert_eq!(plan_run(ids("a\nb\nc"), None, true).unwrap().start, 0);
}

#[test]
fn resume_starts_from_the_first_id_when_the_last_id_is_not_in_the_list() {
    let saved = BulkProgress { request_id: "r".into(), ids: String::new(), last_id: Some("z".into()), processed: 2 };
    let plan = plan_run(ids("a\nb"), Some(&saved), true).unwrap();
    assert_eq!(plan.start, 0);
    assert_eq!(plan.notice.as_deref(), Some("The last id, z, is not in this list, so the run starts from the first id."));
}

#[test]
fn nothing_runs_without_ids_or_when_every_id_is_done() {
    let saved = BulkProgress { request_id: "r".into(), ids: String::new(), last_id: Some("b".into()), processed: 2 };
    assert_eq!(plan_run(ids(" \n"), None, false), Err("Paste at least one id.".to_string()));
    assert_eq!(plan_run(ids(""), Some(&saved), true), Err("Paste the ids, then resume.".to_string()));
    assert_eq!(plan_run(ids("a\nb"), Some(&saved), true), Err("Every id has already been processed.".to_string()));
}

#[test]
fn start_saves_the_ids_with_no_last_id() {
    assert_eq!(starting_progress("r", &ids("a\nb")), BulkProgress { request_id: "r".into(), ids: "a\nb".into(), last_id: None, processed: 0 });
    assert_eq!(run_header("a", 4, 2, "2026-11-10 09:00:00"), "--- Run from a, 4 thread(s), 2 id(s), 2026-11-10 09:00:00 ---");
    assert_eq!(starting_status("a", 1), "Running from a on 1 thread…");
    assert_eq!(starting_status("a", 4), "Running from a on 4 threads…");
}

#[test]
fn the_last_id_saved_is_the_end_of_the_leading_run_of_finished_ids() {
    let mut tracker = RunTracker::new("r", ids("a\nb\nc\nd"), 0);
    assert_eq!(tracker.last_id(), None);
    tracker.record(&result(1, Some(200)));
    assert_eq!(tracker.last_id(), None, "a has not finished");
    assert_eq!(tracker.processed(), 1);
    tracker.record(&result(0, Some(500)));
    assert_eq!(tracker.last_id(), Some("b"));
    tracker.record(&result(3, None));
    assert_eq!(tracker.last_id(), Some("b"), "c never finished");
    assert_eq!(tracker.progress(), BulkProgress { request_id: "r".into(), ids: "a\nb\nc\nd".into(), last_id: Some("b".into()), processed: 3 });
    assert_eq!(tracker.running_status(), "3 of 4 processed. Last id: b. Responses: 2 (1 OK, 1 failed).");
    assert_eq!(tracker.ended_status(false), "Stopped after b. Resume to continue from there. Responses: 2 (1 OK, 1 failed).");
    tracker.record(&result(2, Some(200)));
    assert_eq!(tracker.last_id(), Some("d"));
    assert_eq!(tracker.ended_status(true), "Finished. 4 of 4 processed. Responses: 3 (2 OK, 1 failed).");
}

#[test]
fn a_resumed_run_counts_the_ids_before_it_as_processed() {
    let mut tracker = RunTracker::new("r", ids("a\nb\nc"), 2);
    assert_eq!((tracker.processed(), tracker.total(), tracker.last_id()), (2, 3, Some("b")));
    // Positions count from the run's start, so position 0 is c. A repeat or an unknown position changes nothing.
    tracker.record(&result(0, Some(200)));
    tracker.record(&result(0, Some(200)));
    tracker.record(&result(5, Some(200)));
    assert_eq!((tracker.processed(), tracker.last_id()), (3, Some("c")));
    assert_eq!(tracker.ended_status(false), "Stopped after c. Resume to continue from there. Responses: 1 (1 OK, 0 failed).");
}
