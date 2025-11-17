/// Console formatting module - Pure rendering concerns
///
/// This module handles all console output formatting including:
/// - Table layout and borders
/// - Color terminal output
/// - Text truncation and padding
/// - Error box rendering
///
/// It accepts pre-formatted data from the report module and renders it to the console.
///
/// ## Output Flexibility
///
/// This module supports writing to any `std::io::Write` destination:
/// - Console (stdout/stderr) with optional colors
/// - String buffers (for markdown/HTML)
/// - Files
/// - Any combination via `TableWriter`

use std::io::{self, Write};
use std::sync::OnceLock;
use term::color::Color;
use terminal_size::{Width, terminal_size};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Writer for table output - configurable for color/plain text
pub struct TableWriter<W: Write> {
    writer: W,
    use_colors: bool,
}

impl<W: Write> TableWriter<W> {
    /// Create a new table writer
    pub fn new(writer: W, use_colors: bool) -> Self {
        Self { writer, use_colors }
    }

    /// Write formatted text, optionally with color
    fn write_colored(&mut self, text: &str, color: Color) -> io::Result<()> {
        if self.use_colors {
            // Use RGB for bright yellow (better Windows Terminal support)
            if color == term::color::BRIGHT_YELLOW {
                write!(self.writer, "\x1b[38;2;255;255;102m{}\x1b[0m", text)
            } else if let Some(ref mut t) = term::stdout() {
                let _ = t.fg(color);
                let _ = t.write_all(text.as_bytes());
                let _ = t.reset();
                Ok(())
            } else {
                write!(self.writer, "{}", text)
            }
        } else {
            write!(self.writer, "{}", text)
        }
    }

    /// Write a newline
    fn writeln(&mut self) -> io::Result<()> {
        writeln!(self.writer)
    }

    /// Write table header
    pub fn write_table_header(
        &mut self,
        crate_name: &str,
        display_version: &str,
        total_deps: usize,
        test_plan: Option<&str>,
        this_path: Option<&str>,
    ) -> io::Result<()> {
        write!(self.writer, "{}", format_table_header(crate_name, display_version, total_deps, test_plan, this_path))
    }

    /// Write table footer
    pub fn write_table_footer(&mut self) -> io::Result<()> {
        write!(self.writer, "{}", format_table_footer())
    }

    /// Write separator line between dependents
    pub fn write_separator_line(&mut self) -> io::Result<()> {
        let w = get_widths();
        writeln!(
            self.writer,
            "├{:─<width1$}┼{:─<width2$}┼{:─<width3$}┼{:─<width4$}┼{:─<width5$}┤",
            "", "", "", "", "",
            width1 = w.offered,
            width2 = w.spec,
            width3 = w.resolved,
            width4 = w.dependent,
            width5 = w.result
        )
    }

    /// Write a main 5-column row with proper formatting and color
    pub fn write_main_row(&mut self, cells: [&str; 5], color: Color) -> io::Result<()> {
        let w = get_widths();
        let displays: Vec<String> = cells
            .iter()
            .zip([w.offered, w.spec, w.resolved, w.dependent, w.result].iter())
            .map(|(cell, width)| truncate_with_padding(cell, width - 2))
            .collect();

        let row = format!("│ {} │ {} │ {} │ {} │ {} │", displays[0], displays[1], displays[2], displays[3], displays[4]);
        self.write_colored(&row, color)?;
        self.writeln()
    }

    /// Write multi-version rows (for transitive dependencies)
    pub fn write_multi_version_rows(&mut self, rows: &[(String, String, String)]) -> io::Result<()> {
        if rows.is_empty() {
            return Ok(());
        }

        let w = get_widths();
        let last_idx = rows.len() - 1;

        for (i, (spec, resolved, dependent)) in rows.iter().enumerate() {
            let prefix = if i == last_idx { "└─" } else { "├─" };
            let spec_display = truncate_with_padding(&format!("{} {}", prefix, spec), w.spec - 2);
            let resolved_display = truncate_with_padding(&format!("{} {}", prefix, resolved), w.resolved - 2);
            let dependent_display = truncate_with_padding(&format!("{} {}", prefix, dependent), w.dependent - 2);

            writeln!(
                self.writer,
                "│{:width1$}│ {} │ {} │ {} │{:width5$}│",
                "",
                spec_display,
                resolved_display,
                dependent_display,
                "",
                width1 = w.offered,
                width5 = w.result
            )?;
        }
        Ok(())
    }

