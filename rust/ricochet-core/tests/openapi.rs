use pretty_assertions::assert_eq;
use ricochet_core::models::*;
use ricochet_core::openapi::{OpenApiError, build};

const URL: &str = "https://api.example.test/openapi.json";

const SPEC: &str = r##"{
  "openapi": "3.0.3",
  "info": { "title": "Pets" },
  "servers": [ { "url": "https://api.example.test/v1/" } ],
  "security": [ { "bearerAuth": [] } ],
  "paths": {
    "/pets": {
      "get": { "operationId": "listPets", "summary": "List pets", "tags": ["pets"],
        "parameters": [ { "name": "limit", "in": "query", "schema": { "type": "integer" } } ] },
      "post": { "operationId": "createPet", "summary": "Create a pet", "tags": ["pets"],
        "requestBody": { "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Pet" } } } } }
    },
    "/pets/{petId}": {
      "delete": { "summary": "Delete a pet", "tags": ["pets"], "security": [ { "apiKeyAuth": [] } ] }
    },
    "/health": {
      "get": { "operationId": "health", "summary": "Health", "security": [] }
    }
  },
  "components": {
    "schemas": {
      "Pet": {
        "type": "object",
        "properties": {
          "name": { "type": "string" },
          "age": { "type": "integer" },
          "tags": { "type": "array", "items": { "type": "string" } }
        }
      }
    },
    "securitySchemes": {
      "bearerAuth": { "type": "http", "scheme": "bearer" },
      "apiKeyAuth": { "type": "apiKey", "in": "header", "name": "X-Api-Key" }
    }
  }
}"##;

/// The same description with a path parameter declared on DELETE /pets/{petId}.
fn spec_with_path_parameter() -> String {
    SPEC.replace(
        r##""summary": "Delete a pet","##,
        r##""summary": "Delete a pet", "parameters": [ { "name": "petId", "in": "path", "required": true, "schema": { "type": "integer" } } ],"##,
    )
}

fn import(json: &str, existing: Option<Collection>) -> ricochet_core::openapi::OpenApiResult {
    build(json, URL, existing).expect("the fixture should be a readable description")
}

fn variable<'a>(collection: &'a Collection, key: &str) -> &'a KeyValue {
    collection.variables.iter().find(|v| v.key == key).unwrap_or_else(|| panic!("no variable {key}"))
}

fn variable_mut<'a>(collection: &'a mut Collection, key: &str) -> &'a mut KeyValue {
    collection.variables.iter_mut().find(|v| v.key == key).unwrap_or_else(|| panic!("no variable {key}"))
}

fn folder<'a>(collection: &'a Collection, name: &str) -> &'a CollectionItem {
    collection
        .items
        .iter()
        .find(|i| i.request.is_none() && i.name == name)
        .unwrap_or_else(|| panic!("no folder {name}"))
}

fn folder_mut<'a>(collection: &'a mut Collection, name: &str) -> &'a mut CollectionItem {
    collection
        .items
        .iter_mut()
        .find(|i| i.request.is_none() && i.name == name)
        .unwrap_or_else(|| panic!("no folder {name}"))
}

fn names(items: &[CollectionItem]) -> Vec<&str> {
    items.iter().map(|i| i.name.as_str()).collect()
}

fn request(item: &CollectionItem) -> &ApiRequest {
    item.request.as_ref().expect("the item should be a request")
}

#[test]
fn creates_a_collection_with_a_folder_per_tag_and_the_server_as_a_variable() {
    let collection = import(SPEC, None).collection;

    assert_eq!(collection.name, "Pets");
    assert_eq!(collection.spec_url.as_deref(), Some(URL));
    assert_eq!(variable(&collection, "baseUrl").value, "https://api.example.test/v1");

    let pets = folder(&collection, "pets");
    assert_eq!(names(&pets.children), ["List pets", "Create a pet", "Delete a pet"]);
    assert_eq!(request(&pets.children[0]).url, "{{baseUrl}}/pets");
    assert_eq!(request(&pets.children[0]).method, "GET");
    assert_eq!(request(&pets.children[2]).url, "{{baseUrl}}/pets/{{petId}}");
    assert_eq!(request(&pets.children[0]).spec_key.as_deref(), Some("listPets"));
    assert_eq!(request(&pets.children[2]).spec_key.as_deref(), Some("DELETE /pets/{petId}"));

    // Without a tag, an operation is filed under the first word of its path.
    let health = folder(&collection, "health");
    assert_eq!(request(&health.children[0]).spec_key.as_deref(), Some("health"));
}

