/// Failure categorization module
///
/// Classifies test failures by root cause to help users distinguish
/// "your fault" from "not your problem."
use crate::types::{CommandType, OfferedRow};

/// Category of a failure
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FailureCategory {
    /// Dependency was yanked from crates.io
    YankedDeps,
    /// build.rs / custom build command failed
    BuildScript,
    /// Missing system library (pkg-config, cmake, libclang, etc.)
    SystemLibrary,
    /// Requires nightly Rust features
    NightlyFeature,
    /// Multiple versions of same crate in dependency graph
    VersionConflict,
    /// Platform-specific crate (embedded, esp, stm32, etc.)
    PlatformSpecific,
    /// Uncategorized failure
    Other,
}

impl FailureCategory {
    pub fn label(&self) -> &'static str {
        match self {
            FailureCategory::YankedDeps => "Yanked deps",
            FailureCategory::BuildScript => "build.rs",
            FailureCategory::SystemLibrary => "System libs",
            FailureCategory::NightlyFeature => "Nightly",
            FailureCategory::VersionConflict => "Version conflicts",
            FailureCategory::PlatformSpecific => "Platform",
            FailureCategory::Other => "Other",
        }
    }
}

/// A categorized failure with context
#[derive(Debug, Clone)]
pub struct CategorizedFailure {
    pub dependent_name: String,
    pub dependent_version: String,
    pub category: FailureCategory,
    /// Whether the error text mentions the base crate name
    pub mentions_base_crate: bool,
    /// First error line for display
    pub error_snippet: Option<String>,
}

/// Categorize a single failed row
pub fn categorize_failure(row: &OfferedRow, base_crate_name: &str) -> CategorizedFailure {
    let error_text = collect_error_text(row);
    let category = detect_category(&error_text, &row.primary.dependent_name);
    let mentions_base_crate = mentions_crate(&error_text, base_crate_name);
    let error_snippet = first_error_line_from_text(&error_text);

    CategorizedFailure {
        dependent_name: row.primary.dependent_name.clone(),
        dependent_version: row.primary.dependent_version.clone(),
        category,
        mentions_base_crate,
        error_snippet,
    }
}

/// Collect all error text from a row's failed commands
fn collect_error_text(row: &OfferedRow) -> String {
    let mut text = String::new();
    for cmd in &row.test.commands {
        if !cmd.result.passed {
            for failure in &cmd.result.failures {
                text.push_str(&failure.error_message);
                text.push('\n');
            }
        }
    }
    text
}

/// Detect the failure category from error text
fn detect_category(error_text: &str, dependent_name: &str) -> FailureCategory {
    // Check in priority order (most specific first)

    // Yanked deps
    if error_text.contains("is yanked") || error_text.contains("was yanked") {
        return FailureCategory::YankedDeps;
    }

    // Nightly features
    if error_text.contains("may not be used on the stable release channel")
        || error_text.contains("#![feature]")
        || error_text.contains("requires nightly")
        || error_text.contains("is not stable enough")
    {
        return FailureCategory::NightlyFeature;
    }

    // Version conflicts
    if error_text.contains("there are multiple different versions of crate")
        || error_text.contains("two different versions of crate")
    {
        return FailureCategory::VersionConflict;
    }

    // System libraries (check before build.rs since system lib errors often appear in build.rs)
    if error_text.contains("pkg-config")
        || error_text.contains("cannot find -l")
        || error_text.contains("libclang")
        || error_text.contains("cmake")
        || error_text.contains("Could not find ")
        || error_text.contains("library not found")
        || error_text.contains("pkg_config")
        || error_text.contains("vcpkg")
    {
        return FailureCategory::SystemLibrary;
    }

    // build.rs failures
    if error_text.contains("failed to run custom build command")
        || error_text.contains("build script")
        || error_text.contains("build.rs")
    {
        return FailureCategory::BuildScript;
    }

    // Platform-specific (heuristic: crate name patterns)
    let platform_prefixes = [
        "esp-",
        "embassy-",
        "stm32",
        "nrf-",
        "cortex-",
        "rp2040-",
        "unicorn_hat",
        "sparreal-",
        "avr-",
        "pic32-",
        "teensy-",
        "arduino-",
    ];
    let dep_lower = dependent_name.to_lowercase();
    for prefix in &platform_prefixes {
        if dep_lower.starts_with(prefix) {
            return FailureCategory::PlatformSpecific;
        }
    }
    // Also check error text for platform issues
    if error_text.contains("platform not supported")
        || error_text.contains("target is not supported")
        || error_text.contains("only available on")
    {
        return FailureCategory::PlatformSpecific;
    }

    FailureCategory::Other
}

