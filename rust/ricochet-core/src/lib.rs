//! Port of src/Ricochet.Core (ADR 0091). Ported so far: the data model (models), variable resolution
//! (variables), built-in functions (builtins), and assertions and captures (assertions). Sending, runs, the
//! workspace store and imports are not ported yet, and the C# app remains the reference for behaviour.

pub mod assertions;
pub mod builtins;
pub mod dotenv;
pub mod models;
pub mod response_compare;
pub mod variables;
pub mod docs_html;
pub mod data_file;
pub mod curl;
pub mod postman_export;
pub mod openapi_export;
pub mod duplicate;
pub mod name_filter;
pub mod postman;
pub mod insomnia;
pub mod store;
pub mod openapi;
pub mod response_status;
pub mod key_value_columns;
pub mod folder_tags;
pub mod shortcut_sheet;
pub mod settings_catalog;
pub mod command_palette;
pub mod sidebar_menu;
pub mod export_files;
pub mod placeholder_chips;
pub mod git_status;
pub mod crash_log;
pub mod environment_transfer;
pub mod git_clone;
pub mod collection_export;
pub mod auth_signing;
pub mod mock_server;
pub mod workspace_archive;
pub mod send;
pub mod response_format;
pub mod response_examples;
