/// Stable API that will remain in both v1 and v2
pub fn stable_api() -> String {
    "stable".to_string()
}

/// Old API that will be removed in v2 (breaking change)
pub fn old_api() -> i32 {
    42
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stable_api() {
        assert_eq!(stable_api(), "stable");
    }

    #[test]
    fn test_old_api() {
        assert_eq!(old_api(), 42);
    }
}