    /// Write error box top
    pub fn write_error_box_top(&mut self) -> io::Result<()> {
        let w = get_widths();
        let error_box_width = w.spec + w.resolved + w.dependent + 6 - 2;

        writeln!(
            self.writer,
            "│{:width1$}│ ╭{:─<error_width$}╮ │{:width5$}│",
            "", "", "",
            width1 = w.offered,
            error_width = error_box_width - 4,
            width5 = w.result
        )
    }

    /// Write error box line
    pub fn write_error_box_line(&mut self, line: &str) -> io::Result<()> {
        let w = get_widths();
        let error_box_width = w.spec + w.resolved + w.dependent + 6 - 2;
        let padded = truncate_with_padding(line, error_box_width - 6);

        writeln!(
            self.writer,
            "│{:width1$}│ │ {} │ │{:width5$}│",
            "", padded, "",
            width1 = w.offered,
            width5 = w.result
        )
    }

    /// Write error box bottom
    pub fn write_error_box_bottom(&mut self) -> io::Result<()> {
        let w = get_widths();
        let error_box_width = w.spec + w.resolved + w.dependent + 6 - 2;

        writeln!(
            self.writer,
            "│{:width1$}│ ╰{:─<error_width$}╯ │{:width5$}│",
            "", "", "",
            width1 = w.offered,
            error_width = error_box_width - 4,
            width5 = w.result
        )
    }

    /// Write comparison table
    pub fn write_comparison_table(&mut self, stats_list: &[ComparisonStats]) -> io::Result<()> {
        if stats_list.is_empty() {
            return Ok(());
        }

        // Calculate column widths
        let label_width = 26;
        let value_width = 16;
        let total_width = label_width + stats_list.len() * value_width;

        // Title
        writeln!(self.writer, "\nVersion Comparison:")?;

        // Headers
        write!(self.writer, "{:<label_width$}", "", label_width = label_width)?;
        for stats in stats_list {
            write!(self.writer, "{:>value_width$}", stats.version_label, value_width = value_width)?;
        }
        writeln!(self.writer)?;

        // Separator
        writeln!(self.writer, "{}", "━".repeat(total_width))?;

        // Write each row
        self.write_simple_row("Total tested", stats_list, |s| s.total_tested)?;

        // Already broken (special case - shows "-" for non-baseline)
        write!(self.writer, "{:<26}", "Already broken")?;
        for stats in stats_list {
            write!(self.writer, "{:>16}", stats.already_broken.map_or("-".to_string(), |c| c.to_string()))?;
        }
        writeln!(self.writer)?;

        writeln!(self.writer, "{}", "━".repeat(total_width))?;

        self.write_delta_row("Passed fetch", stats_list, |s| s.passed_fetch)?;
        self.write_delta_row("Passed check", stats_list, |s| s.passed_check)?;
        self.write_delta_row("Passed test", stats_list, |s| s.passed_test)?;

        writeln!(self.writer, "{}", "━".repeat(total_width))?;

        self.write_delta_row("Fully passing", stats_list, |s| s.fully_passing)?;
        writeln!(self.writer)?;

        Ok(())
    }

    /// Helper to write a simple comparison row (no deltas)
    fn write_simple_row<F>(&mut self, label: &str, stats_list: &[ComparisonStats], get_val: F) -> io::Result<()>
    where
        F: Fn(&ComparisonStats) -> usize,
    {
        write!(self.writer, "{:<26}", label)?;
        for stats in stats_list {
            write!(self.writer, "{:>16}", get_val(stats))?;
        }
        writeln!(self.writer)
    }

