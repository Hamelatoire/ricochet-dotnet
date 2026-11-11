//! The GPUI shell for Ricochet (ADR 0091). This library holds the view-model logic, which tests reach without a
//! window. The window itself lives in `main.rs`.

pub mod bulk_page;
pub mod mock_page;
pub mod import_page;
pub mod edit;
pub mod exports;
pub mod runner_results;
pub mod palette;
pub mod json_color;
pub mod request_view;
pub mod scope;
pub mod settings_page;
pub mod shell;
pub mod state;
pub mod status;
pub mod text_area;
pub mod text_input;
pub mod text_edit;
pub mod tree;
pub mod tree_edit;
