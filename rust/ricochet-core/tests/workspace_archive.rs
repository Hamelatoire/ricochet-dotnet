//! Tests for the workspace archive, ported from `WorkspaceArchiveTests.cs`, plus the Secret and zip-bomb cases the port
//! adds. Each test works in its own temporary folder, which is removed afterwards.

use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use pretty_assertions::assert_eq;
use ricochet_core::assertions::{ops, sources, RequestAssertion, RequestCapture};
use ricochet_core::models::*;
use ricochet_core::store::{GITIGNORE_CONTENT, WorkspaceFolder, WorkspaceIssue};
use ricochet_core::workspace_archive::*;
use zip::write::SimpleFileOptions;

const MIB: u64 = 1024 * 1024;

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("ricochet-archive-test-{tag}-{}", new_id()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn source(&self) -> PathBuf {
        self.0.join("source")
    }

    fn zip(&self) -> PathBuf {
        self.0.join("workspace.zip")
    }

    fn target(&self) -> PathBuf {
        self.0.join("imported")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn request(name: &str, id: &str) -> ApiRequest {
    ApiRequest { id: id.to_string(), name: name.to_string(), url: format!("http://x.test/{name}"), ..ApiRequest::default() }
}

fn request_item(name: &str, id: &str) -> CollectionItem {
    CollectionItem { id: id.to_string(), name: name.to_string(), request: Some(request(name, &format!("r{id}"))), ..CollectionItem::default() }
}

fn expected_assertion() -> RequestAssertion {
    RequestAssertion { source: sources::STATUS.to_string(), op: ops::EQUAL.to_string(), expected: "200".to_string(), ..RequestAssertion::default() }
}

fn expected_capture() -> RequestCapture {
    RequestCapture { source: sources::JSON.to_string(), path: "$.id".to_string(), variable: "newId".to_string(), ..RequestCapture::default() }
}

/// A Workspace with a typed assertion and capture on one Request, so the round trip covers both.
fn sample() -> Workspace {
    let mut zebra = request_item("Zebra", "i1aaaaaaaa");
    if let Some(request) = zebra.request.as_mut() {
        request.assertions = Some(vec![expected_assertion()]);
        request.captures = Some(vec![expected_capture()]);
    }

    let orders = CollectionItem {
        id: "f1aaaaaaaa".into(),
        name: "Orders".into(),
        children: vec![request_item("Create", "i2aaaaaaaa")],
        ..CollectionItem::default()
    };

    Workspace {
        collections: vec![Collection {
            id: "c1aaaaaaaa".into(),
            name: "Shop API".into(),
            variables: vec![KeyValue::new("base", "http://x.test")],
            items: vec![zebra, orders],
            ..Collection::default()
        }],
        environments: vec![ApiEnvironment { id: "e1aaaaaaaa".into(), name: "Staging".into(), variables: vec![KeyValue::new("a", "1")] }],
        globals: vec![KeyValue::new("g", "1")],
        ..Workspace::default()
    }
}

/// Saves a Workspace into a folder through the store, as the app does.
fn save_into(root: &Path, workspace: &Workspace) {
    let mut folder = WorkspaceFolder::new(root);
    folder.load().unwrap();
    let left_alone = folder.save(workspace).unwrap();
    assert!(left_alone.is_empty(), "{left_alone:?}");
}

fn save_sample(root: &Path) {
    save_into(root, &sample());
}

/// Loads a folder through the store, returning the Workspace and any issues.
fn load(root: &Path) -> (Workspace, Vec<WorkspaceIssue>) {
    WorkspaceFolder::new(root).load().unwrap()
}

/// Writes a ZIP with exactly these entries. Names are written as given, so unsafe ones can be tested.
fn make_zip(path: &Path, entries: &[(&str, &str)]) {
    let raw: Vec<(&str, Vec<u8>)> = entries.iter().map(|(name, content)| (*name, content.as_bytes().to_vec())).collect();
    make_zip_bytes(path, &raw);
}

fn make_zip_bytes(path: &Path, entries: &[(&str, Vec<u8>)]) {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default();
    for (name, content) in entries {
        zip.start_file(*name, options).unwrap();
        zip.write_all(content).unwrap();
    }
    let bytes = zip.finish().unwrap().into_inner();
    fs::write(path, bytes).unwrap();
}

/// The entry names in an archive, sorted.
fn zip_names(path: &Path) -> Vec<String> {
    let mut archive = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut names: Vec<String> = (0..archive.len()).map(|i| archive.by_index(i).unwrap().name().unwrap().to_string()).collect();
    names.sort();
    names
}

/// Every entry's text, so a test can check that a value appears nowhere in the archive.
fn zip_texts(path: &Path) -> Vec<String> {
    let mut archive = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut texts = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut bytes).unwrap();
        texts.push(String::from_utf8_lossy(&bytes).into_owned());
    }
    texts
}

