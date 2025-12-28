/// Error extraction module for parsing cargo JSON output
///
/// This module parses cargo's --message-format=json output to extract
/// structured error information for better reporting.
use serde::{Deserialize, Serialize};
// BufRead not needed for current implementation

/// A diagnostic message from the compiler
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CargoMessage {
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<CompilerMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompilerMessage {
    pub message: String,
    #[serde(default)]
    pub level: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<ErrorCode>,
    #[serde(default)]
    pub spans: Vec<Span>,
    #[serde(default)]
    pub children: Vec<CompilerMessage>,
    pub rendered: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorCode {
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Span {
    pub file_name: String,
    pub line_start: usize,
    pub line_end: usize,
    pub column_start: usize,
    pub column_end: usize,
    pub is_primary: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default)]
    pub text: Vec<SpanText>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanText {
    pub text: String,
}

/// A parsed diagnostic with extracted key information
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub code: Option<String>,
    pub message: String,
    pub rendered: String,
    pub primary_span: Option<SpanInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DiagnosticLevel {
    Error,
    Warning,
    Help,
    Note,
    Other(String),
}

impl DiagnosticLevel {
    pub fn from_str(s: &str) -> Self {
        match s {
            "error" => DiagnosticLevel::Error,
            "warning" => DiagnosticLevel::Warning,
            "help" => DiagnosticLevel::Help,
            "note" => DiagnosticLevel::Note,
            other => DiagnosticLevel::Other(other.to_string()),
        }
    }

    pub fn is_error(&self) -> bool {
        matches!(self, DiagnosticLevel::Error)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SpanInfo {
    pub file_name: String,
    pub line: usize,
    pub column: usize,
    pub label: Option<String>,
}

/// Parse cargo JSON output and extract diagnostics
pub fn parse_cargo_json(output: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for line in output.lines() {
        if line.trim().is_empty() {
            continue;
        }

        match serde_json::from_str::<CargoMessage>(line) {
            Ok(msg) if msg.reason == "compiler-message" => {
                if let Some(compiler_msg) = msg.message
                    && let Some(diag) = convert_compiler_message(&compiler_msg)
                {
                    diagnostics.push(diag);
                }
            }
            _ => continue, // Skip non-compiler messages or parse errors
        }
    }

    diagnostics
}

fn convert_compiler_message(msg: &CompilerMessage) -> Option<Diagnostic> {
    let level = DiagnosticLevel::from_str(&msg.level);

    // Only capture errors and warnings, not help/note (those are children)
    if !matches!(level, DiagnosticLevel::Error | DiagnosticLevel::Warning) {
        return None;
    }

    let code = msg.code.as_ref().map(|c| c.code.clone());

    // Find primary span
    let primary_span = msg.spans.iter().find(|s| s.is_primary).map(|s| SpanInfo {
        file_name: s.file_name.clone(),
        line: s.line_start,
        column: s.column_start,
        label: s.label.clone(),
    });

    // Use rendered output if available, otherwise construct from message
    let rendered = msg.rendered.clone().unwrap_or_else(|| format_diagnostic_text(msg));

    Some(Diagnostic { level, code, message: msg.message.clone(), rendered, primary_span })
}

fn format_diagnostic_text(msg: &CompilerMessage) -> String {
    let mut output = String::new();

    // Error header
    if let Some(code) = &msg.code {
        output.push_str(&format!("{}[{}]: {}\n", msg.level, code.code, msg.message));
    } else {
        output.push_str(&format!("{}: {}\n", msg.level, msg.message));
    }

    // Primary span location
    if let Some(span) = msg.spans.iter().find(|s| s.is_primary) {
        output.push_str(&format!(" --> {}:{}:{}\n", span.file_name, span.line_start, span.column_start));
    }

    output
}

/// Information about a "multiple versions of crate X" conflict
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MultipleVersionsConflict {
    /// The crate that has multiple versions (e.g., "rgb")
    pub crate_name: String,
    /// The crates that are pulling in conflicting versions
    pub conflicting_deps: Vec<ConflictingDep>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConflictingDep {
    /// The crate that has the conflicting dependency
    pub dep_name: String,
    /// How this dep uses the crate (e.g., "as a dependency of crate `ravif`")
    pub usage: String,
}

/// Detect "multiple different versions of crate X" errors from rendered output
/// Returns a list of conflicts found
pub fn detect_multiple_version_conflicts(output: &str) -> Vec<MultipleVersionsConflict> {
    let mut conflicts = Vec::new();

    // Pattern: "note: there are multiple different versions of crate `X` in the dependency graph"
    let re_main =
        regex::Regex::new(r"there are multiple different versions of crate `([^`]+)` in the dependency graph").ok();
    // Pattern: "one version of crate `X` used here, as a dependency of crate `Y`"
    let re_dep = regex::Regex::new(
        r"one version of crate `([^`]+)` used here, as a (dependency of crate `([^`]+)`|direct dependency)",
    )
    .ok();

    if re_main.is_none() {
        // Fallback: simple string search if regex fails
        if output.contains("there are multiple different versions of crate") {
            // Extract crate name with simple pattern matching
            for line in output.lines() {
                if line.contains("there are multiple different versions of crate `")
                    && let Some(start) = line.find("crate `")
                {
                    let rest = &line[start + 7..];
                    if let Some(end) = rest.find('`') {
                        let crate_name = rest[..end].to_string();
                        conflicts.push(MultipleVersionsConflict { crate_name, conflicting_deps: Vec::new() });
                    }
                }
            }
        }
        return conflicts;
    }

    let re_main = re_main.unwrap();
    let re_dep = re_dep.unwrap();

    // Split output into sections per error
    for section in output.split("error[") {
        if let Some(caps) = re_main.captures(section) {
            let crate_name = caps.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();

            let mut conflicting_deps = Vec::new();
            for dep_caps in re_dep.captures_iter(section) {
                let usage = if dep_caps.get(3).is_some() {
                    format!("dependency of {}", dep_caps.get(3).unwrap().as_str())
                } else {
                    "direct dependency".to_string()
                };
                conflicting_deps.push(ConflictingDep {
                    dep_name: dep_caps.get(1).map(|m| m.as_str().to_string()).unwrap_or_default(),
                    usage,
                });
            }

            conflicts.push(MultipleVersionsConflict { crate_name, conflicting_deps });
        }
    }

    // Deduplicate by crate name
    conflicts.sort_by(|a, b| a.crate_name.cmp(&b.crate_name));
    conflicts.dedup_by(|a, b| a.crate_name == b.crate_name);

    conflicts
}

/// Check if an error output contains "multiple versions" conflict
pub fn has_multiple_version_conflict(output: &str) -> bool {
    output.contains("there are multiple different versions of crate")
}

/// Extract the list of crates that need patching to resolve a multi-version conflict
pub fn extract_crates_needing_patch(output: &str, base_crate: &str) -> Vec<String> {
    let conflicts = detect_multiple_version_conflicts(output);
    let mut crates_to_patch = Vec::new();

    for conflict in conflicts {
        if conflict.crate_name == base_crate {
            // Add all deps that are using the wrong version
            for dep in conflict.conflicting_deps {
                if !dep.dep_name.is_empty() && dep.usage.contains("dependency of") {
                    // Extract the parent crate name
                    if let Some(start) = dep.usage.find("dependency of ") {
                        let parent = dep.usage[start + 14..].trim();
                        if !parent.is_empty() && !crates_to_patch.contains(&parent.to_string()) {
                            crates_to_patch.push(parent.to_string());
                        }
                    }
                }
            }
        }
    }

    crates_to_patch
}

/// Extract just error messages for quick display
/// Uses the rendered field which contains the full formatted error with code snippets
///
/// # Arguments
/// * `diagnostics` - The diagnostics to extract errors from
/// * `max_lines` - Maximum number of lines to include per error (0 = unlimited)
pub fn extract_error_summary(diagnostics: &[Diagnostic], max_lines: usize) -> String {
    diagnostics
        .iter()
        .filter(|d| d.level.is_error())
        .map(|d| {
            if max_lines == 0 {
                d.rendered.clone()
            } else {
                // Limit to max_lines, appending "..." if truncated
                let lines: Vec<&str> = d.rendered.lines().collect();
                if lines.len() > max_lines {
                    let mut truncated = lines[..max_lines].join("\n");
                    truncated.push_str(&format!("\n... ({} more lines)", lines.len() - max_lines));
                    truncated
                } else {
                    d.rendered.clone()
                }
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty_output() {
        let diagnostics = parse_cargo_json("");
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_parse_error_message() {
        let json = r#"{"reason":"compiler-message","message":{"message":"mismatched types","code":{"code":"E0308","explanation":"..."},"level":"error","spans":[{"file_name":"src/lib.rs","line_start":6,"line_end":6,"column_start":5,"column_end":7,"is_primary":true,"label":"expected `String`, found integer","text":[{"text":"    42"}]}],"rendered":"error[E0308]: mismatched types\n --> src/lib.rs:6:5\n"}}"#;

        let diagnostics = parse_cargo_json(json);
        assert_eq!(diagnostics.len(), 1);

        let diag = &diagnostics[0];
        assert!(diag.level.is_error());
        assert_eq!(diag.code.as_ref().unwrap(), "E0308");
        assert_eq!(diag.message, "mismatched types");
        assert!(diag.primary_span.is_some());
    }

    #[test]
    fn test_parse_multiple_messages() {
        let json = r#"{"reason":"compiler-artifact"}
{"reason":"compiler-message","message":{"message":"unused variable","level":"warning","spans":[],"rendered":"warning: unused variable"}}
{"reason":"compiler-message","message":{"message":"cannot find value","level":"error","spans":[],"rendered":"error: cannot find value"}}"#;

        let diagnostics = parse_cargo_json(json);
        assert_eq!(diagnostics.len(), 2); // 1 warning + 1 error

        let errors: Vec<_> = diagnostics.iter().filter(|d| d.level.is_error()).collect();
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn test_detect_multiple_version_conflict() {
        let output = r#"
error[E0277]: the trait bound `[u8]: AsPixels<rgb::formats::rgb::Rgb<u8>>` is not satisfied
note: there are multiple different versions of crate `rgb` in the dependency graph
   --> rgb-0.8.91-alpha.3/src/legacy/internal/convert/mod.rs:10:1
    | pub trait AsPixels<PixelType> {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ this is the required trait
    |
   ::: src/codecs/avif/encoder.rs:21:5
    | use ravif::{BitDepth, Encoder, Img, RGB8, RGBA8};
    |     ----- one version of crate `rgb` used here, as a dependency of crate `ravif`
    | use rgb::AsPixels;
    |     --- one version of crate `rgb` used here, as a direct dependency of the current crate
"#;
        let conflicts = detect_multiple_version_conflicts(output);
        assert!(!conflicts.is_empty(), "Should detect conflict");
        assert_eq!(conflicts[0].crate_name, "rgb");
    }

    #[test]
    fn test_has_multiple_version_conflict() {
        let output = "note: there are multiple different versions of crate `rgb` in the dependency graph";
        assert!(has_multiple_version_conflict(output));
        assert!(!has_multiple_version_conflict("some other error"));
    }

    #[test]
    fn test_error_summary() {
        let diagnostics = vec![
            Diagnostic {
                level: DiagnosticLevel::Error,
                code: Some("E0425".to_string()),
                message: "cannot find value `foo`".to_string(),
                rendered: "error[E0425]: cannot find value `foo` in this scope\n --> src/main.rs:10:5".to_string(),
                primary_span: Some(SpanInfo {
                    file_name: "src/main.rs".to_string(),
                    line: 10,
                    column: 5,
                    label: Some("not found in this scope".to_string()),
                }),
            },
            Diagnostic {
                level: DiagnosticLevel::Warning,
                code: None,
                message: "unused variable".to_string(),
                rendered: "warning: unused variable".to_string(),
                primary_span: None,
            },
        ];

        let summary = extract_error_summary(&diagnostics, 0);
        // extract_error_summary only returns the rendered field from errors
        assert!(summary.contains("error[E0425]"));
        assert!(summary.contains("cannot find value"));
        assert!(summary.contains("src/main.rs:10:5"));
        assert!(!summary.contains("unused variable")); // Warnings excluded
    }
}
