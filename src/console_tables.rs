//! Console table formatting with box-drawing characters
//!
//! This module handles the generation of separator rows for console tables
//! with proper box-drawing character selection based on column layouts.
//!
//! ## Status: Work in Progress
//!
//! ### Completed:
//! - **Asymmetric separation API**: ColSize now has separate `draw_horizontal_line_above`
//!   and `draw_horizontal_line_below` fields because a single row needs different separation
//!   preferences depending on which separator is being generated (above vs below).
//! - **Convenience constructors**: `new()` for symmetric separation, `new_asymmetric()` for
//!   asymmetric separation needs.
//! - **Border character logic**: Left and right borders correctly show ┌ ┐ └ ┘ ├ ┤ based on
//!   the first and last column's separation preferences.
//!
//! ### In Progress (lines 91-130):
//! - **Overlapping open regions**: When two rows have misaligned columns with no explicit
//!   horizontal line requests, overlapping regions should automatically generate connector
//!   characters (corners and dashes) to create visual continuity.
//!
//! **Example**: Rows [13,10] → [4,19] with no horizontal lines requested should produce:
//!   ```
//!   │    ┌────────┘          │
//!   ```
//!   - Spaces at edges (rowspan areas where only one row has content)
//!   - `┌` corner at ~position 4 (overlap region starts)
//!   - Dashes in middle (overlapping region between boundaries)
//!   - `┘` corner at ~position 13 (overlap region ends)
//!   - Spaces on right (rowspan area)
//!
//! **Current approach (lines 91-130)**: Using even/odd toggle algorithm to detect regions
//! BETWEEN boundaries (not continuous rowspan). The algorithm mutates `resolved_cols` during
//! preprocessing to enable horizontal lines only in overlapping open regions.
//!
//! **Current issue**: Toggle detection (lines 112-120) checks mutated `draw_horizontal_line`
//! flags instead of original flags, so it can't correctly detect overlapping open regions.
//! Current output: `│    │────────│          │` (has dashes but wrong junction characters).
//!
//! **Next step**: Store original `draw_horizontal_line` flags before the mutation loop, then
//! use those original flags to detect overlapping open regions and toggle state appropriately.
//!
//! ### Test Cases:
//! - `test_simple_overlap()`: Basic misaligned columns [5,10] → [10,5]
//! - `test_complex_table_output()`: Full table demonstrating all separator scenarios
//!
//! See /tmp/analyze_table.txt for visual analysis of expected separator patterns.

/// Column descriptor for separator generation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColSize {
    pub width: usize,
    pub draw_horizontal_line_above: bool,  // Does this row want separator above it?
    pub draw_horizontal_line_below: bool,  // Does this row want separator below it?
}

impl ColSize {
    /// Convenience constructor for columns that want separation in both directions
    pub fn new(width: usize, draw_horizontal_line: bool) -> Self {
        Self {
            width,
            draw_horizontal_line_above: draw_horizontal_line,
            draw_horizontal_line_below: draw_horizontal_line,
        }
    }