#[test]
fn path_parameters_become_empty_collection_variables() {
    let collection = import(&spec_with_path_parameter(), None).collection;

    let petid = variable(&collection, "petId");
    assert_eq!(petid.value, "");
    let pets = folder(&collection, "pets");
    let delete = pets.children.iter().find(|c| c.name == "Delete a pet").expect("delete request");
    assert_eq!(request(delete).url, "{{baseUrl}}/pets/{{petId}}");
}

#[test]
fn path_parameters_do_not_overwrite_values_already_typed() {
    let json = spec_with_path_parameter();
    let first = import(&json, None).collection;
    let mut typed = first.clone();
    variable_mut(&mut typed, "petId").value = "42".to_string();

    let again = import(&json, Some(typed)).collection;

    assert_eq!(variable(&again, "petId").value, "42");
}

#[test]
fn tag_groups_become_folders_around_the_tag_folders() {
    let json = SPEC.replace(
        r##""components": {"##,
        r##""x-tagGroups": [ { "name": "Core Resources", "tags": ["pets"] } ], "components": {"##,
    );
    let collection = import(&json, None).collection;

    let group = folder(&collection, "Core Resources");
    let pets = group.children.iter().find(|c| c.name == "pets").expect("pets folder inside the group");
    assert_eq!(names(&pets.children), ["List pets", "Create a pet", "Delete a pet"]);
    assert!(!collection.items.iter().any(|i| i.name == "pets"));
}

#[test]
fn untagged_paths_are_grouped_by_their_resource_in_nested_folders() {
    let untagged = r##"{
      "openapi": "3.0.3",
      "info": { "title": "Untagged" },
      "servers": [ { "url": "https://api.example.test" } ],
      "paths": {
        "/v1/account_links": { "post": { "summary": "Create link" } },
        "/v1/accounts/{id}": { "get": { "summary": "Get account" } },
        "/{id}": { "get": { "summary": "Anything" } }
      }
    }"##;
    let collection = import(untagged, None).collection;

    // The version segment is skipped, and a path that starts with a parameter has no folder.
    assert_eq!(names(&folder(&collection, "account_links").children), ["Create link"]);
    assert_eq!(names(&folder(&collection, "accounts").children), ["Get account"]);
    assert!(collection.items.iter().any(|i| i.request.is_some() && i.name == "Anything"));
}

