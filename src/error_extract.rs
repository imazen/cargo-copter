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
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub code: Option<String>,
    pub message: String,
    pub rendered: String,
    pub primary_span: Option<SpanInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone)]
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
                if let Some(compiler_msg) = msg.message {
                    if let Some(diag) = convert_compiler_message(&compiler_msg) {
                        diagnostics.push(diag);
                    }
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
    let primary_span = msg.spans.iter()
        .find(|s| s.is_primary)
        .map(|s| SpanInfo {
            file_name: s.file_name.clone(),
            line: s.line_start,
            column: s.column_start,
            label: s.label.clone(),
        });

    // Use rendered output if available, otherwise construct from message
    let rendered = msg.rendered.clone()
        .unwrap_or_else(|| format_diagnostic_text(msg));

    Some(Diagnostic {
        level,
        code,
        message: msg.message.clone(),
        rendered,
        primary_span,
    })
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
        output.push_str(&format!(
            " --> {}:{}:{}\n",
            span.file_name, span.line_start, span.column_start
        ));
    }

    output
}

/// Extract just error messages for quick display
/// Uses the rendered field which contains the full formatted error with code snippets
pub fn extract_error_summary(diagnostics: &[Diagnostic]) -> String {
    diagnostics.iter()
        .filter(|d| d.level.is_error())
        .map(|d| d.rendered.clone())
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
    fn test_error_summary() {
        let diagnostics = vec![
            Diagnostic {
                level: DiagnosticLevel::Error,
                code: Some("E0425".to_string()),
                message: "cannot find value `foo`".to_string(),
                rendered: "full error text".to_string(),
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
                rendered: "warning text".to_string(),
                primary_span: None,
            },
        ];

        let summary = extract_error_summary(&diagnostics);
        assert!(summary.contains("error[E0425]"));
        assert!(summary.contains("cannot find value"));
        assert!(summary.contains("src/main.rs:10:5"));
        assert!(!summary.contains("unused variable")); // Warnings excluded
    }
}
