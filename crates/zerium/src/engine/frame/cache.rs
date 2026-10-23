//! Bounded retention of immutable raster results, independent of their producer.
use std::{
    collections::{BTreeMap, HashMap},
    hash::Hash,
    sync::Arc,
};

use super::VisualFrame;

pub(crate) const RASTER_CACHE_BYTES: usize = 128 * 1024 * 1024;

struct CachedFrame {
    frame: Arc<VisualFrame>,
    last_used: u64,
}

pub(crate) struct FrameCache<K> {
    frames: HashMap<Arc<K>, CachedFrame>,
    recency: BTreeMap<u64, Arc<K>>,
    byte_budget: usize,
    resident_bytes: usize,
    clock: u64,
}

impl<K: Eq + Hash> FrameCache<K> {
    pub(crate) fn new(byte_budget: usize) -> Self {
        Self {
            frames: HashMap::new(),
            recency: BTreeMap::new(),
            byte_budget,
            resident_bytes: 0,
            clock: 0,
        }
    }

    pub(crate) fn byte_budget(&self) -> usize {
        self.byte_budget
    }

    fn next_tick(&mut self) -> u64 {
        if self.clock == u64::MAX {
            self.recency = std::mem::take(&mut self.recency)
                .into_values()
                .enumerate()
                .map(|(index, key)| {
                    let tick = index as u64;
                    self.frames.get_mut(&key).expect("cached frame").last_used = tick;
                    (tick, key)
                })
                .collect();
            self.clock = self.recency.len() as u64;
        }
        self.clock += 1;
        self.clock
    }

    pub(crate) fn get(&mut self, key: &K) -> Option<Arc<VisualFrame>> {
        let tick = self.next_tick();
        let cached = self.frames.get_mut(key)?;
        let key = self
            .recency
            .remove(&cached.last_used)
            .expect("cached recency");
        cached.last_used = tick;
        self.recency.insert(tick, key);
        Some(cached.frame.clone())
    }

    fn make_room(&mut self, bytes: usize) -> bool {
        let Some(available) = self.byte_budget.checked_sub(bytes) else {
            return false;
        };
        let mut remaining = self.resident_bytes;
        let mut evictions = Vec::new();
        // A live graph pins its exact pixel version, not all versions from the
        // same source. Never discard that identity to admit an unrelated raster.
        for (&tick, key) in &self.recency {
            if remaining <= available {
                break;
            }
            let cached = &self.frames[key];
            if Arc::strong_count(&cached.frame) == 1 {
                remaining -= cached.frame.byte_len();
                evictions.push(tick);
            }
        }
        for tick in evictions {
            let key = self.recency.remove(&tick).expect("cached recency");
            self.frames.remove(&key);
        }
        self.resident_bytes = remaining;
        remaining <= available
    }

    pub(crate) fn insert(&mut self, key: K, frame: Arc<VisualFrame>) {
        if let Some(previous) = self.frames.remove(&key) {
            self.recency.remove(&previous.last_used);
            self.resident_bytes -= previous.frame.byte_len();
        }
        let bytes = frame.byte_len();
        // A result larger than the budget, or competing with pinned results,
        // remains usable by its caller without expanding cache retention.
        if !self.make_room(bytes) {
            return;
        }
        let tick = self.next_tick();
        let key = Arc::new(key);
        self.frames.insert(
            key.clone(),
            CachedFrame {
                frame,
                last_used: tick,
            },
        );
        self.recency.insert(tick, key);
        self.resident_bytes += bytes;
    }
}
