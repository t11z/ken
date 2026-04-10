/// Current schema version for the Ken agent-server protocol.
///
/// This version is included in every heartbeat and enrollment exchange.
/// Both sides check compatibility before processing messages. Bump this
/// constant only when a wire-incompatible change is made, accompanied
/// by a dedicated ADR documenting the migration path.
///
/// Version 2: ADR-0019 — all observer-contributed fields wrapped in
/// `Observation<T>`, subsystem-level `Option` wrappers removed.
pub const SCHEMA_VERSION: u32 = 2;

/// Check whether a remote peer's schema version is compatible with ours.
///
/// Currently requires exact match. Future versions may relax this to
/// support a compatibility range, but the check is explicit so that
/// any relaxation is a deliberate decision.
#[must_use]
pub fn is_compatible(remote: u32) -> bool {
    remote == SCHEMA_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_version_is_two() {
        assert_eq!(SCHEMA_VERSION, 2);
    }

    #[test]
    fn compatible_with_same_version() {
        assert!(is_compatible(SCHEMA_VERSION));
    }

    #[test]
    fn incompatible_with_different_version() {
        assert!(!is_compatible(0));
        assert!(!is_compatible(1));
        assert!(!is_compatible(3));
    }
}
