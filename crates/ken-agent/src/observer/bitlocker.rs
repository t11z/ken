//! `BitLocker` observer via WMI `Win32_EncryptableVolume`.
//!
//! On Windows, queries the `ROOT\CIMV2\Security\MicrosoftVolumeEncryption`
//! namespace. Requires SYSTEM privileges (the agent has them).

use ken_protocol::status::{BitLockerStatus, Observation};

use super::lifecycle::ObserverLifecycle;
use super::tick::TickBoundary;
use super::trait_def::{Observer, ObserverKind};

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
    const KIND: ObserverKind = ObserverKind::Synchronous;

    fn name(&self) -> &'static str {
        "bitlocker"
    }

    fn observe(&mut self, _tick: &TickBoundary) -> BitLockerStatus {
        #[cfg(windows)]
        {
            tracing::debug!("bitlocker observer: WMI query not yet implemented");
        }

        BitLockerStatus {
            volumes: Observation::Unobserved,
        }
    }

    fn start(&mut self, _lifecycle: ObserverLifecycle) {
        // Synchronous observer — no background work to start.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observe_returns_all_unobserved() {
        let mut obs = BitLockerObserver::new();
        let tick = TickBoundary::now();
        let status = obs.observe(&tick);
        assert_eq!(status.volumes, Observation::Unobserved);
    }
}
