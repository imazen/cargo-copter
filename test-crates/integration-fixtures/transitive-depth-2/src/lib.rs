/// Re-export transitive-depth-1 functionality
pub use transitive_depth_1;

/// Wrapper - goes through depth-1 to base-crate
pub fn level2_stable() -> String {
    transitive_depth_1::level1_stable()
}

/// Uses the old API through depth-1
pub fn level2_uses_old_api() -> i32 {
    transitive_depth_1::level1_uses_old_api()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_level2_stable() {
        assert_eq!(level2_stable(), "stable");
    }

    #[test]
    fn test_level2_uses_old_api() {
        assert_eq!(level2_uses_old_api(), 42);
    }
}
