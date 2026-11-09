//! Port of the variable part of `VariableResolver.cs` (ADR 0008, 0019, 0020).
//! Not ported yet: built-in functions such as `{{$guid}}`, external Secret references (ADR 0025),
//! prompt answers, and the assertions and captures inside a Request. Unknown names are left as written.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::OnceLock;

use regex::Regex;

use crate::models::{ApiRequest, AuthDetails, Collection, CollectionItem, KeyValue, RequestAuth, auth_kinds};

/// The request to send, and the copy History keeps: placeholders filled in for the send, Variables left as written in History.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedRequests {
    pub sent: ApiRequest,
    pub history: ApiRequest,
}

/// A prompt Variable the Request uses (ADR 0019). `default` is its stored value, which the prompt starts from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptVariable {
    pub name: String,
    pub default: String,
}

/// A (sent, shown) pair of strings for one piece of text.
type Pair = (String, String);

fn placeholder() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"\{\{([^{}]+)\}\}").expect("placeholder pattern is valid"))
}

/// Combines scopes, lowest priority first. A later scope overrides an earlier one with the same name.
pub fn layer(scopes: &[&[KeyValue]]) -> Vec<KeyValue> {
    scopes.iter().flat_map(|scope| scope.iter().cloned()).collect()
}

/// The Variables of the Folders around an item, outermost first, so a nearer Folder overrides one further out.
/// Empty when the item is directly in the Collection, or is not in it (ADR 0020).
pub fn folder_scope(collection: &Collection, item_id: Option<&str>) -> Vec<KeyValue> {
    folder_path(&collection.items, item_id).unwrap_or_default()
}

fn folder_path(items: &[CollectionItem], item_id: Option<&str>) -> Option<Vec<KeyValue>> {
    let item_id = item_id?;
    for item in items {
        if item.request.is_some() {
            if item.id == item_id {
                return Some(Vec::new());
            }
            continue;
        }
        if let Some(below) = folder_path(&item.children, Some(item_id)) {
            let mut out = item.variables.clone();
            out.extend(below);
            return Some(out);
        }
    }
    None
}

/// The Request to send, with every placeholder replaced.
pub fn apply(request: &ApiRequest, variables: &[KeyValue]) -> ApiRequest {
    resolve(request, variables).sent
}

/// Resolves once, so the sent Request and its History copy agree. Variables keep their names in History.
pub fn resolve(request: &ApiRequest, variables: &[KeyValue]) -> ResolvedRequests {
    let mut resolver = Resolver::new(variables);

    let url = resolver.expand(&request.url);
    let body = resolver.resolve_body(&request.body);
    let headers: Vec<(Pair, Pair)> = request
        .headers
        .iter()
        .map(|h| (resolver.expand(&h.key), resolver.expand(&h.value)))
        .collect();
    let auth = request.auth.as_ref().map(|a| expand_auth(&mut resolver, a));

    let mut sent = request.clone();
    let mut history = request.clone();

    (sent.url, history.url) = url;
    (sent.body, history.body) = body;
    sent.headers = request
        .headers
        .iter()
        .zip(&headers)
        .map(|(h, ((key, _), (value, _)))| KeyValue { key: key.clone(), value: value.clone(), ..h.clone() })
        .collect();
    history.headers = request
        .headers
        .iter()
        .zip(&headers)
        .map(|(h, ((_, key), (_, value)))| KeyValue { key: key.clone(), value: value.clone(), ..h.clone() })
        .collect();
    if let Some((sent_auth, shown_auth)) = auth {
        sent.auth = Some(sent_auth);
        history.auth = Some(shown_auth);
    }

    ResolvedRequests { sent, history }
}

/// The prompt Variables this Request uses, directly or through other Variables, with the value each would otherwise have.
/// A prompt Variable's own stored value is not searched further, because the answer replaces it (ADR 0019).
pub fn find_prompt_variables(request: &ApiRequest, variables: &[KeyValue]) -> Vec<PromptVariable> {
    let mut winners: HashMap<String, &KeyValue> = HashMap::new();
    for variable in variables {
        if variable.enabled && !variable.key.trim().is_empty() && !(variable.is_secret && variable.value.is_empty()) {
            winners.insert(variable.key.trim().to_string(), variable);
        }
    }

    let mut found = Vec::new();
    let mut seen = HashSet::new();
    let mut pending: VecDeque<String> = find_placeholders(request).into();
    while let Some(name) = pending.pop_front() {
        let Some(variable) = winners.get(&name) else { continue };
        if !seen.insert(name.clone()) {
            continue;
        }

        if variable.is_prompt {
            found.push(PromptVariable { name, default: variable.value.clone() });
            continue;
        }

        for caps in placeholder().captures_iter(&variable.value) {
            pending.push_back(caps[1].trim().to_string());
        }
    }

    found
}