    /// Helper to write a comparison row with delta calculation
    fn write_delta_row<F>(&mut self, label: &str, stats_list: &[ComparisonStats], get_val: F) -> io::Result<()>
    where
        F: Fn(&ComparisonStats) -> usize,
    {
        write!(self.writer, "{:<26}", label)?;
        for (i, stats) in stats_list.iter().enumerate() {
            let val = get_val(stats);
            if i == 0 {
                write!(self.writer, "{:>16}", val)?;
            } else {
                let prev = get_val(&stats_list[i - 1]);
                let fixed = if val > prev { val - prev } else { 0 };
                let regressed = if val < prev { prev - val } else { 0 };
                let delta_str = match (fixed, regressed) {
                    (0, 0) => format!("{}", val),
                    (f, 0) => format!("+{} → {}", f, val),
                    (0, r) => format!("-{} → {}", r, val),
                    (f, r) => format!("+{} -{} → {}", f, r, val),
                };
                write!(self.writer, "{:>16}", delta_str)?;
            }
        }
        writeln!(self.writer)
    }
}

//
// Table Layout and Widths
//

/// Column widths for the 5-column table
#[derive(Clone, Copy)]
pub struct TableWidths {
    pub offered: usize,
    pub spec: usize,
    pub resolved: usize,
    pub dependent: usize,
    pub result: usize,
    pub total: usize, // Total table width including borders
}

impl TableWidths {
    pub fn new(terminal_width: usize) -> Self {
        Self::new_with_offered(terminal_width, None)
    }

    pub fn new_with_offered(terminal_width: usize, offered_width: Option<usize>) -> Self {
        // Borders: │ = 6 characters (1 before each column + 1 at end)
        let borders = 6;
        let available = terminal_width.saturating_sub(borders);

        // Use fixed widths for columns with known/predictable values
        // Offered: use provided width or default to 25
        let offered = offered_width.unwrap_or(25);
        // Spec: "^0.8.52" or "→ =this" max ~12 chars
        let spec = 12;
        // Resolved: "0.8.91-preview 📦" max ~18 chars
        let resolved = 18;
        // Result: "build failed ✓✗-  1.3s" fixed ~25 chars
        let result = 25;

        // Dependent gets remaining space (for long crate names)
        let fixed_total = offered + spec + resolved + result;
        let dependent = if available > fixed_total {
            available - fixed_total
        } else {
            20 // Minimum fallback
        };

        TableWidths { offered, spec, resolved, dependent, result, total: terminal_width }
    }

    /// Calculate minimum offered column width for given versions
    pub fn calculate_offered_width(versions: &[String], _display_version: &str, force_versions: bool) -> usize {
        let mut max_width = "- baseline".len(); // 10 chars

        // Forced marker is 6 chars: " [≠→!]"
        let forced_width = if force_versions { 6 } else { 0 };

        // Check all test versions
        for version in versions {
            // Format: "{icon} {resolution}{version}[ [≠→!]]"
            // Icon (1) + space (1) + resolution (1) + version + optional forced marker
            let width = 1 + 1 + 1 + version.len() + forced_width;
            max_width = max_width.max(width);
        }

        // Add generous padding for comfortable spacing (6 chars breathing room)
        max_width + 6
    }
}

/// Get terminal width or default to 120
fn get_terminal_width() -> usize {
    if let Some((Width(w), _)) = terminal_size() {
        w as usize
    } else {
        120 // Default width
    }
}

// Table widths - initialized once with actual version data
static WIDTHS: OnceLock<TableWidths> = OnceLock::new();

/// Initialize table widths based on versions being tested
pub fn init_table_widths(versions: &[String], display_version: &str, force_versions: bool) {
    let offered_width = TableWidths::calculate_offered_width(versions, display_version, force_versions);
    let widths = TableWidths::new_with_offered(get_terminal_width(), Some(offered_width));
    let _ = WIDTHS.set(widths); // Ignore error if already initialized
}

/// Get table widths (with fallback to defaults if not initialized)
pub fn get_widths() -> &'static TableWidths {
    WIDTHS.get_or_init(|| TableWidths::new(get_terminal_width()))
}

//
// Text Formatting Utilities
//

