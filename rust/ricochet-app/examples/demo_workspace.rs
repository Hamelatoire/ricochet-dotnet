//! Writes a small throwaway Workspace for trying the window and taking screenshots:
//! `cargo run --example demo_workspace -- <folder> [base-url]`. Never point it at a real Workspace.

use ricochet_core::models::{ApiEnvironment, ApiRequest, Collection, CollectionItem, KeyValue, Workspace};
use ricochet_core::store::WorkspaceFolder;

fn request(name: &str, method: &str, url: &str, headers: Vec<KeyValue>, body: &str) -> CollectionItem {
    let request = ApiRequest { name: name.into(), method: method.into(), url: url.into(), headers, body: body.into(), ..Default::default() };
    CollectionItem { name: name.into(), request: Some(request), ..Default::default() }
}

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let folder = args.next().expect("pass the folder to write the demo Workspace into");
    let base_url = args.next().unwrap_or_else(|| "http://127.0.0.1:8765".to_string());
    std::fs::create_dir_all(&folder)?;

    let orders = CollectionItem {
        name: "Orders".into(),
        variables: vec![KeyValue::new("resource", "orders")],
        children: vec![
            request("List orders", "GET", "{{baseUrl}}/{{resource}}.json?limit=10&sort=newest", vec![KeyValue::new("Accept", "application/json")], ""),
            request(
                "Create order",
                "POST",
                "{{baseUrl}}/{{resource}}",
                vec![KeyValue::new("Content-Type", "application/json"), KeyValue::new("X-Trace", "{{traceId}}")],
                "{\n  \"item\": \"kettle\",\n  \"quantity\": 2\n}",
            ),
        ],
        ..Default::default()
    };
    // The active Environment is personal state, not part of the folder, so the base URL is a Collection Variable.
    let staging = ApiEnvironment { name: "Staging".into(), variables: vec![KeyValue::new("region", "eu")], ..Default::default() };
    let workspace = Workspace {
        collections: vec![Collection {
            name: "Shop API".into(),
            variables: vec![KeyValue::new("baseUrl", base_url)],
            items: vec![request("Status", "GET", "{{baseUrl}}/status.json", Vec::new(), ""), orders],
            ..Default::default()
        }],
        environments: vec![staging],
        ..Default::default()
    };
    let issues = WorkspaceFolder::new(&folder).save(&workspace)?;
    println!("Wrote the demo Workspace to {folder} ({} issue(s))", issues.len());
    Ok(())
}
