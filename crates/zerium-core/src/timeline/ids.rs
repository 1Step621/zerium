use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

/// Stable identity of an item within a project.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct ItemId(pub u64);

impl ItemId {
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Stable identity of a project. Scene identity includes this value so that
/// clipboard data cannot alias an unrelated scene with the same local ordinal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ProjectId {
    high: u64,
    low: u64,
}

impl ProjectId {
    pub(super) fn generate() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let high = (timestamp >> 64) as u64;
        let low = timestamp as u64
            ^ u64::from(std::process::id()).rotate_left(32)
            ^ COUNTER.fetch_add(1, Ordering::Relaxed).rotate_left(17);
        Self::from_parts(high, low).expect("generated project identity must be non-zero")
    }

    pub const fn from_parts(high: u64, low: u64) -> Option<Self> {
        if high == 0 && low == 0 {
            return None;
        }
        Some(Self { high, low })
    }

    pub const fn high(self) -> u64 {
        self.high
    }

    pub const fn low(self) -> u64 {
        self.low
    }
}

/// Stable identity of a reusable scene. The local ordinal is meaningful only
/// together with its project identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SceneId {
    project: ProjectId,
    local: u64,
}

impl SceneId {
    pub const fn new(project: ProjectId, local: u64) -> Self {
        Self { project, local }
    }

    pub const fn project(self) -> ProjectId {
        self.project
    }

    pub const fn get(self) -> u64 {
        self.local
    }
}

/// Timeline row identity. Layers are intentionally sparse and ordered by ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LayerId(pub(super) u64);

impl LayerId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Project-wide stable identity of one effect instance in an item's stack.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct EffectInstanceId(pub(super) u64);

impl EffectInstanceId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}
