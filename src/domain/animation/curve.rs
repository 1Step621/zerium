//! Editable animation curves and control-point invariants.
use super::easing::{SegmentInterpolation, cubic, easing};
use serde::{Deserialize, Deserializer, Serialize};
const MIN_ANCHOR_DISTANCE: f32 = 0.001;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BezierHandle {
    In,
    Out,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct AnimationCurve {
    anchors: Vec<[f32; 2]>,
    segments: Vec<SegmentInterpolation>,
}

impl<'de> Deserialize<'de> for AnimationCurve {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error as _;
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawAnimationCurve {
            anchors: Vec<[f32; 2]>,
            segments: Vec<SegmentInterpolation>,
        }
        let raw = RawAnimationCurve::deserialize(deserializer)?;
        let curve = Self {
            anchors: raw.anchors,
            segments: raw.segments,
        };
        curve
            .is_valid()
            .then_some(curve)
            .ok_or_else(|| D::Error::custom("invalid animation curve"))
    }
}

impl Default for AnimationCurve {
    fn default() -> Self {
        Self {
            anchors: vec![[0., 0.], [1., 1.]],
            segments: vec![SegmentInterpolation::Linear],
        }
    }
}

impl AnimationCurve {
    pub(crate) fn anchors(&self) -> &[[f32; 2]] {
        &self.anchors
    }

    #[allow(dead_code)]
    pub(crate) fn segments(&self) -> &[SegmentInterpolation] {
        &self.segments
    }

    pub(crate) fn interpolation(&self, segment: usize) -> Option<SegmentInterpolation> {
        self.segments.get(segment).copied()
    }

    pub(crate) fn is_custom(&self, segment: usize) -> bool {
        self.segments.get(segment).is_some_and(|s| s.is_custom())
    }

    pub(crate) fn control(&self, index: usize, handle: BezierHandle) -> Option<[f32; 2]> {
        match handle {
            BezierHandle::Out => match self.segments.get(index) {
                Some(SegmentInterpolation::Custom { control_out, .. }) => Some(*control_out),
                _ => None,
            },
            BezierHandle::In => match index.checked_sub(1).and_then(|s| self.segments.get(s)) {
                Some(SegmentInterpolation::Custom { control_in, .. }) => Some(*control_in),
                _ => None,
            },
        }
    }

    fn clamp_anchor(position: [f32; 2]) -> [f32; 2] {
        [position[0].clamp(0., 1.), position[1].clamp(0., 1.)]
    }

    fn clamp_control(start: [f32; 2], end: [f32; 2], position: [f32; 2]) -> [f32; 2] {
        let (lo, hi) = (start[0].min(end[0]), start[0].max(end[0]));
        [position[0].clamp(lo, hi), position[1].clamp(0., 1.)]
    }

    pub(crate) fn is_valid(&self) -> bool {
        if self.anchors.len() < 2 || self.segments.len() != self.anchors.len() - 1 {
            return false;
        }
        if self.anchors.first().map(|a| a[0]) != Some(0.)
            || self.anchors.last().map(|a| a[0]) != Some(1.)
        {
            return false;
        }
        for (index, anchor) in self.anchors.iter().enumerate() {
            if !anchor.iter().all(|v| v.is_finite()) {
                return false;
            }
            if !(0. ..=1.).contains(&anchor[0]) || !(0. ..=1.).contains(&anchor[1]) {
                return false;
            }
            if index > 0 && anchor[0] <= self.anchors[index - 1][0] {
                return false;
            }
        }
        for (i, seg) in self.segments.iter().enumerate() {
            if let SegmentInterpolation::Custom {
                control_out,
                control_in,
            } = seg
            {
                let (s, e) = (self.anchors[i], self.anchors[i + 1]);
                for c in [control_out, control_in] {
                    if !c.iter().all(|v| v.is_finite()) {
                        return false;
                    }
                    if !(0. ..=1.).contains(&c[1]) {
                        return false;
                    }
                    if c[0] < s[0].min(e[0]) || c[0] > s[0].max(e[0]) {
                        return false;
                    }
                }
            }
        }
        true
    }

