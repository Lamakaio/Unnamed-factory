//! Data parallelism on all cores with scoped threads (no dependency): the per-cell passes of
//! shaping, routing and erosion. Every helper gives the same result as the sequential loop.

/// Number of threads to split work into.
pub fn threads() -> usize {
    std::thread::available_parallelism().map_or(4, |n| n.get())
}

/// `f` over `0..len`, on all cores, in order.
pub fn map<T: Send>(len: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let chunk = len.div_ceil(threads()).max(1);
    if len <= chunk {
        return (0..len).map(f).collect();
    }
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..len)
            .step_by(chunk)
            .map(|start| {
                let f = &f;
                scope.spawn(move || (start..(start + chunk).min(len)).map(f).collect::<Vec<T>>())
            })
            .collect();
        let mut out = Vec::with_capacity(len);
        for handle in handles {
            out.extend(handle.join().unwrap());
        }
        out
    })
}

/// `f(index, item)` for every item of `items`, on all cores.
pub fn for_each_mut<T: Send>(items: &mut [T], f: impl Fn(usize, &mut T) + Sync) {
    let len = items.len();
    let chunk = len.div_ceil(threads()).max(1);
    std::thread::scope(|scope| {
        for (k, part) in items.chunks_mut(chunk).enumerate() {
            let f = &f;
            scope.spawn(move || {
                for (j, item) in part.iter_mut().enumerate() {
                    f(k * chunk + j, item);
                }
            });
        }
    });
}
