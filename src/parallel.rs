// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

//! A small work-stealing helper over scoped threads.
//!
//! Sizing a cache, finding a package manager's cache directory and counting a repository's
//! dependents are all independent reads of the file system whose cost varies by orders of
//! magnitude from one job to the next. A fixed slice per thread leaves every thread idle
//! behind whichever one drew the 160 GiB tree, so jobs are taken off a shared cursor
//! instead. The thread-count rule is the status scan's, read from the same environment
//! variable and constants, so one setting governs every parallel pass.

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::constants;

/// How many workers to use for `total` jobs, from [`constants::STATUS_SCAN_THREADS_ENV`]
/// or two per reported core, clamped by [`clamp_workers`].
pub fn worker_count(total: usize) -> usize {
    let requested = explicit_threads().unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(std::num::NonZeroUsize::get)
            .unwrap_or(4)
            .saturating_mul(constants::STATUS_SCAN_THREADS_PER_CORE)
    });
    clamp_workers(requested, total)
}

/// [`worker_count`] for the `devp caches` walks, which stop scaling well before the
/// status scan does.
///
/// The environment variable still wins, so someone who knows their disk can ask for more.
/// Without it: half the cores, between [`constants::CACHE_WALK_MIN_THREADS`] and
/// [`constants::CACHE_WALK_MAX_THREADS`].
pub fn walk_worker_count(total: usize) -> usize {
    let cores = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(4);
    walk_workers_for(cores, explicit_threads(), total)
}

/// The decision inside [`walk_worker_count`], without the environment or core-count reads.
fn walk_workers_for(cores: usize, explicit: Option<usize>, total: usize) -> usize {
    let requested = explicit.unwrap_or_else(|| {
        (cores / 2).clamp(
            constants::CACHE_WALK_MIN_THREADS,
            constants::CACHE_WALK_MAX_THREADS,
        )
    });
    clamp_workers(requested, total)
}

fn explicit_threads() -> Option<usize> {
    std::env::var(constants::STATUS_SCAN_THREADS_ENV)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
}

/// The clamping half of [`worker_count`], without the environment read, so the bounds can
/// be tested without mutating process-wide state.
///
/// Never more workers than jobs, never fewer than one, because the calling thread is
/// itself the first worker.
pub fn clamp_workers(requested: usize, total: usize) -> usize {
    requested
        .clamp(1, constants::STATUS_SCAN_MAX_THREADS)
        .min(total.max(1))
}

/// Run `task` on up to `workers` threads, the calling thread included, and return what
/// each one returned.
///
/// `Builder::spawn_scoped` rather than `scope.spawn`, which panics when the OS refuses a
/// thread: under a low process limit or in a constrained container. A refusal here means
/// fewer workers, and because the calling thread always runs `task` itself, the worst
/// case is a sequential pass that still finishes. A panicking worker is re-raised rather
/// than swallowed, so a short result is never returned as if it were complete.
pub fn run_workers<R: Send>(workers: usize, task: impl Fn() -> R + Sync) -> Vec<R> {
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers.saturating_sub(1));
        for n in 1..workers {
            match std::thread::Builder::new()
                .name(format!("devp-worker-{n}"))
                .spawn_scoped(scope, &task)
            {
                Ok(handle) => handles.push(handle),
                Err(_) => break,
            }
        }
        let mut out = vec![task()];
        out.extend(
            handles
                .into_iter()
                .map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e))),
        );
        out
    })
}

/// Apply `job` to every item on up to `workers` threads and return the results in input
/// order, whichever worker finished which.
pub fn map_ordered<T: Sync, R: Send>(
    items: &[T],
    workers: usize,
    job: impl Fn(usize, &T) -> R + Sync,
) -> Vec<R> {
    let total = items.len();
    let next = AtomicUsize::new(0);
    let chunks = run_workers(workers.min(total.max(1)), || {
        let mut mine = Vec::new();
        loop {
            let i = next.fetch_add(1, Ordering::Relaxed);
            if i >= total {
                break;
            }
            mine.push((i, job(i, &items[i])));
        }
        mine
    });

    let mut all: Vec<(usize, R)> = chunks.into_iter().flatten().collect();
    all.sort_unstable_by_key(|(i, _)| *i);
    all.into_iter().map(|(_, r)| r).collect()
}

