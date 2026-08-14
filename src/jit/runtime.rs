//! Process-wide native-JIT admission and executable-memory accounting.
//!
//! Generated programs remain owned by their existing request/plan caches. This
//! module only supplies a shared production boundary: runtime opt-out and a
//! hard cap on live RX mappings. Reaching the cap declines compilation; the
//! caller then continues through the existing typed executor.

use std::ffi::OsStr;
use std::io;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

pub const DEFAULT_CODE_MAPPING_LIMIT_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_CODE_MAPPING_LIMIT_BYTES: usize = 1024 * 1024 * 1024;

static ENABLED: OnceLock<bool> = OnceLock::new();
static CODE_MAPPING_LIMIT_BYTES: OnceLock<usize> = OnceLock::new();
static LIVE_CODE_MAPPING_BYTES: AtomicUsize = AtomicUsize::new(0);
static PEAK_CODE_MAPPING_BYTES: AtomicUsize = AtomicUsize::new(0);
static LIVE_CODE_MAPPINGS: AtomicUsize = AtomicUsize::new(0);
static PEAK_CODE_MAPPINGS: AtomicUsize = AtomicUsize::new(0);
static CREATED_CODE_MAPPINGS: AtomicU64 = AtomicU64::new(0);
static DISABLED_REJECTIONS: AtomicU64 = AtomicU64::new(0);
static BUDGET_REJECTIONS: AtomicU64 = AtomicU64::new(0);
static SYSTEM_FAILURES: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeTelemetry {
    pub enabled: bool,
    pub mapping_limit_bytes: usize,
    pub live_mapping_bytes: usize,
    pub peak_mapping_bytes: usize,
    pub live_mappings: usize,
    pub peak_mappings: usize,
    pub created_mappings: u64,
    pub disabled_rejections: u64,
    pub budget_rejections: u64,
    pub system_failures: u64,
}

#[inline]
pub fn enabled() -> bool {
    *ENABLED
        .get_or_init(|| enabled_from_disable_value(std::env::var_os("RPHP_DISABLE_JIT").as_deref()))
}

pub(crate) fn ensure_compilation_enabled() -> io::Result<()> {
    if enabled() {
        return Ok(());
    }
    DISABLED_REJECTIONS.fetch_add(1, Ordering::Relaxed);
    Err(io::Error::new(
        io::ErrorKind::PermissionDenied,
        "native JIT is disabled",
    ))
}

#[inline]
fn enabled_from_disable_value(disable_value: Option<&OsStr>) -> bool {
    disable_value.is_none()
}

#[inline]
fn mapping_limit_bytes() -> usize {
    *CODE_MAPPING_LIMIT_BYTES.get_or_init(|| {
        mapping_limit_from_value(std::env::var_os("RPHP_JIT_CODE_LIMIT_BYTES").as_deref())
    })
}

fn mapping_limit_from_value(value: Option<&OsStr>) -> usize {
    value
        .and_then(OsStr::to_str)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_CODE_MAPPING_LIMIT_BYTES)
        .min(MAX_CODE_MAPPING_LIMIT_BYTES)
}

pub fn telemetry() -> RuntimeTelemetry {
    RuntimeTelemetry {
        enabled: enabled(),
        mapping_limit_bytes: mapping_limit_bytes(),
        live_mapping_bytes: LIVE_CODE_MAPPING_BYTES.load(Ordering::Relaxed),
        peak_mapping_bytes: PEAK_CODE_MAPPING_BYTES.load(Ordering::Relaxed),
        live_mappings: LIVE_CODE_MAPPINGS.load(Ordering::Relaxed),
        peak_mappings: PEAK_CODE_MAPPINGS.load(Ordering::Relaxed),
        created_mappings: CREATED_CODE_MAPPINGS.load(Ordering::Relaxed),
        disabled_rejections: DISABLED_REJECTIONS.load(Ordering::Relaxed),
        budget_rejections: BUDGET_REJECTIONS.load(Ordering::Relaxed),
        system_failures: SYSTEM_FAILURES.load(Ordering::Relaxed),
    }
}

pub(crate) struct MappingReservation {
    bytes: usize,
}

impl MappingReservation {
    pub(crate) fn acquire(bytes: usize) -> Option<Self> {
        if !enabled() {
            DISABLED_REJECTIONS.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        if !try_acquire_bytes(&LIVE_CODE_MAPPING_BYTES, bytes, mapping_limit_bytes()) {
            BUDGET_REJECTIONS.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        let live_mappings = LIVE_CODE_MAPPINGS.fetch_add(1, Ordering::Relaxed) + 1;
        update_peak(
            &PEAK_CODE_MAPPING_BYTES,
            LIVE_CODE_MAPPING_BYTES.load(Ordering::Relaxed),
        );
        update_peak(&PEAK_CODE_MAPPINGS, live_mappings);
        Some(Self { bytes })
    }

    pub(crate) fn record_created(&self) {
        CREATED_CODE_MAPPINGS.fetch_add(1, Ordering::Relaxed);
    }
}

impl Drop for MappingReservation {
    fn drop(&mut self) {
        LIVE_CODE_MAPPING_BYTES.fetch_sub(self.bytes, Ordering::Relaxed);
        LIVE_CODE_MAPPINGS.fetch_sub(1, Ordering::Relaxed);
    }
}

pub(crate) fn record_system_failure() {
    SYSTEM_FAILURES.fetch_add(1, Ordering::Relaxed);
}

fn try_acquire_bytes(counter: &AtomicUsize, bytes: usize, limit: usize) -> bool {
    let mut current = counter.load(Ordering::Relaxed);
    loop {
        let Some(next) = current.checked_add(bytes) else {
            return false;
        };
        if next > limit {
            return false;
        }
        match counter.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return true,
            Err(observed) => current = observed,
        }
    }
}

fn update_peak(peak: &AtomicUsize, value: usize) {
    let mut current = peak.load(Ordering::Relaxed);
    while value > current {
        match peak.compare_exchange_weak(current, value, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return,
            Err(observed) => current = observed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disable_variable_uses_presence_semantics() {
        assert!(enabled_from_disable_value(None));
        assert!(!enabled_from_disable_value(Some(OsStr::new(""))));
        assert!(!enabled_from_disable_value(Some(OsStr::new("0"))));
    }

    #[test]
    fn code_limit_is_bounded_and_malformed_values_use_the_default() {
        assert_eq!(
            mapping_limit_from_value(None),
            DEFAULT_CODE_MAPPING_LIMIT_BYTES
        );
        assert_eq!(
            mapping_limit_from_value(Some(OsStr::new("invalid"))),
            DEFAULT_CODE_MAPPING_LIMIT_BYTES
        );
        assert_eq!(mapping_limit_from_value(Some(OsStr::new("4096"))), 4096);
        let oversized = (MAX_CODE_MAPPING_LIMIT_BYTES + 1).to_string();
        assert_eq!(
            mapping_limit_from_value(Some(OsStr::new(&oversized))),
            MAX_CODE_MAPPING_LIMIT_BYTES
        );
    }

    #[test]
    fn reservation_never_exceeds_its_limit() {
        let counter = AtomicUsize::new(0);
        assert!(try_acquire_bytes(&counter, 4096, 8192));
        assert!(try_acquire_bytes(&counter, 4096, 8192));
        assert!(!try_acquire_bytes(&counter, 1, 8192));
        assert_eq!(counter.load(Ordering::Relaxed), 8192);
    }
}
