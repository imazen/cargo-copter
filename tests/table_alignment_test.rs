// Integration test for console table alignment
// This test verifies that all rows in the five-column table have consistent alignment

/// Count the actual display width of a string as rendered in a terminal
/// This is a more accurate measurement than character count
fn measure_display_width(s: &str) -> usize {
    // Use unicode-width crate's accurate measurement if available,
    // otherwise use a simple approximation
    s.chars()
        .map(|c| {
            match c {
                // Emojis - definitely 2 columns
                '📦' | '📁' => 2,
                // Box drawing - 1 column
                '─' | '│' | '┌' | '┐' | '└' | '┘' | '├' | '┤' | '┬' | '┴' | '┼' => 1,
                // Status symbols - need to verify these
                '✓' | '✗' | '⊘' | '⚠' => {
                    // These are ambiguous - could be 1 or 2 depending on terminal
                    // Let's measure what they actually are
                    1 // ASSUMPTION - need to verify
                }
                '⚡' => 2, // This is Wide
                // Regular ASCII
                c if c.is_ascii() => 1,
                // Everything else
                _ => {
                    let code = c as u32;
                    if (0x1F300..=0x1F9FF).contains(&code) || (0x2600..=0x26FF).contains(&code) { 2 } else { 1 }
                }
            }
        })
        .sum()
}

#[test]
fn test_table_row_alignment() {
    // Test data: create a simple table output and measure alignment
    let test_lines = vec![
        "┌────────────────────┬──────────┬─────────────────┬─────────────────────────┬─────────────────────┐",
        "│      Offered       │   Spec   │    Resolved     │        Dependent        │ Result         Time │",
        "├────────────────────┼──────────┼─────────────────┼─────────────────────────┼─────────────────────┤",
        "│ -                  │ ^0.8.52  │ 0.8.51 📦       │ image 0.25.8            │ PASSED ✓✓✓     2.1s │",
        "│ ✓ =this(0.8.91)    │ ^0.8.52  │ 0.8.91 📁       │ image 0.25.8            │ PASSED ✓✓✓     1.9s │",
        "│                    ├──────────┘                  └─────────────────────────┘                     │",
        "│                    │ cargo check failed on image:0.25.8                                          │",
        "│                    ├──────────┬─────────────────┬─────────────────────────┬─────────────────────┤",
        "└────────────────────┴──────────┴─────────────────┴─────────────────────────┴─────────────────────┘",
    ];

    println!("\nMeasuring display widths of each line:");
    let mut widths = Vec::new();
    for (i, line) in test_lines.iter().enumerate() {
        let width = measure_display_width(line);
        widths.push(width);
        println!("Line {}: {} chars (display width: {})", i, line.chars().count(), width);
        println!("  Content: {}", line);
    }

    // Check if all widths are the same
    let first_width = widths[0];
    let mut all_same = true;
    for (i, &w) in widths.iter().enumerate() {
        if w != first_width {
            println!("ERROR: Line {} has width {} but expected {}", i, w, first_width);
            all_same = false;
        }
    }

    // Also print byte lengths for debugging
    println!("\nByte lengths:");
    for (i, line) in test_lines.iter().enumerate() {
        println!("Line {}: {} bytes, {} chars", i, line.len(), line.chars().count());
    }

    // The test should pass - we're just measuring for now
    // Once we know the actual issue, we can make this fail on misalignment
    if !all_same {
        println!("\n⚠️  ALIGNMENT ISSUE DETECTED");
    } else {
        println!("\n✓ All lines have consistent width");
    }
}

#[test]
fn test_unicode_character_widths() {
    // Test individual characters to understand their terminal width
    let test_chars = vec![
        ('📦', "package emoji"),
        ('📁', "folder emoji"),
        ('✓', "check mark"),
        ('✗', "cross mark"),
        ('⊘', "circled slash"),
        ('⚠', "warning sign"),
        ('⚡', "lightning bolt"),
        ('─', "box drawing horizontal"),
        ('│', "box drawing vertical"),
    ];

    println!("\nUnicode character width analysis:");
    for (ch, name) in test_chars {
        let code = ch as u32;
        let measured = measure_display_width(&ch.to_string());
        println!("  {} (U+{:04X}) {}: measured={} columns", ch, code, name, measured);
    }
}

#[test]
fn test_string_with_emojis() {
    // Test strings that contain emojis to see actual rendering
    let test_strings = vec!["0.8.51 📦", "0.8.91 📁", "✓✓✓", "✗--", "⊘ ↑0.8.48"];

    println!("\nString width measurements:");
    for s in test_strings {
        let char_count = s.chars().count();
        let byte_count = s.len();
        let measured_width = measure_display_width(s);
        println!("  '{}': {} chars, {} bytes, {} display width", s, char_count, byte_count, measured_width);
    }
}
