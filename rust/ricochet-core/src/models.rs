//! Port of `Models.cs`. Field names and defaults match the C# file format, which uses camelCase.
//! Optional fields are skipped when empty, so they add nothing to a file, as in C#.

use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};
use crate::assertions::{RequestAssertion, RequestCapture};

/// Max entries kept in a Workspace's history.
pub const MAX_HISTORY: usize = 200;
/// Name given to the Collection that legacy top-level requests are moved into.
pub const DEFAULT_COLLECTION_NAME: &str = "My Requests";

pub fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn yes() -> bool {
    true
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct KeyValue {
    pub key: String,
    pub value: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub is_secret: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub is_prompt: bool,
}

impl Default for KeyValue {
    fn default() -> Self {
        Self {
            key: String::new(),
            value: String::new(),
            enabled: true,
            is_secret: false,
            is_prompt: false,
        }
    }
}

impl KeyValue {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self { key: key.into(), value: value.into(), ..Self::default() }
    }
}

pub mod auth_kinds {
    pub const NONE: &str = "none";
    pub const BEARER: &str = "bearer";
    pub const BASIC: &str = "basic";
    pub const API_KEY: &str = "apikey";
    pub const DIGEST: &str = "digest";
    pub const OAUTH1: &str = "oauth1";
    pub const AWS_SIG_V4: &str = "awsv4";
}

/// One Request's auth settings. Fields of other kinds are kept, so switching back loses nothing,
/// but only the chosen kind is sent. Token, Password and KeyValue are Secrets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RequestAuth {
    pub kind: String,
    pub token: String,
    pub username: String,
    pub password: String,
    pub key_name: String,
    pub key_value: String,
    pub key_in_query: bool,
    /// Settings for the OAuth 1.0 and AWS kinds (ADR 0024). None when unused.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<AuthDetails>,
}

impl Default for RequestAuth {
    fn default() -> Self {
        Self {
            kind: auth_kinds::NONE.to_string(),
            token: String::new(),
            username: String::new(),
            password: String::new(),
            key_name: String::new(),
            key_value: String::new(),
            key_in_query: false,
            details: None,
        }
    }
}

/// Settings for the signing auth kinds. Secret fields are stored in the vault, never in the file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AuthDetails {
    pub consumer_key: String,
    /// A Secret.
    pub consumer_secret: String,
    /// A Secret, paired with the auth's token.
    pub token_secret: String,
    pub aws_access_key_id: String,
    /// A Secret.
    pub aws_secret_access_key: String,
    /// A Secret.
    pub aws_session_token: String,
    pub aws_region: String,
    pub aws_service: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ApiRequest {
    pub id: String,
    pub name: String,
    pub method: String,
    pub url: String,
    pub headers: Vec<KeyValue>,
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<RequestAuth>,
    /// Checks on the response (ADR 0006). None when there are none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assertions: Option<Vec<RequestAssertion>>,
    /// Values saved from the response for later Requests (ADR 0006). None when there are none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub captures: Option<Vec<RequestCapture>>,
    /// Set on Requests made from an OpenAPI description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec_key: Option<String>,
    /// Saved example responses (ADR 0023). None when there are none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub examples: Option<Vec<ResponseExample>>,
}

impl Default for ApiRequest {
    fn default() -> Self {
        Self {
            id: new_id(),
            name: String::new(),
            method: "GET".to_string(),
            url: String::new(),
            headers: Vec::new(),
            body: String::new(),
            auth: None,
            assertions: None,
            captures: None,
            spec_key: None,
            examples: None,
        }
    }
}

/// A response the user saved as an expected example (ADR 0023). Credentials are masked first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ResponseExample {
    pub id: String,
    pub name: String,
    pub status_code: i32,
    pub reason_phrase: String,
    pub headers: Vec<KeyValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    pub body: String,
}

impl Default for ResponseExample {
    fn default() -> Self {
        Self {
            id: new_id(),
            name: String::new(),
            status_code: 0,
            reason_phrase: String::new(),
            headers: Vec::new(),
            media_type: None,
            body: String::new(),
        }
    }
}

/// A script kept from an import. It is stored but never run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredScript {
    pub listen: String,
    pub source: String,
}

/// A Folder when `request` is None, otherwise a Request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CollectionItem {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<ApiRequest>,
    pub children: Vec<CollectionItem>,
    pub scripts: Vec<StoredScript>,
    /// On a Folder: Variables for the Requests inside it (ADR 0020).
    pub variables: Vec<KeyValue>,
    /// On Folders only (ADR 0030). None when there are none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

impl Default for CollectionItem {
    fn default() -> Self {
        Self {
            id: new_id(),
            name: String::new(),
            request: None,
            children: Vec::new(),
            scripts: Vec::new(),
            variables: Vec::new(),
            tags: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Collection {
    pub id: String,
    pub name: String,
    pub items: Vec<CollectionItem>,
    pub variables: Vec<KeyValue>,
    pub scripts: Vec<StoredScript>,
    /// Set on a Collection kept up to date from an OpenAPI URL (ADR 0014).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec_url: Option<String>,
    /// Folders and Requests the user has hidden (ADR 0032).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hidden_item_ids: Option<Vec<String>>,
}

impl Default for Collection {
    fn default() -> Self {
        Self {
            id: new_id(),
            name: String::new(),
            items: Vec::new(),
            variables: Vec::new(),
            scripts: Vec::new(),
            spec_url: None,
            hidden_item_ids: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ApiEnvironment {
    pub id: String,
    pub name: String,
    pub variables: Vec<KeyValue>,
}

impl Default for ApiEnvironment {
    fn default() -> Self {
        Self { id: new_id(), name: String::new(), variables: Vec::new() }
    }
}

/// One send. `certificates_unverified` is set only when certificate checks were off (ADR 0079).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub request: ApiRequest,
    pub status_code: Option<i32>,
    pub elapsed_ms: i64,
    pub at: DateTime<FixedOffset>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub collection_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub certificates_unverified: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Workspace {
    pub collections: Vec<Collection>,
    pub environments: Vec<ApiEnvironment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_environment_id: Option<String>,
    pub globals: Vec<KeyValue>,
    pub history: Vec<HistoryEntry>,
    /// From the original file format. Read so it can be migrated, never written.
    #[serde(rename = "saved", skip_serializing, default)]
    pub legacy_saved: Option<Vec<ApiRequest>>,
    /// From the original file format. Read so it can be migrated, never written.
    #[serde(rename = "variables", skip_serializing, default)]
    pub legacy_variables: Option<Vec<KeyValue>>,
}

impl Workspace {
    /// Moves the original file format's top-level fields into the current shape.
    pub fn migrate_legacy_fields(mut self) -> Self {
        if self.legacy_saved.is_none() && self.legacy_variables.is_none() {
            return self;
        }

        if let Some(saved) = self.legacy_saved.take() {
            if !saved.is_empty() {
                let items = saved
                    .into_iter()
                    .map(|request| CollectionItem {
                        name: request.name.clone(),
                        request: Some(request),
                        ..CollectionItem::default()
                    })
                    .collect();
                self.collections.push(Collection {
                    name: DEFAULT_COLLECTION_NAME.to_string(),
                    items,
                    ..Collection::default()
                });
            }
        }

        if let Some(variables) = self.legacy_variables.take() {
            if !variables.is_empty() {
                self.globals = variables;
            }
        }

        self
    }
}
