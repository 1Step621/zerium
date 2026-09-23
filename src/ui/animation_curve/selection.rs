use super::*;
use std::ops::Deref;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AnimationTarget {
    pub address: PropertyAddress,
}

impl Deref for AnimationTarget {
    type Target = PropertyAddress;

    fn deref(&self) -> &Self::Target {
        &self.address
    }
}

#[derive(Default)]
pub(crate) struct AnimationSelection {
    target: Option<AnimationTarget>,
    // The focused interval follows the playhead while the curve is queried,
    // so it intentionally does not trigger a second render notification.
    focused_segment: Cell<Option<usize>>,
}

impl AnimationSelection {
    pub(crate) fn target(&self) -> Option<&AnimationTarget> {
        self.target.as_ref()
    }

    pub(crate) fn focused_segment(&self) -> Option<usize> {
        self.focused_segment.get()
    }

    pub(crate) fn focus_segment(&self, segment: usize) {
        self.focused_segment.set(Some(segment));
    }

    pub(crate) fn select(&mut self, target: AnimationTarget, cx: &mut Context<Self>) {
        if self.target.as_ref() == Some(&target) {
            return;
        }
        self.target = Some(target);
        self.focused_segment.set(None);
        cx.notify();
    }

    pub(crate) fn clear_if(&mut self, target: &AnimationTarget, cx: &mut Context<Self>) {
        if self.target.as_ref() == Some(target) {
            self.target = None;
            self.focused_segment.set(None);
            cx.notify();
        }
    }

    pub(crate) fn clear(&mut self, cx: &mut Context<Self>) {
        if self.target.take().is_some() {
            self.focused_segment.set(None);
            cx.notify();
        }
    }
}
