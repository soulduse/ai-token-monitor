//! Shared crash-resilience primitives for provider stats pipelines.
//!
//! Every provider guards its parse with a `static PARSING: AtomicBool` (so only
//! one thread parses at a time) and keeps results in a `static STATS_CACHE:
//! Mutex<...>`. Both are unwind-fragile in their raw form:
//!
//! - a panic between `PARSING.store(true)` and `PARSING.store(false)` leaves the
//!   flag set forever, so every later fetch takes the "someone else is parsing"
//!   path and returns stale data until the app restarts;
//! - a panic while holding the cache mutex poisons it, and the widespread
//!   `if let Ok(cache) = STATS_CACHE.lock()` pattern then silently skips the
//!   cache — or worse, silently skips *writing* it — on every call thereafter.
//!
//! Together these turn one panic anywhere in a parse into "the numbers never
//! move again until relaunch", with nothing in the UI to say so. Observed in the
//! field on 2026-08-04 (macOS, stats frozen while logs kept growing; a relaunch
//! fixed it). [`ParsingGuard`] releases the flag on unwind, and
//! [`lock_unpoisoned`] treats a poisoned mutex as recoverable — every site
//! replaces the whole cached value rather than mutating it in place, so a
//! half-finished update cannot be observed.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// RAII holder of a provider's `PARSING` flag: released on drop, including
/// drops that happen because a parse panicked and is unwinding.
pub(crate) struct ParsingGuard(&'static AtomicBool);

impl ParsingGuard {
    /// Claim `flag`, or return `None` when another thread already holds it
    /// (the caller then serves stale cache instead of double-parsing).
    pub(crate) fn try_acquire(flag: &'static AtomicBool) -> Option<Self> {
        flag.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
            .then_some(Self(flag))
    }
}

impl Drop for ParsingGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Lock `mutex`, recovering the data if a previous holder panicked. Poisoning
/// is just a flag on the mutex — the data inside is whatever the last complete
/// write left there, which for the stats caches is always a consistent value.
pub(crate) fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_FLAG: AtomicBool = AtomicBool::new(false);

    // The whole point of the guard: a panicking parse must not leave the flag
    // set, or every fetch after the panic serves stale data forever.
    #[test]
    fn parsing_guard_releases_on_panic() {
        let result = std::panic::catch_unwind(|| {
            let _guard = ParsingGuard::try_acquire(&TEST_FLAG).expect("flag free");
            panic!("simulated parse crash");
        });
        assert!(result.is_err(), "the parse did panic");
        assert!(
            !TEST_FLAG.load(Ordering::SeqCst),
            "flag must be released by unwind, not stuck forever"
        );
        // And the flag is immediately reusable.
        let guard = ParsingGuard::try_acquire(&TEST_FLAG);
        assert!(guard.is_some(), "next fetch must be able to parse again");
    }

    #[test]
    fn parsing_guard_excludes_concurrent_holders() {
        let first = ParsingGuard::try_acquire(&TEST_FLAG).expect("flag free");
        assert!(ParsingGuard::try_acquire(&TEST_FLAG).is_none(), "held flags stay exclusive");
        drop(first);
        assert!(ParsingGuard::try_acquire(&TEST_FLAG).is_some(), "released flags are reusable");
    }

    // A panic while holding the cache mutex must not make the cache unreachable.
    #[test]
    fn lock_unpoisoned_recovers_after_a_panic() {
        let mutex = Mutex::new(41);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _held = mutex.lock().unwrap();
            panic!("simulated crash while holding the cache");
        }));
        assert!(result.is_err());
        assert!(mutex.lock().is_err(), "the mutex really is poisoned");

        let mut guard = lock_unpoisoned(&mutex);
        assert_eq!(*guard, 41, "the last complete value survives");
        *guard = 42;
        drop(guard);
        assert_eq!(*lock_unpoisoned(&mutex), 42);
    }
}