/// Count the display width of a string, accounting for wide Unicode characters
pub fn display_width(s: &str) -> usize {
    // Use unicode-width crate for accurate width calculation
    UnicodeWidthStr::width(s)
}

/// Truncate and pad string to exact width
pub fn truncate_with_padding(s: &str, width: usize) -> String {
    let display_w = display_width(s);

    if display_w > width {
        // Truncate
        let mut result = String::new();
        let mut current_width = 0;
        let chars: Vec<char> = s.chars().collect();

        // Reserve space for "..."
        let target_width = if width >= 3 { width - 3 } else { width };

        for c in chars.iter() {
            let c_width = UnicodeWidthChar::width(*c).unwrap_or(1);

            if current_width + c_width > target_width {
                break;
            }

            result.push(*c);
            current_width += c_width;
        }

        if width >= 3 {
            result.push_str("...");
            current_width += 3;
        }

        // Pad if needed
        if current_width < width {
            result.push_str(&" ".repeat(width - current_width));
        }

        result
    } else {
        // Pad with spaces to reach the width
        let padding = width - display_w;
        format!("{}{}", s, " ".repeat(padding))
    }
}

//
// Table Header/Footer Rendering
//

/// Format table header as a string
pub fn format_table_header(
    crate_name: &str,
    display_version: &str,
    total_deps: usize,
    test_plan: Option<&str>,
    this_path: Option<&str>,
) -> String {
    let w = get_widths();

    let mut output = String::new();
    output.push('\n');

    // Show "Testing X dependencies" first
    output.push_str(&format!("Testing {} reverse dependencies of {}\n", total_deps, crate_name));

    // Include test plan if provided (dependents and versions lines)
    if let Some(plan) = test_plan {
        output.push_str(plan);
        output.push('\n');
    }

    // Format "this =" line with optional path
    let this_line = if let Some(path) = this_path {
        format!("  this = {} ({})", display_version, path)
    } else {
        format!("  this = {} (your work-in-progress version)", display_version)
    };
    output.push_str(&format!("{}\n", this_line));

    output.push('\n');

    output.push_str(&format!(
        "┌{:─<width1$}┬{:─<width2$}┬{:─<width3$}┬{:─<width4$}┬{:─<width5$}┐\n",
        "",
        "",
        "",
        "",
        "",
        width1 = w.offered,
        width2 = w.spec,
        width3 = w.resolved,
        width4 = w.dependent,
        width5 = w.result
    ));
    output.push_str(&format!(
        "│{:^width1$}│{:^width2$}│{:^width3$}│{:^width4$}│{:^width5$}│\n",
        "Offered",
        "Spec",
        "Resolved",
        "Dependent",
        "Result         Time",
        width1 = w.offered,
        width2 = w.spec,
        width3 = w.resolved,
        width4 = w.dependent,
        width5 = w.result
    ));
    output.push_str(&format!(
        "├{:─<width1$}┼{:─<width2$}┼{:─<width3$}┼{:─<width4$}┼{:─<width5$}┤\n",
        "",
        "",
        "",
        "",
        "",
        width1 = w.offered,
        width2 = w.spec,
        width3 = w.resolved,
        width4 = w.dependent,
        width5 = w.result
    ));

    output
}

/// Print table header to stdout with colors
pub fn print_table_header(
    crate_name: &str,
    display_version: &str,
    total_deps: usize,
    test_plan: Option<&str>,
    this_path: Option<&str>,
) {
    let mut writer = TableWriter::new(io::stdout(), false); // No colors for header
    let _ = writer.write_table_header(crate_name, display_version, total_deps, test_plan, this_path);
}

/// Format table footer as a string
pub fn format_table_footer() -> String {
    let w = get_widths();
    format!(
        "└{:─<width1$}┴{:─<width2$}┴{:─<width3$}┴{:─<width4$}┴{:─<width5$}┘\n",
        "",
        "",
        "",
        "",
        "",
        width1 = w.offered,
        width2 = w.spec,
        width3 = w.resolved,
        width4 = w.dependent,
        width5 = w.result
    )
}