    pub(crate) fn add_anchor(&mut self, position: [f32; 2]) -> Option<usize> {
        if !position.iter().all(|value| value.is_finite()) {
            return None;
        }
        let position = Self::clamp_anchor(position);
        let insertion = self
            .anchors
            .partition_point(|anchor| anchor[0] < position[0]);
        if insertion == 0 || insertion == self.anchors.len() {
            return None;
        }
        let previous = self.anchors[insertion - 1];
        let next = self.anchors[insertion];
        if position[0] - previous[0] < MIN_ANCHOR_DISTANCE
            || next[0] - position[0] < MIN_ANCHOR_DISTANCE
        {
            return None;
        }
        self.anchors.insert(insertion, position);
        let interpolation = self.segments[insertion - 1];
        // Split: duplicate interpolation, but Custom controls are recomputed (no shape inheritance).
        let (left, right) = match interpolation {
            SegmentInterpolation::Custom { .. } => (
                SegmentInterpolation::custom_default(previous, position),
                SegmentInterpolation::custom_default(position, next),
            ),
            other => (other, other),
        };
        self.segments[insertion - 1] = left;
        self.segments.insert(insertion, right);
        Some(insertion)
    }

    pub(crate) fn set_anchor(&mut self, index: usize, position: [f32; 2]) -> bool {
        if !position.iter().all(|value| value.is_finite()) {
            return false;
        }
        let Some(&anchor) = self.anchors.get(index) else {
            return false;
        };
        let min_x = if index == 0 {
            0.
        } else {
            self.anchors[index - 1][0] + MIN_ANCHOR_DISTANCE
        };
        let max_x = if index + 1 == self.anchors.len() {
            1.
        } else {
            self.anchors[index + 1][0] - MIN_ANCHOR_DISTANCE
        };
        let x = if index == 0 {
            0.
        } else if index + 1 == self.anchors.len() {
            1.
        } else {
            position[0].clamp(min_x, max_x)
        };
        let next_position = [x, position[1].clamp(0., 1.)];
        if next_position == anchor {
            return false;
        }
        self.anchors[index] = next_position;
        // Move adjacent Custom controls rigidly with the anchor.
        let delta = [next_position[0] - anchor[0], next_position[1] - anchor[1]];
        let shift = |c: &mut [f32; 2]| {
            c[0] += delta[0];
            c[1] += delta[1];
        };
        if let Some(SegmentInterpolation::Custom { control_out, .. }) = self.segments.get_mut(index)
        {
            shift(control_out);
        }
        if index > 0
            && let Some(SegmentInterpolation::Custom { control_in, .. }) =
                self.segments.get_mut(index - 1)
        {
            shift(control_in);
        }
        // Clamp moved controls into their segments.
        for seg in [
            index.checked_sub(1),
            (index < self.segments.len()).then_some(index),
        ]
        .into_iter()
        .flatten()
        {
            let (s, e) = (self.anchors[seg], self.anchors[seg + 1]);
            if let Some(SegmentInterpolation::Custom {
                control_out,
                control_in,
            }) = self.segments.get_mut(seg)
            {
                *control_out = Self::clamp_control(s, e, *control_out);
                *control_in = Self::clamp_control(s, e, *control_in);
            }
        }
        true
    }

    pub(crate) fn set_handle(
        &mut self,
        index: usize,
        handle: BezierHandle,
        position: [f32; 2],
    ) -> bool {
        if !position.iter().all(|value| value.is_finite()) {
            return false;
        }
        let segment = match handle {
            BezierHandle::In => index.checked_sub(1),
            BezierHandle::Out => (index < self.segments.len()).then_some(index),
        };
        let Some(segment) = segment else {
            return false;
        };
        if index >= self.anchors.len() {
            return false;
        }
        let (s, e) = (self.anchors[segment], self.anchors[segment + 1]);
        let position = Self::clamp_control(s, e, position);
        match (handle, self.segments.get_mut(segment)) {
            (BezierHandle::Out, Some(SegmentInterpolation::Custom { control_out, .. })) => {
                if *control_out == position {
                    return false;
                }
                *control_out = position;
                true
            }
            (BezierHandle::In, Some(SegmentInterpolation::Custom { control_in, .. })) => {
                if *control_in == position {
                    return false;
                }
                *control_in = position;
                true
            }
            _ => false,
        }
    }

    pub(crate) fn remove_anchor(&mut self, index: usize) -> bool {
        if index == 0 || index + 1 >= self.anchors.len() {
            return false;
        }
        let left = self.segments[index - 1];
        let right = self.segments[index];
        self.anchors.remove(index);
        // Merge: keep interpolation only if both sides agree; otherwise fall back to Linear (no inheritance).
        self.segments[index - 1] = if left == right && !left.is_custom() {
            left
        } else {
            SegmentInterpolation::Linear
        };
        self.segments.remove(index);
        true
    }