const VALID_MARKER: &str = "{\"id\":\"w1aaaaaaaa\"}";

/// Problems that start with the given entry name.
fn problems_for<'a>(result: &'a WorkspaceArchiveResult, name: &str) -> Vec<&'a String> {
    result.problems.iter().filter(|problem| problem.starts_with(name)).collect()
}

#[test]
fn export_then_import_gives_back_the_same_workspace() {
    let dir = TempDir::new("roundtrip");
    save_sample(&dir.source());

    let exported = export(&dir.source(), &dir.zip());
    let imported = import(&dir.zip(), &dir.target());

    assert!(exported.succeeded(), "{:?}", exported.problems);
    assert!(imported.succeeded(), "{:?}", imported.problems);
    assert_eq!(exported.files, imported.files);

    let (original, original_issues) = load(&dir.source());
    let (restored, restored_issues) = load(&dir.target());
    assert!(original_issues.is_empty() && restored_issues.is_empty());
    assert_eq!(restored, original);

    let zebra = restored.collections[0].items[0].request.as_ref().unwrap();
    assert_eq!(zebra.assertions, Some(vec![expected_assertion()]));
    assert_eq!(zebra.captures, Some(vec![expected_capture()]));
    assert_eq!(fs::read_to_string(dir.target().join(".gitignore")).unwrap(), GITIGNORE_CONTENT);
}

#[test]
fn export_writes_only_the_shareable_layout() {
    let dir = TempDir::new("layout");
    save_sample(&dir.source());

    let result = export(&dir.source(), &dir.zip());

    assert!(result.succeeded(), "{:?}", result.problems);
    assert_eq!(
        zip_names(&dir.zip()),
        [
            "collections/shop-api-c1aaaaaa/collection.json",
            "collections/shop-api-c1aaaaaa/orders-f1aaaaaa/create-i2aaaaaa.json",
            "collections/shop-api-c1aaaaaa/orders-f1aaaaaa/folder.json",
            "collections/shop-api-c1aaaaaa/zebra-i1aaaaaa.json",
            "environments/staging-e1aaaaaa.json",
            "globals.json",
            "ricochet.json",
        ]
    );
    assert_eq!(result.files, 7);
    assert!(!dir.zip().with_extension("zip.tmp").exists());
}

#[test]
fn export_leaves_out_personal_files_history_secrets_and_ignored_patterns() {
    let dir = TempDir::new("personal");
    save_sample(&dir.source());
    fs::write(dir.source().join("state.json"), "{\"theme\":\"personal-marker\"}").unwrap();
    fs::write(dir.source().join("history.json"), "[\"personal-marker\"]").unwrap();
    fs::write(dir.source().join("collections").join("history.json"), "personal-marker").unwrap();
    fs::write(dir.source().join("secrets.dat"), "personal-marker").unwrap();
    fs::write(dir.source().join(".env"), "TOKEN=personal-marker").unwrap();
    fs::write(dir.source().join("notes.secret"), "personal-marker").unwrap();
    fs::write(dir.source().join("scratch.tmp"), "personal-marker").unwrap();

    let result = export(&dir.source(), &dir.zip());

    assert!(result.succeeded(), "{:?}", result.problems);
    let names = zip_names(&dir.zip());
    for name in &names {
        assert!(!name.contains("state") && !name.contains("history") && !name.contains("secret"), "{name}");
        assert!(!name.contains(".env") && !name.ends_with(".tmp") && !name.ends_with(".gitignore"), "{name}");
    }
    assert!(names.contains(&"ricochet.json".to_string()));
    assert!(names.contains(&"globals.json".to_string()));
    assert!(zip_texts(&dir.zip()).iter().all(|text| !text.contains("personal-marker")));
}

