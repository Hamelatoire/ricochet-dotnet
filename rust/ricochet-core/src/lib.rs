//! Port of `src/Ricochet.Core` (ADR 0091). Ported so far: the data model (`models`) and
//! variable resolution (`variables`). Sending, runs, the workspace store and imports are not
//! ported yet, and the C# app remains the reference for behaviour.

pub mod models;
pub mod variables;