/// Check if error text mentions the base crate name as a separate word
fn mentions_crate(error_text: &str, crate_name: &str) -> bool {
    // Check for crate name in error messages (as word boundary)
    // Simple heuristic: check for `crate_name` preceded/followed by non-alphanumeric
    let name_lower = crate_name.to_lowercase();
    let text_lower = error_text.to_lowercase();

    // Check common patterns
    text_lower.contains(&format!("`{}`", name_lower))
        || text_lower.contains(&format!("{}::", name_lower))
        || text_lower.contains(&format!("crate `{}`", name_lower))
        || text_lower.contains(&format!(" {} ", name_lower))
        || text_lower.contains(&format!("/{}/", name_lower))
}

/// Extract first error line from error text
fn first_error_line_from_text(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("error") {
            let display = if trimmed.len() > 120 { format!("{}...", &trimmed[..120]) } else { trimmed.to_string() };
            return Some(display);
        }
    }
    // Fallback: first non-empty line
    text.lines().find(|l| !l.trim().is_empty()).map(|l| {
        let trimmed = l.trim();
        if trimmed.len() > 120 { format!("{}...", &trimmed[..120]) } else { trimmed.to_string() }
    })
}

/// Summary of categorized failures grouped by category
#[derive(Debug, Clone, Default)]
pub struct FailureSummary {
    pub categories: Vec<(FailureCategory, Vec<CategorizedFailure>)>,
}

impl FailureSummary {
    /// Build a summary from a list of categorized failures
    pub fn from_failures(mut failures: Vec<CategorizedFailure>) -> Self {
        use std::collections::BTreeMap;

        // Group by category, maintaining a stable order
        let mut groups: BTreeMap<u8, (FailureCategory, Vec<CategorizedFailure>)> = BTreeMap::new();

        let order = |cat: &FailureCategory| -> u8 {
            match cat {
                FailureCategory::YankedDeps => 0,
                FailureCategory::SystemLibrary => 1,
                FailureCategory::BuildScript => 2,
                FailureCategory::NightlyFeature => 3,
                FailureCategory::PlatformSpecific => 4,
                FailureCategory::VersionConflict => 5,
                FailureCategory::Other => 6,
            }
        };

        // Deduplicate by dependent name
        failures.sort_by(|a, b| a.dependent_name.cmp(&b.dependent_name));
        failures.dedup_by(|a, b| a.dependent_name == b.dependent_name);

        for f in failures {
            let key = order(&f.category);
            groups.entry(key).or_insert_with(|| (f.category.clone(), Vec::new())).1.push(f);
        }

        FailureSummary { categories: groups.into_values().collect() }
    }

    pub fn total(&self) -> usize {
        self.categories.iter().map(|(_, fs)| fs.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_yanked() {
        assert_eq!(detect_category("version 0.1.0 is yanked", "foo"), FailureCategory::YankedDeps);
    }

    #[test]
    fn test_detect_nightly() {
        assert_eq!(
            detect_category("#![feature] may not be used on the stable release channel", "foo"),
            FailureCategory::NightlyFeature
        );
    }

    #[test]
    fn test_detect_version_conflict() {
        assert_eq!(
            detect_category("there are multiple different versions of crate `rgb`", "foo"),
            FailureCategory::VersionConflict
        );
    }

    #[test]
    fn test_detect_system_lib() {
        assert_eq!(detect_category("pkg-config exited with status code 1", "foo"), FailureCategory::SystemLibrary);
    }

    #[test]
    fn test_detect_build_script() {
        assert_eq!(
            detect_category("failed to run custom build command for `openssl-sys`", "foo"),
            FailureCategory::BuildScript
        );
    }

    #[test]
    fn test_detect_platform() {
        assert_eq!(detect_category("some error", "esp-hal-smartled"), FailureCategory::PlatformSpecific);
    }

    #[test]
    fn test_detect_other() {
        assert_eq!(detect_category("mismatched types", "image"), FailureCategory::Other);
    }

    #[test]
    fn test_mentions_crate() {
        assert!(mentions_crate("expected `rgb::Rgb<u8>`", "rgb"));
        assert!(mentions_crate("crate `rgb` has multiple versions", "rgb"));
        assert!(!mentions_crate("failed to run custom build command", "rgb"));
    }
}
