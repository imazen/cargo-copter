/// Stable API that exists in both v1 and v2
pub fn stable_api() -> String {
    "stable".to_string()
}

/// New API added in v2
pub fn new_api() -> bool {
    true
}

// Note: old_api() has been removed in v2 - this is a breaking change

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stable_api() {
        assert_eq!(stable_api(), "stable");
    }

    #[test]
    fn test_new_api() {
        assert_eq!(new_api(), true);
    }
}
