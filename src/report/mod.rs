//! Report generation module - Data transformations and business logic.
//!
//! This module handles:
//! - Converting OfferedRow to FormattedRow (business logic)
//! - Calculating statistics and summaries
//! - Generating comparison tables
//! - Error signature extraction for deduplication
//! - Export to JSON and Markdown formats
//! - Simple (AI-friendly) output format
//!
//! Console rendering is handled by the console_format module.
//!
//! # Module Organization
//!
//! - `types` - Core rendering types (StatusIcon, Resolution, OfferedCell, FormattedRow)
//! - `stats` - Summary statistics and comparison table generation
//! - `export` - JSON and Markdown export, failure logs
//! - `table` - Console table output and error deduplication
//! - `simple` - AI-friendly verbal output format

mod export;
mod simple;
mod stats;
mod table;
mod types;

// Re-export types
pub use types::{DependentResults, FormattedRow, OfferedCell, Resolution, StatusIcon, TestSummary};

// Re-export the internal format function for use by table and export
pub(crate) use types::format_offered_row;

// Re-export stats functions
pub use stats::{generate_comparison_table, summarize_offered_rows};

// Re-export export functions
pub use export::{export_json_report, export_markdown_table_report, write_combined_log, write_failure_log};

// Re-export table functions
pub use table::{
    error_signature, extract_error_text, format_offered_row_string, format_table_footer, format_table_header,
    init_table_widths, print_comparison_table, print_offered_row, print_separator_line, print_table_footer,
    print_table_header,
};

// Re-export simple output functions
pub use simple::{print_simple_dependent_result, print_simple_header, print_simple_summary};