    pub(crate) fn remap_time_range(&mut self, start: f64, end: f64) {
        debug_assert!(start.is_finite() && end.is_finite() && start < end);
        if !start.is_finite() || !end.is_finite() || start >= end || (start == 0. && end == 1.) {
            return;
        }
        let start = start as f32;
        let end = end as f32;
        if start > 0. {
            let first = self
                .anchors
                .partition_point(|anchor| anchor[0] <= start)
                .saturating_sub(1);
            self.anchors.drain(..first);
            self.segments.drain(..first);
            self.anchors[0][0] = start;
        } else if start < 0. {
            if self.anchors[0][1] == self.anchors[1][1] {
                self.anchors[0][0] = start;
            } else {
                let value = self.anchors[0][1];
                self.anchors.insert(0, [start, value]);
                self.segments.insert(0, SegmentInterpolation::Linear);
            }
        }
        if end < 1. {
            let last = self
                .anchors
                .partition_point(|anchor| anchor[0] < end)
                .min(self.anchors.len().saturating_sub(1));
            self.anchors.truncate(last + 1);
            self.segments.truncate(last);
            self.anchors[last][0] = end;
        } else if end > 1. {
            let last = self.anchors.len() - 1;
            if self.anchors[last - 1][1] == self.anchors[last][1] {
                self.anchors[last][0] = end;
            } else {
                let value = self.anchors[last][1];
                self.anchors.push([end, value]);
                self.segments.push(SegmentInterpolation::Linear);
            }
        }
        let span = end - start;
        for anchor in &mut self.anchors {
            anchor[0] = (anchor[0] - start) / span;
        }
        for (i, seg) in self.segments.iter_mut().enumerate() {
            if let SegmentInterpolation::Custom {
                control_out,
                control_in,
            } = seg
            {
                control_out[0] = (control_out[0] - start) / span;
                control_in[0] = (control_in[0] - start) / span;
            }
            // Non-custom segments carry no geometry; Custom x-range is clamped below.
            let (s, e) = (self.anchors[i], self.anchors[i + 1]);
            if let SegmentInterpolation::Custom {
                control_out,
                control_in,
            } = seg
            {
                *control_out = Self::clamp_control(s, e, *control_out);
                *control_in = Self::clamp_control(s, e, *control_in);
            }
        }
        self.anchors[0][0] = 0.;
        self.anchors.last_mut().expect("curve has endpoints")[0] = 1.;
        debug_assert!(self.is_valid());
    }

    pub(crate) fn set_interpolation(
        &mut self,
        segment: usize,
        interpolation: SegmentInterpolation,
    ) -> bool {
        // Custom always gets fresh linear defaults: no shape inheritance.
        if matches!(interpolation, SegmentInterpolation::Custom { .. }) {
            return self.set_custom(segment);
        }
        let Some(target) = self.segments.get(segment).copied() else {
            return false;
        };
        if target == interpolation {
            return false;
        }
        self.segments[segment] = interpolation;
        true
    }

    pub(crate) fn set_custom(&mut self, segment: usize) -> bool {
        if segment >= self.segments.len() {
            return false;
        }
        let (s, e) = (self.anchors[segment], self.anchors[segment + 1]);
        let next = SegmentInterpolation::custom_default(s, e);
        if self.segments[segment] == next {
            return false;
        }
        self.segments[segment] = next;
        true
    }

    pub(crate) fn evaluate(&self, progress: f32) -> f32 {
        let x = progress.clamp(0., 1.);
        if x == 0. {
            return self.anchors.first().map_or(0., |anchor| anchor[1]);
        }
        if x == 1. {
            return self.anchors.last().map_or(1., |anchor| anchor[1]);
        }
        let segment = self
            .anchors
            .windows(2)
            .position(|anchors| x <= anchors[1][0])
            .unwrap_or_else(|| self.anchors.len().saturating_sub(2));
        let start = self.anchors[segment];
        let end = self.anchors[segment + 1];
        let interpolation = self.segments[segment];
        if let SegmentInterpolation::Custom {
            control_out,
            control_in,
        } = interpolation
        {
            let mut low = 0.;
            let mut high = 1.;
            for _ in 0..16 {
                let t = (low + high) * 0.5;
                if cubic(start[0], control_out[0], control_in[0], end[0], t) < x {
                    low = t;
                } else {
                    high = t;
                }
            }
            cubic(
                start[1],
                control_out[1],
                control_in[1],
                end[1],
                (low + high) * 0.5,
            )
        } else {
            let local_progress = (x - start[0]) / (end[0] - start[0]);
            let eased = easing(interpolation, local_progress);
            start[1] + (end[1] - start[1]) * eased
        }
    }
}
