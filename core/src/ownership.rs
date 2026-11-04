//! Capture ownership policy. HWND properties live with the window, not its PID.
pub const AUTO_PROPERTY: &str = "PawHide.AutoAffinity.v1";
pub const MANUAL_PROPERTY: &str = "PawHide.ManualAffinity.v1";

#[derive(Debug, PartialEq, Eq)]
pub enum RuleMatch {
    Matches,
    Outside,
    Uncertain,
}

pub fn should_restore(owned: bool, application: bool, folder: RuleMatch) -> bool {
    owned && !application && folder == RuleMatch::Outside
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_out_restores_only_automatic_exclusions() {
        assert!(!should_restore(true, false, RuleMatch::Matches));
        assert!(should_restore(true, false, RuleMatch::Outside));
        assert!(!should_restore(false, false, RuleMatch::Outside));
    }

    #[test]
    fn overlapping_rules_and_uncertain_tabs_keep_exclusion() {
        assert!(!should_restore(true, true, RuleMatch::Outside));
        assert!(!should_restore(true, false, RuleMatch::Uncertain));
    }
}
