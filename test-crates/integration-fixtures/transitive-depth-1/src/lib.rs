/// Re-export base-crate functionality
pub use base_crate;

/// Wrapper around base_crate::stable_api
pub fn level1_stable() -> String {
    base_crate::stable_api()
}

/// Uses old_api from base-crate v1 - will break with v2+
pub fn level1_uses_old_api() -> i32 {
    base_crate::old_api()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_level1_stable() {
        assert_eq!(level1_stable(), "stable");
    }

    #[test]
    fn test_level1_uses_old_api() {
        assert_eq!(level1_uses_old_api(), 42);
    }
}