/// Start `job` on a scoped thread now, so it overlaps whatever the caller does next.
///
/// `Fn` and borrowed rather than `FnOnce`: a refused spawn consumes the closure it was
/// given, and the caller needs the job back to run it itself. `None` means no thread
/// could be had, and [`finish`] then runs the job on the caller's thread.
pub fn start<'scope, 'env, F, R>(
    scope: &'scope std::thread::Scope<'scope, 'env>,
    name: &str,
    job: &'scope F,
) -> Option<std::thread::ScopedJoinHandle<'scope, R>>
where
    F: Fn() -> R + Sync + 'scope,
    R: Send + 'scope,
{
    std::thread::Builder::new()
        .name(name.to_string())
        .spawn_scoped(scope, job)
        .ok()
}

/// The result of a job from [`start`], waiting for its thread or running it here.
///
/// A panic on the job's thread is re-raised, so a missing result is never mistaken for an
/// empty one.
pub fn finish<R>(started: Option<std::thread::ScopedJoinHandle<'_, R>>, job: impl Fn() -> R) -> R {
    match started {
        Some(handle) => handle
            .join()
            .unwrap_or_else(|e| std::panic::resume_unwind(e)),
        None => job(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_sequential_result_for_zero_one_and_many_jobs() {
        for n in [0usize, 1, 2, 7, 200] {
            let items: Vec<usize> = (0..n).collect();
            let sequential: Vec<usize> = items.iter().map(|x| x * x + 1).collect();
            let parallel = map_ordered(&items, 8, |_, x| x * x + 1);
            assert_eq!(parallel, sequential, "{n} jobs");
        }
    }

    #[test]
    fn keeps_input_order_when_early_jobs_finish_last() {
        let items: Vec<u64> = (0..16).collect();
        let out = map_ordered(&items, 4, |i, x| {
            // The first jobs sleep longest, so any result order other than the input's
            // would show.
            std::thread::sleep(std::time::Duration::from_millis(16 - *x));
            i
        });
        assert_eq!(out, (0..16).collect::<Vec<_>>());
    }

    #[test]
    fn more_jobs_than_workers_all_run_exactly_once() {
        let items = vec![(); 100];
        let ran = AtomicUsize::new(0);
        let out = map_ordered(&items, 3, |i, ()| {
            ran.fetch_add(1, Ordering::SeqCst);
            i
        });
        assert_eq!(ran.load(Ordering::SeqCst), 100);
        assert_eq!(out.len(), 100);
    }

    #[test]
    fn zero_workers_still_runs_on_the_calling_thread() {
        let out = map_ordered(&[1, 2, 3], 0, |_, x| x + 1);
        assert_eq!(out, vec![2, 3, 4]);
    }

    #[test]
    fn a_started_job_overlaps_the_caller_and_finish_returns_its_result() {
        let ran_on = std::sync::Mutex::new(None);
        let job = || {
            *ran_on.lock().unwrap() = Some(std::thread::current().id());
            41 + 1
        };
        let answer = std::thread::scope(|scope| {
            let started = start(scope, "devp-test-job", &job);
            finish(started, job)
        });
        assert_eq!(answer, 42);
        assert!(ran_on.lock().unwrap().is_some());
    }

    #[test]
    fn finish_runs_the_job_itself_when_no_thread_was_started() {
        let calls = AtomicUsize::new(0);
        let answer = finish(None, || {
            calls.fetch_add(1, Ordering::SeqCst);
            7
        });
        assert_eq!((answer, calls.load(Ordering::SeqCst)), (7, 1));
    }

    #[test]
    fn the_walk_default_is_half_the_cores_within_its_bounds() {
        let walk = |cores, explicit| walk_workers_for(cores, explicit, usize::MAX);
        assert_eq!(walk(2, None), constants::CACHE_WALK_MIN_THREADS);
        assert_eq!(walk(10, None), 5);
        assert_eq!(walk(128, None), constants::CACHE_WALK_MAX_THREADS);
        // An explicit request is the one way past the walk ceiling.
        assert_eq!(walk(16, Some(24)), 24);
        assert_eq!(walk(16, Some(9_999)), constants::STATUS_SCAN_MAX_THREADS);
        assert_eq!(walk_workers_for(16, None, 3), 3);
    }

    #[test]
    fn clamping_never_exceeds_jobs_or_the_ceiling_and_never_reaches_zero() {
        assert_eq!(clamp_workers(32, 3), 3);
        assert_eq!(clamp_workers(0, 0), 1);
        assert_eq!(clamp_workers(0, 50), 1);
        assert_eq!(
            clamp_workers(9_999, 500),
            constants::STATUS_SCAN_MAX_THREADS
        );
        assert_eq!(clamp_workers(16, 1), 1);
    }
}
