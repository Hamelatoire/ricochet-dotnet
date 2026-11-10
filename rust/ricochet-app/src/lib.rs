//! The GPUI shell for Ricochet (ADR 0091). This library holds the view-model logic, which tests reach without a
//! window. The window itself lives in `main.rs`.

pub mod edit;
pub mod json_color;
pub mod request_view;
pub mod scope;
pub mod shell;
pub mod state;
pub mod status;
pub mod tree;
