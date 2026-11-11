//! The panic hook writes a crash log without the panic message. This file is its own test binary, because the hook is
//! global to the process.

use std::fs;

use ricochet_app::crash_report;
use ricochet_core::crash_log;

#[test]
fn a_panic_writes_a_log_without_its_message() {
    let personal = std::env::temp_dir().join(format!("ricochet-crash-report-{}", std::process::id()));
    let _ = fs::remove_dir_all(&personal);
    crash_report::install(Some(personal.clone()));

    let result = std::thread::Builder::new()
        .name("worker".into())
        .spawn(|| panic!("failed for https://example.test/?token=hunter2"))
        .unwrap()
        .join();
    assert!(result.is_err());

    let logs: Vec<_> = fs::read_dir(crash_log::directory_for(&personal)).unwrap().map(|e| e.unwrap().path()).collect();
    assert_eq!(logs.len(), 1);
    let text = fs::read_to_string(&logs[0]).unwrap();
    let _ = fs::remove_dir_all(&personal);

    assert!(text.starts_with("Ricochet crash log\n"));
    assert!(text.contains("panic on thread 'worker'"));
    assert!(text.contains(&format!("App version: {}", crash_report::APP_VERSION)));
    assert!(text.contains("tests"), "the location is kept: {text}");
    assert!(!text.contains("hunter2"));
    assert!(!text.contains("example.test"));
}