#[test]
fn export_never_includes_secret_values_when_the_shared_copy_has_none() {
    let dir = TempDir::new("secret-split");
    let mut workspace = sample();
    // What the Secret split leaves in the shared folder: the flag stays, the value does not.
    workspace.globals = vec![KeyValue { key: "token".into(), value: String::new(), is_secret: true, ..KeyValue::default() }];
    save_into(&dir.source(), &workspace);
    fs::write(dir.source().join("secrets.dat"), "hunter2-secret").unwrap();
    fs::write(dir.source().join(".env"), "TOKEN=hunter2-secret").unwrap();
    fs::write(dir.source().join("notes.secret"), "hunter2-secret").unwrap();

    let exported = export(&dir.source(), &dir.zip());
    let imported = import(&dir.zip(), &dir.target());

    assert!(exported.succeeded(), "{:?}", exported.problems);
    assert!(imported.succeeded(), "{:?}", imported.problems);
    assert!(zip_texts(&dir.zip()).iter().all(|text| !text.contains("hunter2-secret")));
    let (restored, _) = load(&dir.target());
    assert_eq!(restored.globals, vec![KeyValue { key: "token".into(), value: String::new(), is_secret: true, ..KeyValue::default() }]);
}

#[test]
fn export_refuses_a_shared_file_that_holds_a_secret_value_and_writes_no_archive() {
    let dir = TempDir::new("secret-leak");
    let mut workspace = sample();
    workspace.globals = vec![KeyValue { key: "token".into(), value: "hunter2-secret".into(), is_secret: true, ..KeyValue::default() }];
    save_into(&dir.source(), &workspace);

    let result = export(&dir.source(), &dir.zip());

    assert!(!result.succeeded());
    assert!(result.problems.iter().any(|problem| problem.contains("token")));
    assert!(result.problems.iter().all(|problem| !problem.contains("hunter2-secret")));
    assert!(!dir.zip().exists());
}

#[test]
fn import_refuses_an_archive_whose_shared_file_holds_a_secret_value() {
    let dir = TempDir::new("secret-import");
    make_zip(
        &dir.zip(),
        &[
            ("ricochet.json", VALID_MARKER),
            ("globals.json", "[{\"key\":\"token\",\"value\":\"hunter2-secret\",\"enabled\":true,\"isSecret\":true}]"),
        ],
    );

    let result = import(&dir.zip(), &dir.target());

    assert!(!result.succeeded());
    assert!(result.problems.iter().all(|problem| !problem.contains("hunter2-secret")));
    assert!(!dir.target().exists());
}

#[test]
fn export_refuses_a_shared_file_it_cannot_read_and_writes_no_archive() {
    let dir = TempDir::new("unreadable");
    save_sample(&dir.source());
    fs::write(dir.source().join("collections").join("shop-api-c1aaaaaa").join("broken-r9.json"), "{ not json").unwrap();

    let result = export(&dir.source(), &dir.zip());

    assert!(!result.succeeded());
    assert!(result.problems.iter().any(|problem| problem.contains("broken-r9.json")));
    assert!(!dir.zip().exists());
}

#[test]
fn export_refuses_a_folder_that_is_not_a_workspace() {
    let dir = TempDir::new("not-workspace");
    fs::create_dir_all(dir.source()).unwrap();

    let result = export(&dir.source(), &dir.zip());

    assert!(!result.succeeded());
    assert!(result.problems.iter().any(|problem| problem.contains("ricochet.json")));
    assert!(!dir.zip().exists());
}

#[test]
fn export_replaces_an_existing_archive() {
    let dir = TempDir::new("replace");
    save_sample(&dir.source());
    fs::write(dir.zip(), "not a zip").unwrap();

    let result = export(&dir.source(), &dir.zip());

    assert!(result.succeeded(), "{:?}", result.problems);
    assert!(zip_names(&dir.zip()).contains(&"ricochet.json".to_string()));
    assert!(!PathBuf::from(format!("{}.tmp", dir.zip().display())).exists());
}

#[test]
fn import_keeps_the_workspace_id() {
    let dir = TempDir::new("id");
    save_sample(&dir.source());
    let mut source = WorkspaceFolder::new(dir.source());
    source.load().unwrap();
    let source_id = source.id().to_string();

    let exported = export(&dir.source(), &dir.zip());
    let result = import(&dir.zip(), &dir.target());

    assert!(exported.succeeded(), "{:?}", exported.problems);
    assert!(result.succeeded(), "{:?}", result.problems);
    let mut restored = WorkspaceFolder::new(dir.target());
    restored.load().unwrap();
    assert_eq!(restored.id(), source_id);
}

