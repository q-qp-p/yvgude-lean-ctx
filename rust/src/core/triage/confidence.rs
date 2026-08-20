pub fn clamp_milli(value: u16) -> u16 {
    value.min(1000)
}
pub fn is_high_confidence(milli: u16) -> bool {
    milli >= 850
}
pub fn is_low_confidence(milli: u16) -> bool {
    milli < 500
}

/// Below this a profile is not actionable and output passes through unfiltered.
pub const ACTIONABLE_FLOOR_MILLI: u16 = 300;

/// Confidence reported by the rules fallback — deliberately below the floor:
/// a rules-only profile without a task text is a guess, and says so.
pub const RULES_FALLBACK_MILLI: u16 = 200;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clamp_preserves_valid() {
        assert_eq!(clamp_milli(700), 700);
    }
    #[test]
    fn clamp_caps_maximum() {
        assert_eq!(clamp_milli(u16::MAX), 1000);
    }
    #[test]
    fn high_is_inclusive() {
        assert!(is_high_confidence(850));
    }
    #[test]
    fn low_is_exclusive() {
        assert!(!is_low_confidence(500));
    }
    #[test]
    fn rules_fallback_stays_below_the_actionable_floor() {
        assert!(RULES_FALLBACK_MILLI < ACTIONABLE_FLOOR_MILLI);
    }
}