#[test]
fn a_json_body_is_built_from_the_schema_and_is_not_invented_for_get() {
    let collection = import(SPEC, None).collection;
    let pets = folder(&collection, "pets");

    assert_eq!(request(&pets.children[0]).body, "");
    let body = &request(&pets.children[1]).body;
    assert!(body.contains(r#""name": """#), "body was {body}");
    assert!(body.contains(r#""age": 0"#), "body was {body}");
    assert!(body.contains(r#""tags": ["#), "body was {body}");
}

#[test]
fn auth_becomes_variables_and_no_value_from_the_description_is_copied() {
    let collection = import(SPEC, None).collection;
    let pets = folder(&collection, "pets");

    let bearer = request(&pets.children[0]).auth.as_ref().expect("bearer auth");
    assert_eq!(bearer.kind, auth_kinds::BEARER);
    assert_eq!(bearer.token, "{{bearerAuth}}");
    assert!(variable(&collection, "bearerAuth").is_secret);

    let api_key = request(&pets.children[2]).auth.as_ref().expect("api key auth");
    assert_eq!(api_key.kind, auth_kinds::API_KEY);
    assert_eq!(api_key.key_name, "X-Api-Key");
    assert_eq!(api_key.key_value, "{{apiKeyAuth}}");

    // An operation that says no security needs none, even though the document has a global default.
    let health = folder(&collection, "health");
    assert_eq!(request(&health.children[0]).auth, None);
    assert_eq!(variable(&collection, "bearerAuth").value, "");
}

#[test]
fn a_rerun_keeps_what_the_user_added_and_reports_what_changed() {
    let first = import(SPEC, None).collection;

    // The user adds a header and a body on an existing request, and sets the staging address.
    let mut edited = first.clone();
    {
        let pets = folder_mut(&mut edited, "pets");
        let kept = pets.children[1].request.as_mut().expect("create request");
        kept.headers = vec![KeyValue::new("X-Trace", "on")];
        kept.body = r#"{"name":"Rex"}"#.to_string();
    }
    variable_mut(&mut edited, "baseUrl").value = "https://staging.example.test/v1".to_string();

    // The new description renames one operation, drops DELETE, and adds another.
    let changed = SPEC
        .replace(r##""summary": "List pets""##, r##""summary": "List all pets""##)
        .replace(
            r##""/health": {"##,
            r##""/owners": { "get": { "operationId": "listOwners", "summary": "Owners", "tags": ["owners"] } },
            "/health": {"##,
        )
        .replace(
            r##""delete": { "summary": "Delete a pet", "tags": ["pets"], "security": [ { "apiKeyAuth": [] } ] }"##,
            r##""put": { "summary": "Put nothing" }"##,
        );

    let result = import(&changed, Some(edited));
    let updated = folder(&result.collection, "pets");

    assert_eq!(updated.children[0].name, "List all pets");
    let kept = request(&updated.children[1]);
    assert_eq!(kept.headers, vec![KeyValue::new("X-Trace", "on")]);
    assert_eq!(kept.body, r#"{"name":"Rex"}"#);

    // The collection keeps the staging address the user set.
    assert_eq!(variable(&result.collection, "baseUrl").value, "https://staging.example.test/v1");

    assert!(names(&folder(&result.collection, "owners").children).contains(&"Owners"));
    assert!(result.summary.removed.contains(&"Delete a pet".to_string()));
    assert!(result.summary.added.contains(&"owners / Owners".to_string()));
    assert!(result.summary.changed.contains(&"List all pets".to_string()));
    assert_eq!(result.collection.id, first.id);
    assert!(!result.summary.is_new);
}

#[test]
fn rerunning_an_unchanged_description_changes_nothing() {
    let first = import(SPEC, None).collection;

    let again = import(SPEC, Some(first)).summary;

    assert!(again.added.is_empty());
    assert!(again.changed.is_empty());
    assert!(again.removed.is_empty());
}

#[test]
fn query_and_header_parameters_are_reported_rather_than_guessed() {
    let summary = import(SPEC, None).summary;

    assert!(summary.notes.iter().any(|n| n.contains("query or header parameters")));
    assert!(summary.is_new);
}

#[test]
fn a_relative_server_is_reported_and_the_description_is_still_read() {
    let json = SPEC.replace("https://api.example.test/v1/", "/v1");

    let result = import(&json, None);

    assert!(result.summary.notes.iter().any(|n| n.contains("relative")));
    assert_eq!(folder(&result.collection, "pets").children.len(), 3);
}

#[test]
fn a_description_without_servers_is_reported_and_leaves_baseurl_empty() {
    let json = SPEC.replace(r##""servers": [ { "url": "https://api.example.test/v1/" } ],"##, "");

    let result = import(&json, None);

    assert!(result.summary.notes.iter().any(|n| n.contains("lists no server")));
    assert_eq!(variable(&result.collection, "baseUrl").value, "");
}

#[test]
fn a_reference_outside_the_document_is_not_followed_and_is_reported() {
    let json = SPEC.replace(
        r##"{ "$ref": "#/components/schemas/Pet" }"##,
        r##"{ "$ref": "https://elsewhere.test/pet.json" }"##,
    );

    let result = import(&json, None);

    assert!(result.summary.notes.iter().any(|n| n.contains("outside this description")));
    assert!(!request(&folder(&result.collection, "pets").children[1]).body.contains("\"name\""));
}

#[test]
fn an_operation_that_appears_twice_is_imported_once_and_reported() {
    let json = SPEC.replace(
        r##""operationId": "health", "summary": "Health""##,
        r##""operationId": "listPets", "summary": "Health again""##,
    );

    let result = import(&json, None);

    assert!(result.summary.notes.iter().any(|n| n.contains("appears more than once")));
    let listed = result.collection.items.iter().filter(|i| i.request.is_some() && i.name == "Health again").count();
    assert_eq!(listed, 0);
}

#[test]
fn the_report_lists_what_was_added_and_what_was_not_imported() {
    let text = import(SPEC, None).summary.to_report_text();

    assert!(text.starts_with("New collection with 4 requests."), "report was:\n{text}");
    assert!(text.contains("\n\nAdded:\n  pets / List pets\n"), "report was:\n{text}");
    assert!(text.contains("\n\nNot imported:\n"), "report was:\n{text}");
}

#[test]
fn text_that_is_not_json_is_refused_with_a_clear_message() {
    let error = build("not json at all", URL, None).unwrap_err();

    assert_eq!(error, OpenApiError::NotJson);
    assert!(error.message().contains("not JSON"));
}

#[test]
fn a_document_that_is_not_openapi_3_is_refused_with_a_clear_message() {
    let error = build(r#"{"swagger": "2.0"}"#, URL, None).unwrap_err();

    assert_eq!(error, OpenApiError::NotOpenApi3);
    assert!(error.message().contains("not an OpenAPI 3"));
}
