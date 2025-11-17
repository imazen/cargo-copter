/// This crate compiles and tests pass with both v1 and v2
/// Expected result: PASSED (all checks and tests pass)

pub fn use_stable() -> String {
    base_crate::stable_api()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_use_stable() {
        // This test passes with both v1 and v2
        assert_eq!(use_stable(), "stable");
    }

    #[test]
    fn test_another_passing_test() {
        // Additional test to verify test suite runs
        assert!(true);
    }
}
