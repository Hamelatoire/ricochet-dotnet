//! Ports the CLI cases for `export`, `docs`, `env`, `zip` and `clone` from `tests/Ricochet.Core.Tests` (ADR 0028,
//! 0029, 0034, 0035, 0036): what each writes, what each refuses, and the exit codes. Each test uses its own temp
//! folder, and `RICOCHET_DATA` points there too, so the real personal folder and vault are never read.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ricochet_core::models::{ApiEnvironment, ApiRequest, Collection, CollectionItem, KeyValue, Workspace};
use ricochet_core::store::WorkspaceFolder;
use uuid::Uuid;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("ricochet-cli-{}", Uuid::new_v4().simple()));
        fs::create_dir_all(path.join("ws")).unwrap();
        fs::create_dir_all(path.join("out")).unwrap();
        fs::create_dir_all(path.join("data")).unwrap();
        let workspace = Workspace {
            collections: vec![Collection {
                id: "shop".into(),
                name: "Shop".into(),
                items: vec![CollectionItem {
                    id: "list".into(),
                    name: "List".into(),
                    request: Some(ApiRequest { name: "List".into(), method: "GET".into(), url: "https://example.com/items".into(), ..ApiRequest::default() }),
                    ..CollectionItem::default()
                }],
                ..Collection::default()
            }],
            environments: vec![ApiEnvironment {
                id: "staging".into(),
                name: "Staging".into(),
                variables: vec![KeyValue::new("host", "example.com")],
                ..ApiEnvironment::default()
            }],
            ..Workspace::default()
        };
        WorkspaceFolder::new(path.join("ws")).save(&workspace).unwrap();
        Self(path)
    }

    fn path(&self, name: &str) -> String {
        self.0.join(name).to_string_lossy().into_owned()
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ricochet"))
            .args(args)
            .env("RICOCHET_DATA", self.0.join("data"))
            .output()
            .unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn code(output: &Output) -> i32 {
    output.status.code().unwrap()
}

fn text(output: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
}

#[test]
fn export_writes_postman_and_openapi_files() {
    let s = Scratch::new();
    let (postman, openapi) = (s.path("out/p.json"), s.path("out/o.json"));
    let out = s.run(&["export", "shop", "--workspace", &s.path("ws"), "--postman", &postman, "--openapi", &openapi]);
    assert_eq!(code(&out), 0, "{}", text(&out));
    assert!(fs::read_to_string(&postman).unwrap().contains("schema.getpostman.com"));
    assert!(fs::read_to_string(&openapi).unwrap().contains("\"openapi\""));
}

#[test]
fn export_refuses_a_file_inside_the_workspace_and_an_existing_file() {
    let s = Scratch::new();
    let inside = s.run(&["export", "Shop", "--workspace", &s.path("ws"), "--postman", &s.path("ws/p.json")]);
    assert_eq!(code(&inside), 2);
    assert!(text(&inside).contains("inside the workspace folder"));

    fs::write(s.path("out/p.json"), "keep").unwrap();
    let existing = s.run(&["export", "Shop", "--workspace", &s.path("ws"), "--postman", &s.path("out/p.json")]);
    assert_eq!(code(&existing), 2);
    assert_eq!(fs::read_to_string(s.path("out/p.json")).unwrap(), "keep");
}

#[test]
fn export_needs_a_known_collection_and_a_format() {
    let s = Scratch::new();
    let unknown = s.run(&["export", "Nope", "--workspace", &s.path("ws"), "--postman", &s.path("out/p.json")]);
    assert_eq!(code(&unknown), 2);
    assert!(text(&unknown).contains("Collections: Shop."));
    let no_format = s.run(&["export", "Shop", "--workspace", &s.path("ws")]);
    assert_eq!(code(&no_format), 2);
    assert_eq!(code(&s.run(&["export", "--help"])), 0);
}

#[test]
fn docs_writes_a_page_outside_the_workspace() {
    let s = Scratch::new();
    let out = s.run(&["docs", "Shop", &s.path("out/d.html"), "--workspace", &s.path("ws")]);
    assert_eq!(code(&out), 0, "{}", text(&out));
    assert!(fs::read_to_string(s.path("out/d.html")).unwrap().contains("Shop"));
    assert_eq!(code(&s.run(&["docs", "Shop", "--workspace", &s.path("ws")])), 2);
}

#[test]
fn env_export_then_import_refuses_a_clash_unless_replace() {
    let s = Scratch::new();
    let file = s.path("out/env.json");
    let exported = s.run(&["env", "export", "staging", &file, "--workspace", &s.path("ws")]);
    assert_eq!(code(&exported), 0, "{}", text(&exported));

    let clash = s.run(&["env", "import", &file, "--workspace", &s.path("ws")]);
    assert_eq!(code(&clash), 1);
    assert!(text(&clash).contains("Pass --replace"));

    let replaced = s.run(&["env", "import", &file, "--workspace", &s.path("ws"), "--replace"]);
    assert_eq!(code(&replaced), 0, "{}", text(&replaced));
    assert!(text(&replaced).contains("Replaced environment \"Staging\""));

    assert_eq!(code(&s.run(&["env", "export", "Prod", &s.path("out/x.json"), "--workspace", &s.path("ws")])), 2);
    assert_eq!(code(&s.run(&["env", "import", &file, "--force", "--workspace", &s.path("ws")])), 2);
}

#[test]
fn zip_export_and_import_round_trip_into_an_empty_folder_only() {
    let s = Scratch::new();
    let zip = s.path("out/w.zip");
    let exported = s.run(&["zip", "export", &s.path("ws"), &zip]);
    assert_eq!(code(&exported), 0, "{}", text(&exported));

    let imported = s.run(&["zip", "import", &zip, &s.path("copy")]);
    assert_eq!(code(&imported), 0, "{}", text(&imported));
    assert!(Path::new(&s.path("copy/ricochet.json")).is_file());

    let again = s.run(&["zip", "import", &zip, &s.path("copy")]);
    assert_eq!(code(&again), 2);
    assert!(text(&again).contains("is not empty"));
}

#[test]
fn zip_import_refuses_an_archive_without_a_marker() {
    let s = Scratch::new();
    fs::write(s.path("out/not.zip"), "not a zip").unwrap();
    let out = s.run(&["zip", "import", &s.path("out/not.zip"), &s.path("copy")]);
    assert_eq!(code(&out), 2);
    assert!(text(&out).contains("not a readable ZIP file"));
    assert!(!Path::new(&s.path("copy")).exists());
}

#[test]
fn clone_refuses_a_password_in_the_address() {
    let s = Scratch::new();
    let out = s.run(&["clone", "https://user:secret@example.com/repo.git", &s.path("out")]);
    assert_eq!(code(&out), 2);
    assert!(!text(&out).contains("secret@"));
    assert_eq!(code(&s.run(&["clone"])), 2);
    assert_eq!(code(&s.run(&["clone", "--help"])), 0);
}

#[test]
fn an_unknown_command_cannot_run() {
    let s = Scratch::new();
    assert_eq!(code(&s.run(&["frobnicate"])), 2);
}
