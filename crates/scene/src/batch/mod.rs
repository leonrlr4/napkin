//! One `napkin apply` batch (AI spec §4): parsing, the `add` skeleton conversion, `update` and
//! `delete`, applied all-or-nothing.

mod add;
mod validate;

pub use add::{Added, LabelSpec, add_elements};

/// One op's validation failure, positioned within a batch for the client to report.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct OpError {
    /// Position of the offending op in `ops`; `None` for an error about the batch as a whole.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub op: Option<usize>,
    /// JSON path inside that op, e.g. `"label.fontSize"` or `"points[2]"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    pub message: String,
}
