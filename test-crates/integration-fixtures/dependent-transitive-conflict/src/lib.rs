/// This crate demonstrates a transitive version conflict scenario.
///
/// It depends on:
/// 1. base-crate directly (flexible spec >=0.1)
/// 2. transitive-depth-4 which eventually requires base-crate =0.1
///
/// When trying to force base-crate to v2 or v3:
/// - Direct dependency can be satisfied (>=0.1 includes v2, v3)
/// - Transitive chain cannot (transitive-depth-1 requires =0.1)
///
/// This creates the "multiple versions of crate X" error that requires
/// recursive patching to resolve.

/// Uses base-crate directly
pub fn use_base_directly() -> String {
    base_crate::stable_api()
}

/// Uses base-crate through the 4-level transitive chain
pub fn use_through_chain() -> String {
    transitive_depth_4::level4_stable()
}

/// This will fail if there are multiple incompatible versions of base-crate
/// in the dependency tree (types won't match across crate boundaries)
pub fn mix_both_paths() -> bool {
    let direct = use_base_directly();
    let chain = use_through_chain();
    direct == chain
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_use_base_directly() {
        assert_eq!(use_base_directly(), "stable");
    }

    #[test]
    fn test_use_through_chain() {
        assert_eq!(use_through_chain(), "stable");
    }

    #[test]
    fn test_mix_both_paths() {
        // This works when all paths use the same base-crate version
        assert!(mix_both_paths());
    }
}