/// Names still written as placeholders in the parts of a Request that will be sent, in order, without repeats.
pub fn find_placeholders(request: &ApiRequest) -> Vec<String> {
    let mut texts: Vec<&str> = vec![request.url.as_str(), request.body.as_str()];
    for header in request.headers.iter().filter(|h| h.enabled) {
        texts.push(header.key.as_str());
        texts.push(header.value.as_str());
    }

    // A Request keeps the fields of other auth kinds, but only the chosen kind is sent.
    if let Some(auth) = &request.auth {
        match auth.kind.as_str() {
            auth_kinds::BEARER => texts.push(auth.token.as_str()),
            auth_kinds::BASIC | auth_kinds::DIGEST => {
                texts.push(auth.username.as_str());
                texts.push(auth.password.as_str());
            }
            auth_kinds::API_KEY => {
                texts.push(auth.key_name.as_str());
                texts.push(auth.key_value.as_str());
            }
            auth_kinds::OAUTH1 => {
                if let Some(d) = &auth.details {
                    texts.extend([
                        d.consumer_key.as_str(),
                        d.consumer_secret.as_str(),
                        auth.token.as_str(),
                        d.token_secret.as_str(),
                    ]);
                }
            }
            auth_kinds::AWS_SIG_V4 => {
                if let Some(d) = &auth.details {
                    texts.extend([
                        d.aws_access_key_id.as_str(),
                        d.aws_secret_access_key.as_str(),
                        d.aws_session_token.as_str(),
                        d.aws_region.as_str(),
                        d.aws_service.as_str(),
                    ]);
                }
            }
            _ => {}
        }
    }

    let mut names: Vec<String> = Vec::new();
    for text in &texts {
        for caps in placeholder().captures_iter(text) {
            names.push(caps[1].trim().to_string());
        }
    }
    for text in &texts {
        if let Some(unclosed) = unclosed_placeholder(text) {
            names.push(unclosed);
        }
    }

    let mut distinct: Vec<String> = Vec::new();
    for name in names {
        if !distinct.contains(&name) {
            distinct.push(name);
        }
    }
    distinct
}

/// The text from a "{{" with no "}}" after it, such as "{{user", or None when there is none.
fn unclosed_placeholder(text: &str) -> Option<String> {
    let open = text.rfind("{{")?;
    if text[open + 2..].contains("}}") {
        return None;
    }
    Some(text[open..].split(['\r', '\n']).next().unwrap_or("").trim().to_string())
}

fn expand_auth(resolver: &mut Resolver<'_>, auth: &RequestAuth) -> (RequestAuth, RequestAuth) {
    let mut sent = auth.clone();
    let mut shown = auth.clone();
    (sent.token, shown.token) = resolver.expand(&auth.token);
    (sent.username, shown.username) = resolver.expand(&auth.username);
    (sent.password, shown.password) = resolver.expand(&auth.password);
    (sent.key_name, shown.key_name) = resolver.expand(&auth.key_name);
    (sent.key_value, shown.key_value) = resolver.expand(&auth.key_value);

    if let Some(d) = &auth.details {
        sent.details = Some(expand_details(resolver, d, true));
        shown.details = Some(expand_details(resolver, d, false));
    }

    (sent, shown)
}

fn expand_details(resolver: &mut Resolver<'_>, d: &AuthDetails, want_sent: bool) -> AuthDetails {
    let mut out = d.clone();
    let pick = |pair: Pair| if want_sent { pair.0 } else { pair.1 };
    out.consumer_key = pick(resolver.expand(&d.consumer_key));
    out.consumer_secret = pick(resolver.expand(&d.consumer_secret));
    out.token_secret = pick(resolver.expand(&d.token_secret));
    out.aws_access_key_id = pick(resolver.expand(&d.aws_access_key_id));
    out.aws_secret_access_key = pick(resolver.expand(&d.aws_secret_access_key));
    out.aws_session_token = pick(resolver.expand(&d.aws_session_token));
    out.aws_region = pick(resolver.expand(&d.aws_region));
    out.aws_service = pick(resolver.expand(&d.aws_service));
    out
}

