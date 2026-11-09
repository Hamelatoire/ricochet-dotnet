//! Port of `src/Ricochet.Core` (ADR 0091, PR 3). This first slice holds the data model only:
//! the types that the Workspace file format is made of. Sending, variables, runs and imports
//! are not ported yet, and the C# app remains the reference for behaviour.

pub mod models;