/// Print table footer to stdout
pub fn print_table_footer() {
    let mut writer = TableWriter::new(io::stdout(), false);
    let _ = writer.write_table_footer();
}

/// Print separator line between dependents to stdout
pub fn print_separator_line() {
    let mut writer = TableWriter::new(io::stdout(), false);
    let _ = writer.write_separator_line();
}

//
// Row Rendering
//

/// Print a main 5-column row with proper formatting and color to stdout
pub fn print_main_row(cells: [&str; 5], color: Color) {
    let mut writer = TableWriter::new(io::stdout(), true); // Enable colors
    let _ = writer.write_main_row(cells, color);
}

/// Print multi-version dependency rows to stdout
pub fn print_multi_version_rows(rows: &[(String, String, String)]) {
    let mut writer = TableWriter::new(io::stdout(), false);
    let _ = writer.write_multi_version_rows(rows);
}

//
// Error Box Rendering
//

/// Helper to print error box top border
pub fn print_error_box_top() {
    let w = get_widths();
    let shortened_offered = 4;
    let corner0_width = if shortened_offered != w.offered { w.offered - shortened_offered - 1 } else { 0 };

    if corner0_width > 0 {
        println!(
            "│{:shortened$}┌{:─<c0$}┴{:─<c1$}┘{:padding$}└{:─<c2$}┘{:result$}│",
            "",
            "",
            "",
            "",
            "",
            "",
            shortened = shortened_offered,
            c0 = corner0_width,
            c1 = w.spec,
            padding = w.resolved,
            c2 = w.dependent,
            result = w.result
        );
    } else {
        println!(
            "│{:offered$}├{:─<spec$}┘{:padding$}└{:─<dep$}┘{:result$}│",
            "",
            "",
            "",
            "",
            "",
            offered = w.offered,
            spec = w.spec,
            padding = w.resolved,
            dep = w.dependent,
            result = w.result
        );
    }
}

/// Helper to print error box content line
pub fn print_error_box_line(line: &str) {
    let w = get_widths();
    let shortened_offered = 4;
    let error_text_width = w.total - 1 - shortened_offered - 1 - 1 - 1 - 1;
    let truncated = truncate_with_padding(line, error_text_width);
    println!("│{:shortened$}│ {} │", "", truncated, shortened = shortened_offered);
}

/// Helper to print error box bottom border (transitioning back to main table)
pub fn print_error_box_bottom() {
    let w = get_widths();
    let shortened_offered = 4;
    let corner0_width = if shortened_offered != w.offered { w.offered - shortened_offered - 1 } else { 0 };

    if corner0_width > 0 {
        println!(
            "│{:shortened$}└{:─<c0$}┬{:─<c1$}┬{:─<c2$}┬{:─<c3$}┬{:─<c4$}┤",
            "",
            "",
            "",
            "",
            "",
            "",
            shortened = shortened_offered,
            c0 = corner0_width,
            c1 = w.spec,
            c2 = w.resolved,
            c3 = w.dependent,
            c4 = w.result
        );
    } else {
        println!(
            "│{:offered$}├{:─<spec$}┬{:─<resolved$}┬{:─<dep$}┬{:─<result$}┤",
            "",
            "",
            "",
            "",
            "",
            offered = w.offered,
            spec = w.spec,
            resolved = w.resolved,
            dep = w.dependent,
            result = w.result
        );
    }
}

//
// Comparison Table Rendering
//

/// Statistics for comparison table
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ComparisonStats {
    pub version_label: String, // "Default" or version number
    pub total_tested: usize,
    pub already_broken: Option<usize>, // Only for baseline
    pub passed_fetch: usize,
    pub passed_check: usize,
    pub passed_test: usize,
    pub fully_passing: usize,
    pub regressions: Vec<String>, // List of "dependent:version" that regressed
}

/// Print comparison table to stdout
pub fn print_comparison_table(stats_list: &[ComparisonStats]) {
    let mut writer = TableWriter::new(io::stdout(), false);
    let _ = writer.write_comparison_table(stats_list);
}

#[cfg(test)]
#[path = "console_format_test.rs"]
mod console_format_test;
