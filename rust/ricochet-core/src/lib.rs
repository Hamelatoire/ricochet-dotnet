//! Port of src/Ricochet.Core (ADR 0091). Ported so far: the data model (models), variable resolution
//! (variables) and built-in functions (builtins). Sending, runs, the workspace store and imports are not
//! ported yet, and the C# app remains the reference for behaviour.

pub mod builtins;
pub mod models;
pub mod variables;
