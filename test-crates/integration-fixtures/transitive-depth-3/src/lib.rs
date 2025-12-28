/// Re-export transitive-depth-2 functionality
pub use transitive_depth_2;

/// Wrapper - goes through depth-2 -> depth-1 -> base-crate
pub fn level3_stable() -> String {
    transitive_depth_2::level2_stable()
}

/// Uses the old API through the chain
pub fn level3_uses_old_api() -> i32 {
    transitive_depth_2::level2_uses_old_api()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_level3_stable() {
        assert_eq!(level3_stable(), "stable");
    }

    #[test]
    fn test_level3_uses_old_api() {
        assert_eq!(level3_uses_old_api(), 42);
    }
}
