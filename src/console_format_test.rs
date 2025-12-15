/// Tests for console formatting module
///
/// These tests ensure console output formatting remains stable
/// and matches our reference fixtures.

#[cfg(test)]
mod tests {
    use crate::console_format::*;

    /// Standard width for tests to ensure reproducible output
    const TEST_CONSOLE_WIDTH: usize = 120;

    /// Set up test environment with fixed console width
    fn setup_test_width() {
        set_console_width(TEST_CONSOLE_WIDTH);
    }

    #[test]
    fn test_display_width_ascii() {
        assert_eq!(display_width("hello"), 5);
        assert_eq!(display_width(""), 0);
        assert_eq!(display_width("test123"), 7);
    }

    #[test]
    fn test_display_width_unicode() {
        // Unicode box drawing characters
        assert_eq!(display_width("│"), 1);
        assert_eq!(display_width("─"), 1);
        // Emoji (wide characters)
        assert_eq!(display_width("📦"), 2);
        assert_eq!(display_width("✓✓✓"), 3);
    }

    #[test]
    fn test_truncate_with_padding_exact_fit() {
        let result = truncate_with_padding("hello", 5);
        assert_eq!(result, "hello");
        assert_eq!(display_width(&result), 5);
    }

    #[test]
    fn test_truncate_with_padding_needs_padding() {
        let result = truncate_with_padding("hi", 5);
        assert_eq!(result, "hi   ");
        assert_eq!(display_width(&result), 5);
    }

    #[test]
    fn test_truncate_with_padding_needs_truncation() {
        let result = truncate_with_padding("hello world", 8);
        assert_eq!(result, "hello...");
        assert_eq!(display_width(&result), 8);
    }

    #[test]
    fn test_truncate_with_padding_unicode() {
        let result = truncate_with_padding("test 📦 box", 10);
        // Should truncate and add "..."
        assert_eq!(display_width(&result), 10);
    }

    #[test]
    fn test_table_widths_calculation() {
        setup_test_width();
        let widths = TableWidths::new(TEST_CONSOLE_WIDTH);

        // Verify total adds up correctly (120 - 6 borders = 114 for content)
        let total_content = widths.offered + widths.spec + widths.resolved + widths.dependent + widths.result;
        assert!(total_content <= 114);

        // Verify reasonable minimums
        assert!(widths.offered >= 10);
        assert!(widths.spec >= 10);
        assert!(widths.resolved >= 10);
        assert!(widths.dependent >= 10);
        assert!(widths.result >= 20);
    }

    #[test]
    fn test_calculate_offered_width() {
        let versions = vec![
            "0.8.50".to_string(),
            "0.8.51".to_string(),
            "0.8.52-alpha.1".to_string(),
        ];

        let width = TableWidths::calculate_offered_width(&versions, "0.8.52", false);

        // Should be at least as wide as the longest version + formatting
        // Format: "{icon} {resolution}{version}" = 1 + 1 + 1 + len + padding
        assert!(width >= 3 + "0.8.52-alpha.1".len());
    }

    #[test]
    fn test_calculate_offered_width_with_forced() {
        let versions = vec!["0.8.50".to_string()];

        let width = TableWidths::calculate_offered_width(&versions, "0.8.52", true);

        // Should account for forced marker "→!" = 2 chars + cell padding = 2
        // Format: "{icon} {resolution}{version}→!"
        // Icon (1) + space (1) + resolution (1) + version (6) + forced (2) + padding (2) = 13
        let expected = 1 + 1 + 1 + "0.8.50".len() + 2 + 2;
        assert_eq!(width, expected);
    }

    #[test]
    fn test_comparison_stats_serialization() {
        let stats = ComparisonStats {
            version_label: "Default".to_string(),
            total_tested: 5,
            already_broken: Some(1),
            passed_fetch: 5,
            passed_check: 4,
            passed_test: 3,
            fully_passing: 3,
            regressions: vec!["crate1".to_string()],
        };

        // Test JSON serialization works
        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains("Default"));
        assert!(json.contains("\"total_tested\":5"));

        // Test deserialization
        let deserialized: ComparisonStats = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.version_label, "Default");
        assert_eq!(deserialized.total_tested, 5);
    }

    #[test]
    fn test_table_header_format_contains_all_columns() {
        setup_test_width();
        init_table_widths(&[], "0.8.52", false);

        let header = format_table_header("test-crate", "0.8.52", 5, None, None);

        // Should contain all column headers
        assert!(header.contains("Offered"));
        assert!(header.contains("Spec"));
        assert!(header.contains("Resolved"));
        assert!(header.contains("Dependent"));
        assert!(header.contains("Result"));
        assert!(header.contains("Time"));

        // Should contain test info
        assert!(header.contains("test-crate"));
        assert!(header.contains("0.8.52"));
        assert!(header.contains("5 reverse dependencies"));
    }

    #[test]
    fn test_table_header_with_test_plan() {
        setup_test_width();
        init_table_widths(&[], "0.8.52", false);

        let test_plan = "  Dependents: foo, bar\n  versions: baseline, 0.8.51\n  2 × 2 = 4 tests";
        let header = format_table_header("test-crate", "0.8.52", 2, Some(test_plan), None);

        // Should include test plan
        assert!(header.contains("Dependents: foo, bar"));
        assert!(header.contains("versions: baseline, 0.8.51"));
        assert!(header.contains("2 × 2 = 4 tests"));
    }

    #[test]
    fn test_table_header_with_this_path() {
        setup_test_width();
        init_table_widths(&[], "0.8.52", false);

        let header = format_table_header("test-crate", "0.8.52", 1, None, Some("/path/to/crate"));

        // Should show path instead of "your work-in-progress version"
        assert!(header.contains("/path/to/crate"));
        assert!(!header.contains("work-in-progress"));
    }

    #[test]
    fn test_table_footer_matches_header_width() {
        setup_test_width();
        init_table_widths(&[], "0.8.52", false);

        let header = format_table_header("test", "0.8.52", 1, None, None);
        let footer = format_table_footer();

        // Get the width of the first line (top border)
        let header_width = header.lines().nth(4).map(|l| l.len()).unwrap_or(0);
        let footer_width = footer.trim_end().len();

        // Should be same width as header border
        assert_eq!(header_width, footer_width);
    }
}
