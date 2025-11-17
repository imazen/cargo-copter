// Unit tests for report.rs alignment

#[cfg(test)]
mod tests {
    #[test]
    fn test_error_row_width_calculation() {
        // Column widths from the actual code
        let w_offered = 20;
        let w_spec = 10;
        let w_resolved = 17;
        let w_dependent = 25;
        let w_result = 21;

        // Expected total width of any row (from border line)
        // ┌{20}┬{10}┬{17}┬{25}┬{21}┐
        let expected_total = 1 + w_offered + 1 + w_spec + 1 + w_resolved + 1 + w_dependent + 1 + w_result + 1;
        assert_eq!(expected_total, 99, "Border line should be 99 chars");

        // Current buggy calculation from line 776
        let detail_width_buggy = w_spec + w_resolved + w_dependent + w_result + 6;
        println!("Buggy detail_width: {}", detail_width_buggy);
        // = 10 + 17 + 25 + 21 + 6 = 79

        // Current corner line calculation from lines 783-790
        let corner1_width = w_spec;  // 10
        let corner2_width = w_dependent;  // 25
        let padding_width = w_resolved + 2;  // 17 + 2 = 19

        // Line format: │{w_offered}├{corner1}┘{padding}└{corner2}┘{w_result}│
        let corner_line_width_buggy =
            1 + // left │
            w_offered + // 20 spaces
            1 + // ├
            corner1_width + 1 + // dashes + ┘ (10 + 1 = 11)
            padding_width + // spaces (19)
            1 + // └
            corner2_width + 1 + // dashes + ┘ (25 + 1 = 26)
            w_result + // 21 spaces
            1; // right │

        println!("Buggy corner line width: {}", corner_line_width_buggy);
        // = 1 + 20 + 1 + 11 + 19 + 1 + 26 + 21 + 1 = 101 (WRONG! Should be 99)

        assert_ne!(corner_line_width_buggy, 99, "Current calculation produces 101 chars instead of 99");

        // Correct calculation
        // The middle section (between col1 and col5) should be:
        // 99 - 1 (│) - 20 (col1) - 21 (col5) - 1 (│) = 56 chars
        let middle_section_width = 99 - 1 - w_offered - w_result - 1;
        println!("Middle section should be: {} chars", middle_section_width);
        assert_eq!(middle_section_width, 56);

        // The middle section contains:
        // ├{col2_dashes}┘{spaces}└{col4_dashes}┘
        // We need: 1 + col2_dashes + 1 + spaces + 1 + col4_dashes + 1 = 56
        // So: col2_dashes + spaces + col4_dashes = 56 - 4 = 52

        // Natural choice: col2_dashes = w_spec, col4_dashes = w_dependent
        // So: spaces = 52 - w_spec - w_dependent = 52 - 10 - 25 = 17
        let correct_padding = 52 - w_spec - w_dependent;
        println!("Correct padding width: {}", correct_padding);
        assert_eq!(correct_padding, 17);

        // Verify the corrected line
        let corner_line_width_correct =
            1 + // │
            w_offered + // 20
            1 + // ├
            w_spec + 1 + // 10 + 1 = 11
            correct_padding + // 17
            1 + // └
            w_dependent + 1 + // 25 + 1 = 26
            w_result + // 21
            1; // │
        println!("Correct corner line width: {}", corner_line_width_correct);
        assert_eq!(corner_line_width_correct, 99, "Corrected calculation should produce 99 chars");
    }

    #[test]
    fn test_error_text_row_width() {
        let w_offered = 20;
        let w_spec = 10;
        let w_resolved = 17;
        let w_dependent = 25;
        let w_result = 21;

        // Error text row format: │{w_offered}│ {error_text} │
        // Total: 1 + 20 + 1 + 1 + error_text + 1 + 1 = 99
        // So: error_text = 99 - 25 = 74

        let error_text_width = 99 - 1 - w_offered - 1 - 1 - 1 - 1;
        println!("Error text width should be: {}", error_text_width);
        assert_eq!(error_text_width, 74);

        // Current buggy calculation claims detail_width = 79, minus 2 for padding = 77
        // That's 3 chars too many!
    }
}
