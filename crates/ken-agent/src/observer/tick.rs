//! Tick-boundary type for observer snapshot collection per ADR-0021.
//!
//! The worker loop computes a single [`TickBoundary`] at the start of
//! each `collect_snapshot` call and passes it to every observer's read
//! path. This ensures that all observers in the same tick agree on
//! whether a cached value is `Fresh` or `Cached`, without consulting
//! a wall clock independently.

use ken_protocol::status::Observation;
use time::OffsetDateTime;

/// The start of the current heartbeat tick.
///
/// Per ADR-0021 and ADR-0019, the tick boundary is the reference point
/// against which observers decide between `Fresh` and `Cached`. A value
/// whose `observed_at` is at or after the tick boundary is `Fresh`; a
/// value observed before the tick boundary is `Cached`.
///
/// Two observers reading on the same tick receive the same
/// `TickBoundary`, which is the only mechanism that makes the
/// `Fresh`/`Cached` distinction consistent across observers within a
/// single snapshot.
#[derive(Debug, Clone, Copy)]
pub struct TickBoundary(OffsetDateTime);

impl TickBoundary {
    /// Create a tick boundary at the current UTC time.
    pub fn now() -> Self {
        Self(OffsetDateTime::now_utc())
    }

    /// Create a tick boundary at a specific time (for testing).
    #[cfg(test)]
    pub fn at(time: OffsetDateTime) -> Self {
        Self(time)
    }

    /// The timestamp of this tick boundary.
    #[must_use]
    pub fn timestamp(&self) -> OffsetDateTime {
        self.0
    }

    /// Tag a raw value as `Fresh` or `Cached` based on when it was
    /// observed relative to this tick boundary.
    ///
    /// Per ADR-0021, this is the single place in the crate that decides
    /// between `Fresh` and `Cached`. Every observer that has a cache
    /// uses this method; no observer reimplements the comparison.
    ///
    /// - `observed_at >= tick_boundary` → [`Observation::Fresh`]
    /// - `observed_at < tick_boundary` → [`Observation::Cached`]
    #[must_use]
    pub fn tag<T>(&self, value: T, observed_at: OffsetDateTime) -> Observation<T> {
        if observed_at >= self.0 {
            Observation::Fresh { value, observed_at }
        } else {
            Observation::Cached { value, observed_at }
        }
    }

    /// Re-tag an existing [`Observation`] based on this tick boundary.
    ///
    /// `Unobserved` passes through unchanged. `Fresh` and `Cached` are
    /// re-evaluated against the tick boundary using the same rule as
    /// [`TickBoundary::tag`].
    #[must_use]
    pub fn retag<T>(&self, observation: Observation<T>) -> Observation<T> {
        match observation {
            Observation::Fresh { value, observed_at }
            | Observation::Cached { value, observed_at } => self.tag(value, observed_at),
            Observation::Unobserved => Observation::Unobserved,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ADR-0021 load-bearing test: a value observed *before* the tick
    /// boundary must be tagged as `Cached`.
    #[test]
    fn tag_produces_cached_when_observed_before_tick() {
        let observed_at = OffsetDateTime::now_utc() - time::Duration::hours(1);
        let tick = TickBoundary::now();

        let result = tick.tag(42u32, observed_at);
        match result {
            Observation::Cached { value, .. } => assert_eq!(value, 42),
            other => panic!("expected Cached, got {other:?}"),
        }
    }

    /// ADR-0021 load-bearing test: a value observed *at or after* the
    /// tick boundary must be tagged as `Fresh`.
    #[test]
    fn tag_produces_fresh_when_observed_at_tick() {
        let now = OffsetDateTime::now_utc();
        let tick = TickBoundary::at(now);

        let result = tick.tag(42u32, now);
        match result {
            Observation::Fresh { value, .. } => assert_eq!(value, 42),
            other => panic!("expected Fresh, got {other:?}"),
        }
    }

    #[test]
    fn tag_produces_fresh_when_observed_after_tick() {
        let tick_time = OffsetDateTime::now_utc() - time::Duration::seconds(5);
        let observed_at = OffsetDateTime::now_utc();
        let tick = TickBoundary::at(tick_time);

        let result = tick.tag(7u32, observed_at);
        match result {
            Observation::Fresh { value, .. } => assert_eq!(value, 7),
            other => panic!("expected Fresh, got {other:?}"),
        }
    }

    #[test]
    fn retag_converts_fresh_to_cached() {
        let observed_at = OffsetDateTime::now_utc() - time::Duration::hours(1);
        let tick = TickBoundary::now();

        let original = Observation::Fresh {
            value: 99u32,
            observed_at,
        };
        let result = tick.retag(original);
        match result {
            Observation::Cached { value, .. } => assert_eq!(value, 99),
            other => panic!("expected Cached, got {other:?}"),
        }
    }

    #[test]
    fn retag_converts_cached_to_fresh() {
        let now = OffsetDateTime::now_utc();
        let tick = TickBoundary::at(now - time::Duration::seconds(1));

        let original = Observation::Cached {
            value: 99u32,
            observed_at: now,
        };
        let result = tick.retag(original);
        match result {
            Observation::Fresh { value, .. } => assert_eq!(value, 99),
            other => panic!("expected Fresh, got {other:?}"),
        }
    }

    #[test]
    fn retag_passes_unobserved_through() {
        let tick = TickBoundary::now();
        let result: Observation<u32> = tick.retag(Observation::Unobserved);
        assert_eq!(result, Observation::Unobserved);
    }
}
