//! The Variables a send of one Request can see, layered as the C# app layers them (ADR 0009, ADR 0020):
//! globals, the Collection, the Folders around the Request, the active Environment, then the `.env` file.
//! Captured values are not held by the window yet, so that layer is absent here.

use ricochet_core::models::{Collection, CollectionItem, KeyValue, Workspace};
use ricochet_core::variables::{folder_scope, layer};

/// The Collection that holds the item with this id, wherever it sits inside it.
pub fn collection_of<'a>(workspace: &'a Workspace, item_id: &str) -> Option<&'a Collection> {
    workspace.collections.iter().find(|collection| holds(&collection.items, item_id))
}

fn holds(items: &[CollectionItem], item_id: &str) -> bool {
    items.iter().any(|item| item.id == item_id || holds(&item.children, item_id))
}

/// Every Variable a send of the Request can see, lowest priority first.
pub fn variables_in_scope(
    workspace: &Workspace,
    request_id: &str,
    active_environment_id: Option<&str>,
    dot_env: &[KeyValue],
) -> Vec<KeyValue> {
    let collection = collection_of(workspace, request_id);
    let collection_variables: &[KeyValue] = collection.map(|c| c.variables.as_slice()).unwrap_or(&[]);
    let folders = collection.map(|c| folder_scope(c, Some(request_id))).unwrap_or_default();
    let environment: &[KeyValue] = active_environment_id
        .and_then(|id| workspace.environments.iter().find(|environment| environment.id == id))
        .map(|environment| environment.variables.as_slice())
        .unwrap_or(&[]);
    layer(&[&workspace.globals, collection_variables, &folders, environment, dot_env])
}