    /// Convenience constructor for columns with asymmetric separation needs
    pub fn new_asymmetric(width: usize, above: bool, below: bool) -> Self {
        Self {
            width,
            draw_horizontal_line_above: above,
            draw_horizontal_line_below: below,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ResolvedColSize {
    pub width: usize,
    pub offset: usize, // offset from 0
    pub from_above: bool,  // Is this column from the previous row (true) or the next row (false)?
    pub draw_horizontal_line: bool,  // Does this column want horizontal line at this separator?
    // For from_above=true: uses draw_horizontal_line_below
    // For from_above=false: uses draw_horizontal_line_above
}

/// Generate a separator row between two table rows with different column layouts
///
/// This function generates the appropriate box-drawing characters to connect
/// vertical dividers and horizontal separation lines based on the column layouts
/// of the previous and next rows.
///
/// # Arguments
/// * `previous_columns` - Column layout of the row above the separator
/// * `next_columns` - Column layout of the row below the separator
///
/// # Returns
/// A string containing the separator row with appropriate box-drawing characters
pub fn format_separator_row(
    previous_columns: &[ColSize],
    next_columns: &[ColSize],
) -> String {
    // Step 1: Resolve all column sizes with their offsets
    let mut resolved_cols = Vec::new();

    // Process previous row columns (use draw_horizontal_line_below)
    let mut offset = 0;
    for col in previous_columns {
        resolved_cols.push(ResolvedColSize {
            width: col.width,
            offset,
            from_above: true,
            draw_horizontal_line: col.draw_horizontal_line_below,
        });
        offset += col.width + 1; // +1 for vertical divider between columns
    }
    let prev_total_width = offset.saturating_sub(1); // Remove the last divider

    // Process next row columns (use draw_horizontal_line_above)
    offset = 0;
    for col in next_columns {
        resolved_cols.push(ResolvedColSize {
            width: col.width,
            offset,
            from_above: false,
            draw_horizontal_line: col.draw_horizontal_line_above,
        });
        offset += col.width + 1;
    }
    let next_total_width = offset.saturating_sub(1);

    let total_width = prev_total_width.max(next_total_width);

    // Step 1.5: Detect overlapping open regions and enable horizontal lines
    // Use even/odd toggle: enable lines only BETWEEN boundaries (not continuous rowspan)
    //
    // TODO: This implementation is incomplete. The toggle detection checks mutated flags
    // instead of original flags. To fix:
    //   1. Before this loop, store: let original_flags: Vec<bool> = resolved_cols.iter().map(|c| c.draw_horizontal_line).collect();
    //   2. In toggle detection below, check original_flags[cpi] and original_flags[cni]
    //   3. This will correctly identify overlapping open regions
    //
    // Current behavior: Produces │    │────────│          │ (dashes but wrong junctions)
    // Expected behavior: Should produce │    ┌────────┘          │ (corners at boundaries)
    let mut in_overlap_region = false;
    for pos in 0..total_width {
        // Find indices of columns at this position
        let prev_idx = resolved_cols.iter().position(|c|
            c.from_above && pos >= c.offset && pos < c.offset + c.width);
        let next_idx = resolved_cols.iter().position(|c|
            !c.from_above && pos >= c.offset && pos < c.offset + c.width);

        // Check if there's a divider at this position
        let is_divider = resolved_cols.iter().any(|c| pos == c.offset + c.width);

        // Toggle at dividers when previous position had overlapping open columns
        if is_divider && pos > 0 {
            let check_pos = pos - 1;
            let check_prev_idx = resolved_cols.iter().position(|c|
                c.from_above && check_pos >= c.offset && check_pos < c.offset + c.width);
            let check_next_idx = resolved_cols.iter().position(|c|
                !c.from_above && check_pos >= c.offset && check_pos < c.offset + c.width);

            if let (Some(cpi), Some(cni)) = (check_prev_idx, check_next_idx) {
                // BUG: These flags have been mutated in previous iterations!
                // We need to check ORIGINAL flags to detect overlapping open regions
                let prev_hl = resolved_cols[cpi].draw_horizontal_line;
                let next_hl = resolved_cols[cni].draw_horizontal_line;
                if !prev_hl || !next_hl {  // At least one didn't originally want lines
                    in_overlap_region = !in_overlap_region;
                }
            }
        }

        // Enable horizontal lines when we're in an overlap region
        if in_overlap_region {
            if let Some(pi) = prev_idx {
                if !resolved_cols[pi].draw_horizontal_line {
                    resolved_cols[pi].draw_horizontal_line = true;
                }
            }
        }
    }

    // Step 2: Determine border characters
    // Left border (based on first column)
    let first_prev_sep = previous_columns.first().map(|c| c.draw_horizontal_line_below).unwrap_or(false);
    let first_next_sep = next_columns.first().map(|c| c.draw_horizontal_line_above).unwrap_or(false);
    let left_char = match (first_prev_sep, first_next_sep) {
        (true, true) => '├',
        (true, false) => '└',
        (false, true) => '┌',
        (false, false) => '│',
    };

    // Right border (based on last column)
    let last_prev_sep = previous_columns.last().map(|c| c.draw_horizontal_line_below).unwrap_or(false);
    let last_next_sep = next_columns.last().map(|c| c.draw_horizontal_line_above).unwrap_or(false);
    let right_char = match (last_prev_sep, last_next_sep) {
        (true, true) => '┤',
        (true, false) => '┘',
        (false, true) => '┐',
        (false, false) => '│',
    };

    let mut result = String::new();
    result.push(left_char);

    // Step 3: Track state for overlapping open regions
    let mut in_open_overlap = false;

    // Step 4: For each position, determine what character to draw
    for pos in 0..total_width {
        // Find which columns occupy this position
        let prev_col = resolved_cols.iter()
            .find(|c| c.from_above && pos >= c.offset && pos < c.offset + c.width);
        let next_col = resolved_cols.iter()
            .find(|c| !c.from_above && pos >= c.offset && pos < c.offset + c.width);

        // Check if this is a divider position (end of column + 1)
        let is_prev_divider = resolved_cols.iter()
            .any(|c| c.from_above && pos == c.offset + c.width);
        let is_next_divider = resolved_cols.iter()
            .any(|c| !c.from_above && pos == c.offset + c.width);

        // Determine if horizontal lines are wanted
        let prev_wants_line = prev_col.map(|c| c.draw_horizontal_line).unwrap_or(false);
        let next_wants_line = next_col.map(|c| c.draw_horizontal_line).unwrap_or(false);

        let ch = if is_prev_divider || is_next_divider {
            // At a vertical divider position - need junction character
            // Check what's to the left (previous position)
            let pos_left = pos.saturating_sub(1);
            let left_prev_col = resolved_cols.iter()
                .find(|c| c.from_above && pos_left >= c.offset && pos_left < c.offset + c.width);
            let left_next_col = resolved_cols.iter()
                .find(|c| !c.from_above && pos_left >= c.offset && pos_left < c.offset + c.width);
            let left_prev_wants = left_prev_col.map(|c| c.draw_horizontal_line).unwrap_or(false);
            let left_next_wants = left_next_col.map(|c| c.draw_horizontal_line).unwrap_or(false);

            // What's to the right (next position after divider)
            let pos_right = pos + 1;
            let right_prev_col = resolved_cols.iter()
                .find(|c| c.from_above && pos_right >= c.offset && pos_right < c.offset + c.width);
            let right_next_col = resolved_cols.iter()
                .find(|c| !c.from_above && pos_right >= c.offset && pos_right < c.offset + c.width);
            let right_prev_wants = right_prev_col.map(|c| c.draw_horizontal_line).unwrap_or(false);
            let right_next_wants = right_next_col.map(|c| c.draw_horizontal_line).unwrap_or(false);

            // Determine junction based on horizontal lines (overlaps already mutated above)
            let has_left = left_prev_wants || left_next_wants;
            let has_right = right_prev_wants || right_next_wants;
            let has_up = is_prev_divider;
            let has_down = is_next_divider;

            match (has_up, has_down) {
                (true, true) => {
                    // Both rows have dividers - use combined logic
                    match (has_left, has_right) {
                        (true, true) => '┼',
                        (true, false) => '┤',
                        (false, true) => '├',
                        (false, false) => '│',
                    }
                }
                (true, false) => {
                    // Only previous row has divider - but check both rows for horizontal lines
                    match (has_left, has_right) {
                        (true, true) => '┴',
                        (true, false) => '┘',
                        (false, true) => '└',
                        (false, false) => '│',
                    }
                }
                (false, true) => {
                    // Only next row has divider - but check both rows for horizontal lines
                    match (has_left, has_right) {
                        (true, true) => '┬',
                        (true, false) => '┐',
                        (false, true) => '┌',
                        (false, false) => '│',
                    }
                }
                (false, false) => unreachable!(), // Can't be at divider with no dividers
            }
        } else {
            // Not at a divider - just content space
            // Draw dashes if explicitly requested OR if we're in an overlapping open region
            if prev_wants_line || next_wants_line || in_open_overlap {
                '─'
            } else {
                ' '
            }
        };

        result.push(ch);

        // Toggle state AFTER processing this position
        // At dividers, check if we're entering/exiting an overlapping open region
        if is_prev_divider || is_next_divider {
            let check_pos = pos.saturating_sub(1);
            let check_prev = resolved_cols.iter()
                .find(|c| c.from_above && check_pos >= c.offset && check_pos < c.offset + c.width);
            let check_next = resolved_cols.iter()
                .find(|c| !c.from_above && check_pos >= c.offset && check_pos < c.offset + c.width);

            if let (Some(p), Some(n)) = (check_prev, check_next) {
                // Had overlap at previous position
                if !p.draw_horizontal_line && !n.draw_horizontal_line {
                    // Both were open - toggle state
                    in_open_overlap = !in_open_overlap;
                }
            }
        }
    }

    result.push(right_char);
    result.push('\n');
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_box_drawing_characters() {
        // Create a layout that exercises all 11 box-drawing characters plus space
        // Designed to create all possible junction scenarios:
        //
        // Row 1: [4,no] [6,yes] [5,no] [6,yes] [4,no] [6,yes] [5,no]
        // Row 2: [2,yes] [3,no] [10,yes] [5,no] [4,yes] [4,no] [8,yes]
        //
        // This creates:
        // - ├ ┤ ┬ ┴ ┼: various T-junctions and crosses
        // - ┌ ┐ └ ┘: corners where dividers start/end with different hl combinations
        // - ─: horizontal lines
        // - │: vertical lines continuing
        // - space: rowspan areas

        // Carefully designed to hit all 11 characters:
        // Position:  0-1  2-4  5-6  7-11  12-15  16-19  20-23  24-27  28-31
        let row1 = vec![
            ColSize::new(2, false),  // 0-1: rowspan
            ColSize::new(3, true),   // 2-4: div@5, ├
            ColSize::new(2, true),   // 6-7: div@8, for corners
            ColSize::new(5, true),   // 9-13: div@14, ┼
            ColSize::new(4, true),   // 15-18: div@19, ┤/┴
            ColSize::new(4, true),   // 20-23: div@24, ┘
            ColSize::new(4, false),  // 25-28: no div, space/corner
            ColSize::new(3, true),   // 29-31: div@32
        ];

        let row2 = vec![
            ColSize::new(2, false),  // 0-1: rowspan, │
            ColSize::new(3, true),   // 2-4: div@5, ├ (aligned!)
            ColSize::new(1, false),  // 6: no div, for ┌
            ColSize::new(2, true),   // 7-8: div@9, ┐
            ColSize::new(5, true),   // 10-14: div@15, ┼ (almost aligned)
            ColSize::new(4, false),  // 16-19: no div, └
            ColSize::new(5, true),   // 20-24: div@25, ┬
            ColSize::new(3, true),   // 26-28: div@29, corners
            ColSize::new(4, true),   // 29-32: ┤
        ];

        let result = format_separator_row(&row1, &row2);

        // Print for visual inspection FIRST
        eprintln!("Main test result:");
        eprintln!("{}", result);

        // Additional test for corners (since they're hard to hit in one layout)
        // Corners need non-aligned dividers with specific hl patterns
        let corners_row1 = vec![
            ColSize::new(3, false),
            ColSize::new(4, true),   // div@8
            ColSize::new(3, false),
        ];
        let corners_row2 = vec![
            ColSize::new(2, false),
            ColSize::new(2, true),   // div@5 for ┌
            ColSize::new(3, false),
            ColSize::new(3, true),   // div@14 for ┐/└/┘
        ];
        let corner_result = format_separator_row(&corners_row1, &corners_row2);
        eprintln!("Corner test: {}", corner_result);

        // Test for ┐ (top-right): divider in next row only, hl to left, no hl to right
        let topright_row1 = vec![
            ColSize::new(3, true),   // 0-2, div@3
            ColSize::new(2, false),  // 4-5, div@6
        ];
        let topright_row2 = vec![
            ColSize::new(5, true),   // 0-4, div@5
            ColSize::new(1, false),  // 6, div@7
        ];
        let topright_result = format_separator_row(&topright_row1, &topright_row2);
        eprintln!("Top-right test: {}", topright_result);

        // Test for ┤ (right-tee): aligned dividers, hl to left, no hl to right
        let righttee_row1 = vec![
            ColSize::new(3, true),   // 0-2, div@3
            ColSize::new(2, false),  // 4-5, div@6
        ];
        let righttee_row2 = vec![
            ColSize::new(3, true),   // 0-2, div@3, aligned
            ColSize::new(2, false),  // 4-5, div@6, aligned
        ];
        let righttee_result = format_separator_row(&righttee_row1, &righttee_row2);
        eprintln!("Right-tee test: {}", righttee_result);

        // Combine all results for comprehensive character check
        let combined = format!("{}{}{}{}", result, corner_result, topright_result, righttee_result);

        // Check for all 11 characters plus space
        assert!(combined.contains('─'), "Missing: ─ (horizontal line)");
        assert!(combined.contains('│'), "Missing: │ (vertical line)");
        assert!(combined.contains('┌'), "Missing: ┌ (top-left corner)");
        assert!(combined.contains('┐'), "Missing: ┐ (top-right corner)");
        assert!(combined.contains('└'), "Missing: └ (bottom-left corner)");
        assert!(combined.contains('┘'), "Missing: ┘ (bottom-right corner)");
        assert!(combined.contains('├'), "Missing: ├ (left tee)");
        assert!(combined.contains('┤'), "Missing: ┤ (right tee)");
        assert!(combined.contains('┬'), "Missing: ┬ (down tee)");
        assert!(combined.contains('┴'), "Missing: ┴ (up tee)");
        assert!(combined.contains('┼'), "Missing: ┼ (cross)");
        assert!(combined.contains(' '), "Missing: space (rowspan)");

        // Check for some clearly invalid patterns
        // Note: "─┐│" can be valid when │ is at the border
        assert!(!combined.contains("─└"), "Invalid pattern: ─└");
        assert!(!combined.contains("│ │"), "Invalid pattern: │ │");
    }

    #[test]
    fn test_simple_overlap() {
        // Test your suggestion: [5,10] and [10,5] with no hl
        let row1 = vec![ColSize::new(5, false), ColSize::new(10, false)];
        let row2 = vec![ColSize::new(10, false), ColSize::new(5, false)];

        let result = format_separator_row(&row1, &row2);
        eprintln!("Simple overlap [5,10]->[10,5] no hl:\n{}", result);

        // With partial hl
        let row1_hl = vec![
            ColSize::new_asymmetric(5, false, true),
            ColSize::new_asymmetric(10, false, false),
        ];
        let result2 = format_separator_row(&row1_hl, &row2);
        eprintln!("With row1[0] hl_below:\n{}", result2);
    }

    #[test]
    fn test_complex_table_output() {
        // Test a complete multi-row table with varying column layouts
        // This tests the function working across multiple separator rows
        //
        // NOTE: Line 3 `│    ┌────────┘          │` has internal nested structure where
        // a single column needs different horizontal line settings at different positions
        // (spaces at edges, dashes in middle). Current per-column API can't represent this.
        // Would need either: finer column splitting or per-position control.

        let expected_output = r#"┌──┬──────────┬──────────┐
├──┴──────────┼──────────┤
│    ┌────────┘          │
├──┬─┴────────┬──────────┤
├──┴──────────┴─┐        │
└───────────────┴────────┘"#;

        let mut result = String::new();

        // Row 0: 3 columns [2, 10, 10]
        let row0 = vec![
            ColSize::new(2, true),
            ColSize::new(10, true),
            ColSize::new(10, true),
        ];

        // Top border (no row before)
        result.push_str(&format_separator_row(&vec![], &row0));

        // Row 1: wants separation above for line 2, none below for line 3
        let row1 = vec![
            ColSize::new_asymmetric(13, true, false),   // Want sep above, not below
            ColSize::new_asymmetric(10, true, false),   // Want sep above, not below
        ];
        result.push_str(&format_separator_row(&row0, &row1));

        // Row 2: misaligned columns [4, 19] - overlaps with row1 [13, 10]
        // No explicit hl request, but overlapping columns create connectors automatically!
        let row2 = vec![
            ColSize::new(4, false),    // Overlaps with row1[0] at positions 0-3
            ColSize::new(19, false),   // Overlaps with row1[0] at 5-12, with row1[1] at 14-22
        ];
        result.push_str(&format_separator_row(&row1, &row2));

        // Row 3: back to 3 columns [2, 10, 10]
        let row3 = vec![
            ColSize::new(2, true),
            ColSize::new(10, true),
            ColSize::new(10, true),
        ];
        result.push_str(&format_separator_row(&row2, &row3));

        // Row 4: merged columns [13] with trailing rowspan [10]
        let row4 = vec![
            ColSize::new(13, true),
            ColSize::new(10, false),
        ];
        result.push_str(&format_separator_row(&row3, &row4));

        // Bottom border (no row after) - final row layout [15, 8]
        let row_end = vec![
            ColSize::new(15, true),
            ColSize::new(8, true),
        ];
        result.push_str(&format_separator_row(&row4, &row_end));

        eprintln!("Generated output:\n{}", result);
        eprintln!("\nExpected output:\n{}", expected_output);

        // Compare line by line for easier debugging
        let generated_lines: Vec<&str> = result.lines().collect();
        let expected_lines: Vec<&str> = expected_output.lines().collect();

        assert_eq!(generated_lines.len(), expected_lines.len(),
                   "Different number of lines");

        for (i, (gen, exp)) in generated_lines.iter().zip(expected_lines.iter()).enumerate() {
            assert_eq!(gen, exp, "Line {} differs", i + 1);
        }
    }
}
