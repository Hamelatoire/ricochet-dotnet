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
