/// This crate has compilation errors independent of base-crate version
/// Expected result: BROKEN (fails to compile even with v1)

pub fn broken_function() -> String {
    // Type mismatch error: trying to return an integer as a String
    42
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_broken() {
        // This test would never run because compilation fails
        assert_eq!(broken_function(), "42");
    }
}
