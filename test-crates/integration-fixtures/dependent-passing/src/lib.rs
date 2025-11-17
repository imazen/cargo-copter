/// This crate only uses stable_api() which exists in both v1 and v2
/// Expected result: PASSED (compiles and tests pass with both versions)

pub fn use_base_crate() -> String {
    base_crate::stable_api()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_use_base_crate() {
        assert_eq!(use_base_crate(), "stable");
    }
}
