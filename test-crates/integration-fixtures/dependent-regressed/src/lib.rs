/// This crate uses old_api() which is removed in v2
/// Expected result: REGRESSED (compiles with v1, fails with v2)

pub fn use_old_api() -> i32 {
    base_crate::old_api()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_use_old_api() {
        assert_eq!(use_old_api(), 42);
    }
}
