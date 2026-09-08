use std::{
    collections::{BTreeMap, HashMap, HashSet},
    hash::Hash,
    time::Duration,
};

struct CachedTimestampValue<V> {
    duration: Duration,
    value: V,
    cost: usize,
    last_access: u64,
}

pub(crate) struct TimestampCacheHit<V> {
    pub(crate) presentation_time: Duration,
    pub(crate) duration: Duration,
    pub(crate) value: V,
}

/// A byte-budgeted LRU cache keyed by media presentation timestamps.
///
/// A lookup hits only when a cached frame's presentation interval covers the
/// requested time. Otherwise the caller must decode.
pub(crate) struct BudgetedTimestampCache<K, V> {
    sequences: HashMap<K, BTreeMap<u64, CachedTimestampValue<V>>>,
    total_cost: usize,
    budget: usize,
    access_serial: u64,
}

impl<K, V> BudgetedTimestampCache<K, V>
where
    K: Clone + Eq + Hash,
{
    pub(crate) fn new(budget: usize) -> Self {
        Self {
            sequences: HashMap::new(),
            total_cost: 0,
            budget,
            access_serial: 0,
        }
    }

    pub(crate) fn contains_time(&self, sequence: &K, presentation_time: Duration) -> bool {
        self.position_for_time(sequence, presentation_time)
            .is_some()
    }

    pub(crate) fn get_for_time(
        &mut self,
        sequence: &K,
        presentation_time: Duration,
    ) -> Option<TimestampCacheHit<V>>
    where
        V: Clone,
    {
        let position = self.position_for_time(sequence, presentation_time)?;
        let access = self.next_access();
        let cached = self.sequences.get_mut(sequence)?.get_mut(&position)?;
        cached.last_access = access;
        Some(TimestampCacheHit {
            presentation_time: Duration::from_nanos(position),
            duration: cached.duration,
            value: cached.value.clone(),
        })
    }

    pub(crate) fn insert(
        &mut self,
        sequence: K,
        presentation_time: Duration,
        duration: Duration,
        value: V,
        cost: usize,
    ) {
        let position = timestamp_position(presentation_time);
        let access = self.next_access();
        let previous = self.sequences.entry(sequence).or_default().insert(
            position,
            CachedTimestampValue {
                duration,
                value,
                cost,
                last_access: access,
            },
        );
        if let Some(previous) = previous {
            self.total_cost = self.total_cost.saturating_sub(previous.cost);
        }
        self.total_cost = self.total_cost.saturating_add(cost);
    }

    pub(crate) fn evict_to_budget(
        &mut self,
        active_requests: impl IntoIterator<Item = (K, Duration)>,
    ) {
        let protected = active_requests
            .into_iter()
            .filter_map(|(sequence, time)| {
                Some((sequence.clone(), self.position_for_time(&sequence, time)?))
            })
            .collect::<HashSet<_>>();

        while self.total_cost > self.budget {
            let candidate = self
                .least_recently_used(|key| !protected.contains(key))
                .or_else(|| self.least_recently_used(|_| true));
            let Some((sequence, position)) = candidate else {
                break;
            };
            self.remove(&sequence, position);
        }
    }

    fn position_for_time(&self, sequence: &K, requested: Duration) -> Option<u64> {
        let requested = timestamp_position(requested);
        let values = self.sequences.get(sequence)?;
        let (position, cached) = values.range(..=requested).next_back()?;
        let end = position.saturating_add(timestamp_position(cached.duration));
        (requested < end || (cached.duration.is_zero() && requested == *position))
            .then_some(*position)
    }

    fn least_recently_used(&self, mut include: impl FnMut(&(K, u64)) -> bool) -> Option<(K, u64)> {
        let mut least_recent = None;
        for (sequence, values) in &self.sequences {
            for (position, cached) in values {
                let key = (sequence.clone(), *position);
                if !include(&key) {
                    continue;
                }
                let sort_key = (cached.last_access, *position);
                if least_recent
                    .as_ref()
                    .is_none_or(|(_, least_sort_key)| sort_key < *least_sort_key)
                {
                    least_recent = Some((key, sort_key));
                }
            }
        }
        least_recent.map(|(key, _)| key)
    }

    fn remove(&mut self, sequence: &K, position: u64) {
        let Some(values) = self.sequences.get_mut(sequence) else {
            return;
        };
        if let Some(removed) = values.remove(&position) {
            self.total_cost = self.total_cost.saturating_sub(removed.cost);
        }
        if values.is_empty() {
            self.sequences.remove(sequence);
        }
    }

    fn next_access(&mut self) -> u64 {
        self.access_serial = self.access_serial.saturating_add(1);
        self.access_serial
    }
}

fn timestamp_position(time: Duration) -> u64 {
    u64::try_from(time.as_nanos()).unwrap_or(u64::MAX)
}
