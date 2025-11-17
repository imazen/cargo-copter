/// This crate compiles with both v1 and v2, but tests fail with v2
/// Expected result: REGRESSED (check passes, but test fails with v2)
///
/// The library code only uses stable_api (works in both versions)
/// But the test code uses old_api (only exists in v1)

pub fn use_stable() -> String {
    base_crate::stable_api()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_library_function() {
        // This test passes with both versions
        assert_eq!(use_stable(), "stable");
    }

    #[test]
    fn test_direct_old_api_access() {
        // This test calls old_api() directly
        // Works with v1, fails to compile with v2 (function removed)
        assert_eq!(base_crate::old_api(), 42);
    }
}
