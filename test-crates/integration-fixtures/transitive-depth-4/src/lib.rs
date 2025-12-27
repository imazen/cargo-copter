/// Re-export transitive-depth-3 functionality
pub use transitive_depth_3;

/// Wrapper - goes 4 levels deep to base-crate
pub fn level4_stable() -> String {
    transitive_depth_3::level3_stable()
}

/// Uses the old API through the entire chain
pub fn level4_uses_old_api() -> i32 {
    transitive_depth_3::level3_uses_old_api()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_level4_stable() {
        assert_eq!(level4_stable(), "stable");
    }

    #[test]
    fn test_level4_uses_old_api() {
        assert_eq!(level4_uses_old_api(), 42);
    }
}