#[test]
fn import_writes_into_a_new_folder_the_same_gitignore_a_new_workspace_gets() {
    let dir = TempDir::new("gitignore");
    make_zip(&dir.zip(), &[("ricochet.json", VALID_MARKER)]);

    let result = import(&dir.zip(), &dir.target());

    assert!(result.succeeded(), "{:?}", result.problems);
    assert_eq!(fs::read_to_string(dir.target().join(".gitignore")).unwrap(), GITIGNORE_CONTENT);
}

#[test]
fn import_refuses_entries_that_could_point_outside_the_folder() {
    for name in ["../escape.json", "collections/../../escape.json", "/escape.json", "C:/escape.json", "collections\\escape.json", "collections//escape.json"] {
        let dir = TempDir::new("traversal");
        make_zip(&dir.zip(), &[("ricochet.json", VALID_MARKER), (name, "{\"id\":\"x\"}")]);

        let result = import(&dir.zip(), &dir.target());

        assert!(!result.succeeded(), "{name} was accepted");
        assert!(!problems_for(&result, name).is_empty(), "{name}: {:?}", result.problems);
        assert!(!dir.target().exists(), "{name} created the folder");
        assert!(!dir.path().join("escape.json").exists(), "{name} wrote outside the folder");
    }
}

#[test]
fn import_refuses_a_folder_that_already_holds_a_workspace_and_leaves_it_alone() {
    let dir = TempDir::new("existing");
    save_sample(&dir.source());
    let existing = dir.source().join("ricochet.json");
    let before = fs::read_to_string(&existing).unwrap();
    make_zip(&dir.zip(), &[("ricochet.json", VALID_MARKER)]);

    let result = import(&dir.zip(), &dir.source());

    assert!(!result.succeeded());
    assert_eq!(fs::read_to_string(&existing).unwrap(), before);
    assert_eq!(load(&dir.source()).0.collections.len(), 1);
}

#[test]
fn import_refuses_a_non_empty_folder_that_is_not_a_workspace() {
    let dir = TempDir::new("non-empty");
    fs::create_dir_all(dir.target()).unwrap();
    fs::write(dir.target().join("notes.txt"), "keep me").unwrap();
    make_zip(&dir.zip(), &[("ricochet.json", VALID_MARKER)]);

    let result = import(&dir.zip(), &dir.target());

    assert!(!result.succeeded());
    assert_eq!(fs::read_to_string(dir.target().join("notes.txt")).unwrap(), "keep me");
    assert!(!dir.target().join("ricochet.json").exists());
}

#[test]
fn import_refuses_a_file_that_already_has_the_target_name() {
    let dir = TempDir::new("file-target");
    fs::write(dir.target(), "not a folder").unwrap();
    make_zip(&dir.zip(), &[("ricochet.json", VALID_MARKER)]);

    let result = import(&dir.zip(), &dir.target());

    assert!(!result.succeeded());
    assert_eq!(fs::read_to_string(dir.target()).unwrap(), "not a folder");
}

#[test]
fn import_refuses_files_that_are_not_readable_workspace_json() {
    let cases = [
        ("collections/shop/collection.json", "not json at all"),
        ("collections/shop/collection.json", "{\"name\":\"no id\"}"),
        ("collections/shop/req-r1.json", "{\"id\":\"i1\",\"name\":\"no request\"}"),
        ("environments/staging.json", "[1, 2, 3]"),
        ("globals.json", "{\"not\":\"a list\"}"),
    ];
    for (name, content) in cases {
        let dir = TempDir::new("unreadable-import");
        make_zip(&dir.zip(), &[("ricochet.json", VALID_MARKER), (name, content)]);

        let result = import(&dir.zip(), &dir.target());

        assert!(!result.succeeded(), "{name} was accepted");
        assert!(!problems_for(&result, name).is_empty(), "{name}: {:?}", result.problems);
        assert!(!dir.target().exists(), "{name} created the folder");
    }
}

