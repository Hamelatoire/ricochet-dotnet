//! Port of src/Ricochet.Core (ADR 0091). Ported so far: the data model (models), variable resolution
//! (variables), built-in functions (builtins), and assertions and captures (assertions). Sending, runs, the
//! workspace store and imports are not ported yet, and the C# app remains the reference for behaviour.

pub mod assertions;
pub mod builtins;
pub mod models;
pub mod variables;
