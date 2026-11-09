pub mod diff;
pub mod normalize;
pub mod report;

pub use diff::{diff_specs, ContractDiff};
pub use normalize::normalize_spec;
pub use report::{render_html, render_markdown};
