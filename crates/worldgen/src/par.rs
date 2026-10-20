//! Data parallelism on all cores: the per-cell passes of shaping, routing, erosion and the
//! climate. Every helper gives the same result as the sequential loop.
//!
//! The work runs on a task pool of worldgen's own (Bevy's `bevy_tasks`, already in the game):
//! its threads live for the whole program, so even small passes repeated thousands of times
//! (the climate's moisture transport) cost no thread start each. Its own, not the game's
//! pools: `AsyncComputeTaskPool` (where the game generates the world) has a few threads only,
//! `ComputeTaskPool` is the frame's; and the examples and tests have no Bevy app to set those
//! up.
//!
//! Work is cut into many more chunks than threads, handed out as threads free up: per-cell
//! costs vary a lot (sea cells are cheap, mountains and coasts dear), and a fixed share per
//! thread would leave most cores idle waiting for the slowest.

use std::sync::{
    OnceLock,
    atomic::{AtomicUsize, Ordering},
};

use bevy_tasks::{TaskPool, TaskPoolBuilder};

/// Number of threads to split work into.
pub fn threads() -> usize {
    std::thread::available_parallelism().map_or(4, |n| n.get())
}

/// Worldgen's task pool (one thread per core; the thread waiting on a pass works too).
fn pool() -> &'static TaskPool {
    static POOL: OnceLock<TaskPool> = OnceLock::new();
    POOL.get_or_init(|| {
        TaskPoolBuilder::new()
            .num_threads(threads().saturating_sub(1).max(1))
            .thread_name("worldgen".into())
            .build()
    })
}

/// Chunks handed out per thread (more: better balance, more overhead).
const CHUNKS_PER_THREAD: usize = 8;
/// Below this many items, a single thread does it all.
const MIN_PARALLEL: usize = 4096;

/// Run `work(start, end)` over `0..len` in chunks on all cores; returns each chunk's result
/// in order.
fn chunked<R: Send + 'static>(len: usize, work: impl Fn(usize, usize) -> R + Sync) -> Vec<R> {
    let threads = threads();
    if len < MIN_PARALLEL || threads == 1 {
        return vec![work(0, len)];
    }
    let chunk = len.div_ceil(threads * CHUNKS_PER_THREAD).max(1);
    let chunks = len.div_ceil(chunk);
    let next = AtomicUsize::new(0);
    let (next, work) = (&next, &work);
    let mut results: Vec<(usize, R)> = pool()
        .scope(|scope| {
            for _ in 0..threads.min(chunks) {
                scope.spawn(async move {
                    let mut done = Vec::new();
                    loop {
                        let k = next.fetch_add(1, Ordering::Relaxed);
                        if k >= chunks {
                            break done;
                        }
                        let start = k * chunk;
                        done.push((k, work(start, (start + chunk).min(len))));
                    }
                });
            }
        })
        .into_iter()
        .flatten()
        .collect();
    results.sort_unstable_by_key(|r| r.0);
    results.into_iter().map(|r| r.1).collect()
}

/// `f` over `0..len`, on all cores, in order.
pub fn map<T: Send + 'static>(len: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let parts = chunked(len, |start, end| (start..end).map(&f).collect::<Vec<T>>());
    let mut out = Vec::with_capacity(len);
    for part in parts {
        out.extend(part);
    }
    out
}

/// `f(index, item)` for every item of `items`, on all cores.
pub fn for_each_mut<T: Send>(items: &mut [T], f: impl Fn(usize, &mut T) + Sync) {
    let shared = Shared(items.as_mut_ptr());
    chunked(items.len(), |start, end| {
        for k in start..end {
            // SAFETY: chunks are disjoint ranges of `items`, each handed to one thread.
            f(k, unsafe { shared.get(k) });
        }
    });
}

/// `f(group)` for each of `groups` (start, end) of a list, on all cores: for work in
/// independent groups of uneven size (the trees of a flow graph).
pub fn for_each_group(groups: &[(usize, usize)], f: impl Fn(usize, usize) + Sync) {
    chunked(groups.len(), |start, end| {
        for &(a, b) in &groups[start..end] {
            f(a, b);
        }
    });
}

/// A slice shared across threads that each write their own, disjoint, elements.
pub struct Shared<T>(*mut T);

// SAFETY: users only touch disjoint elements from different threads (see `get`).
unsafe impl<T: Send> Send for Shared<T> {}
unsafe impl<T: Send> Sync for Shared<T> {}

impl<T> Shared<T> {
    pub fn new(items: &mut [T]) -> Self {
        Self(items.as_mut_ptr())
    }

    /// Element `k`.
    ///
    /// # Safety
    /// `k` is in bounds, and no other thread touches element `k` meanwhile.
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn get(&self, k: usize) -> &mut T {
        unsafe { &mut *self.0.add(k) }
    }
}