/// Expands placeholders once per send, so every use of a Variable sees the same value.
struct Resolver<'a> {
    /// Enabled winners by trimmed name. A later Variable with the same name overrides an earlier one.
    lookup: HashMap<String, &'a str>,
    expanded: HashMap<String, String>,
    in_progress: HashSet<String>,
}

impl<'a> Resolver<'a> {
    fn new(variables: &'a [KeyValue]) -> Self {
        let mut lookup = HashMap::new();
        for variable in variables {
            // A Secret or prompt Variable with no value counts as unset, so it is reported rather than sent blank.
            if variable.enabled && !variable.key.is_empty() && !((variable.is_secret || variable.is_prompt) && variable.value.is_empty()) {
                lookup.insert(variable.key.trim().to_string(), variable.value.as_str());
            }
        }
        Self { lookup, expanded: HashMap::new(), in_progress: HashSet::new() }
    }

    /// Replaces every placeholder in `text`. The sent copy has the values; the shown copy keeps the placeholder.
    fn expand(&mut self, text: &str) -> Pair {
        let mut sent = String::new();
        let mut shown = String::new();
        let mut last = 0;
        for caps in placeholder().captures_iter(text) {
            let m = caps.get(0).expect("group 0 always exists");
            sent.push_str(&text[last..m.start()]);
            shown.push_str(&text[last..m.start()]);
            last = m.end();

            let name = caps[1].trim();
            let (replacement_sent, replacement_shown) = self.replacement(name, m.as_str());
            sent.push_str(&replacement_sent);
            shown.push_str(&replacement_shown);
        }
        sent.push_str(&text[last..]);
        shown.push_str(&text[last..]);
        (sent, shown)
    }

    fn replacement(&mut self, name: &str, original: &str) -> Pair {
        match self.lookup.get(name).copied() {
            Some(value) => {
                let text = self.variable_text(name, value);
                (text, original.to_string())
            }
            None => (original.to_string(), original.to_string()),
        }
    }

    fn variable_text(&mut self, name: &str, value: &str) -> String {
        if let Some(done) = self.expanded.get(name) {
            return done.clone();
        }

        // A Variable that refers back to itself is left as written, so the cycle shows up as a missing value.
        if !self.in_progress.insert(name.to_string()) {
            return format!("{{{{{name}}}}}");
        }

        let text = self.expand(value).0;
        self.in_progress.remove(name);
        self.expanded.insert(name.to_string(), text.clone());
        text
    }

    /// In a JSON body, a value inside a string is escaped so a quote in it cannot end the string early.
    /// A value outside a string is inserted as written, because it may be a whole JSON object.
    fn resolve_body(&mut self, text: &str) -> Pair {
        let trimmed = text.trim_start();
        if !trimmed.starts_with('{') && !trimmed.starts_with('[') {
            return self.expand(text);
        }

        let mut sent = String::new();
        let mut shown = String::new();
        let mut scan = JsonScan::default();
        let mut last = 0;
        for caps in placeholder().captures_iter(text) {
            let m = caps.get(0).expect("group 0 always exists");
            let literal = &text[last..m.start()];
            scan.feed(literal);
            sent.push_str(literal);
            shown.push_str(literal);
            last = m.end();

            let name = caps[1].trim();
            let (replacement_sent, replacement_shown) = self.replacement(name, m.as_str());
            if replacement_sent == m.as_str() {
                sent.push_str(m.as_str());
                shown.push_str(m.as_str());
                continue;
            }

            if scan.in_string {
                sent.push_str(&json_escape(&replacement_sent));
            } else {
                sent.push_str(&replacement_sent);
            }
            shown.push_str(&replacement_shown);
        }

        let literal = &text[last..];
        scan.feed(literal);
        sent.push_str(literal);
        shown.push_str(literal);
        (sent, shown)
    }
}

/// Tracks whether the text so far is inside a JSON string, so a placeholder can be escaped there.
#[derive(Default)]
struct JsonScan {
    in_string: bool,
    escaped: bool,
}

impl JsonScan {
    fn feed(&mut self, literal: &str) {
        for c in literal.chars() {
            if self.escaped {
                self.escaped = false;
            } else if self.in_string && c == '\\' {
                self.escaped = true;
            } else if c == '"' {
                self.in_string = !self.in_string;
            }
        }
    }
}

/// The value escaped for use between the quotes of a JSON string.
fn json_escape(value: &str) -> String {
    let quoted = serde_json::to_string(value).expect("a string always serializes");
    quoted[1..quoted.len() - 1].to_string()
}
