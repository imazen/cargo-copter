/// Stable API that exists in all versions
pub fn stable_api() -> String {
    "stable".to_string()
}

/// New v3 API - not in v1 or v2
pub fn new_v3_api() -> u64 {
    300
}

/// Re-export a type that changed signature in v3
/// In v1/v2: Color was (u8, u8, u8)
/// In v3: Color is a struct with named fields
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub fn as_tuple(&self) -> (u8, u8, u8) {
        (self.r, self.g, self.b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stable_api() {
        assert_eq!(stable_api(), "stable");
    }

    #[test]
    fn test_new_v3_api() {
        assert_eq!(new_v3_api(), 300);
    }

    #[test]
    fn test_color() {
        let c = Color::new(255, 128, 0);
        assert_eq!(c.as_tuple(), (255, 128, 0));
    }
}
