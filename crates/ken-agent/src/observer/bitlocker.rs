//! `BitLocker` observer via WMI `Win32_EncryptableVolume`.
//!
//! On Windows, queries the `ROOT\CIMV2\Security\MicrosoftVolumeEncryption`
//! namespace. Requires SYSTEM privileges (the agent has them).

use ken_protocol::status::{BitLockerStatus, Observation};

use super::trait_def::Observer;

/// `BitLocker` observer struct per ADR-0018.
pub struct BitLockerObserver;

impl BitLockerObserver {
    /// Create a new `BitLocker` observer.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Observer for BitLockerObserver {
    type Output = BitLockerStatus;

    fn name(&self) -> &'static str {
        "bitlocker"
    }

    fn observe(&mut self) -> BitLockerStatus {
        #[cfg(windows)]
        {
            tracing::debug!("bitlocker observer: WMI query not yet implemented");
        }

        BitLockerStatus {
            volumes: Observation::Unobserved,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observe_returns_all_unobserved() {
        let mut obs = BitLockerObserver::new();
        let status = obs.observe();
        assert_eq!(status.volumes, Observation::Unobserved);
    }
}