#[test]
fn import_refuses_a_shared_file_that_is_not_utf8() {
    let dir = TempDir::new("not-utf8");
    make_zip_bytes(&dir.zip(), &[("ricochet.json", VALID_MARKER.as_bytes().to_vec()), ("globals.json", vec![0xff, 0xfe, 0xfd])]);

    let result = import(&dir.zip(), &dir.target());

    assert!(!result.succeeded());
    assert!(!problems_for(&result, "globals.json").is_empty(), "{:?}", result.problems);
    assert!(!dir.target().exists());
}

#[test]
fn import_refuses_files_outside_the_workspace_layout() {
    for name in ["state.json", "history.json", "secrets.dat", ".env", "random-notes.json"] {
        let dir = TempDir::new("outside-layout");
        make_zip(&dir.zip(), &[("ricochet.json", VALID_MARKER), (name, "{\"id\":\"x\"}")]);

        let result = import(&dir.zip(), &dir.target());

        assert!(!result.succeeded(), "{name} was accepted");
        assert!(!problems_for(&result, name).is_empty(), "{name}: {:?}", result.problems);
        assert!(!dir.target().exists(), "{name} created the folder");
    }
}

#[test]
fn import_refuses_an_archive_without_a_marker_file() {
    let dir = TempDir::new("no-marker");
    make_zip(&dir.zip(), &[("globals.json", "[]")]);

    let result = import(&dir.zip(), &dir.target());

    assert!(!result.succeeded());
    assert!(result.problems.iter().any(|problem| problem.contains("ricochet.json")));
    assert!(!dir.target().exists());
}

#[test]
fn import_refuses_an_archive_that_is_not_a_zip() {
    let dir = TempDir::new("not-zip");
    fs::write(dir.zip(), "plain text, not an archive").unwrap();

    let result = import(&dir.zip(), &dir.target());

    assert!(!result.succeeded());
    assert!(!dir.target().exists());
}

#[test]
fn import_refuses_an_entry_larger_than_the_per_entry_limit() {
    let dir = TempDir::new("entry-bomb");
    let padded = " ".repeat(4096);
    make_zip(&dir.zip(), &[("ricochet.json", VALID_MARKER), ("globals.json", padded.as_str())]);
    let limits = ArchiveLimits { max_entries: 100, max_entry_bytes: 1024, max_total_bytes: MIB };

    let result = import_with_limits(&dir.zip(), &dir.target(), limits);

    assert!(!result.succeeded());
    assert!(!problems_for(&result, "globals.json").is_empty(), "{:?}", result.problems);
    assert!(result.problems.iter().any(|problem| problem.contains("1024 bytes")));
    assert!(!dir.target().exists());
}

#[test]
fn import_refuses_an_archive_that_unpacks_past_the_total_limit() {
    let dir = TempDir::new("total-bomb");
    let padded = " ".repeat(1100);
    make_zip(&dir.zip(), &[("ricochet.json", VALID_MARKER), ("globals.json", padded.as_str())]);
    let limits = ArchiveLimits { max_entries: 100, max_entry_bytes: 2048, max_total_bytes: 1000 };

    let result = import_with_limits(&dir.zip(), &dir.target(), limits);

    assert!(!result.succeeded());
    assert!(result.problems.iter().any(|problem| problem.contains("1000 bytes")), "{:?}", result.problems);
    assert!(!dir.target().exists());
}

#[test]
fn import_refuses_an_archive_with_more_entries_than_the_limit() {
    let dir = TempDir::new("entry-count");
    make_zip(&dir.zip(), &[("ricochet.json", VALID_MARKER), ("globals.json", "[]"), ("environments/a.json", "{}")]);
    let limits = ArchiveLimits { max_entries: 2, ..ArchiveLimits::default() };

    let result = import_with_limits(&dir.zip(), &dir.target(), limits);

    assert!(!result.succeeded());
    assert!(result.problems.iter().any(|problem| problem.contains("more than 2 entries")));
    assert!(!dir.target().exists());
}

#[test]
fn import_refuses_a_default_limit_zip_bomb_before_writing_anything() {
    let dir = TempDir::new("default-bomb");
    let padded = " ".repeat((17 * MIB) as usize);
    make_zip(&dir.zip(), &[("ricochet.json", VALID_MARKER), ("globals.json", padded.as_str())]);

    let result = import(&dir.zip(), &dir.target());

    assert!(!result.succeeded());
    assert!(!problems_for(&result, "globals.json").is_empty(), "{:?}", result.problems);
    assert!(!dir.target().exists());
}
