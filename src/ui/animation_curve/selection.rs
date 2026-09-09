use super::*;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AnimationTarget {
    pub item_id: ItemId,
    pub effect_id: Option<EffectInstanceId>,
    pub parameter_id: String,
    pub address: ParameterAnimationAddress,
    pub property: PropertyPath,
}

#[derive(Clone, Debug)]
pub(crate) struct AnimationPresentation {
    pub label: String,
    pub suffix: String,
    pub step: f64,
    pub value_factor: f64,
}

#[derive(Default)]
pub(crate) struct AnimationSelection {
    target: Option<AnimationTarget>,
}

impl AnimationSelection {
    pub(crate) fn target(&self) -> Option<&AnimationTarget> {
        self.target.as_ref()
    }

    pub(crate) fn select(&mut self, target: AnimationTarget, cx: &mut Context<Self>) {
        if self.target.as_ref() == Some(&target) {
            return;
        }
        self.target = Some(target);
        cx.notify();
    }

    pub(crate) fn clear_if(&mut self, target: &AnimationTarget, cx: &mut Context<Self>) {
        if self.target.as_ref() == Some(target) {
            self.target = None;
            cx.notify();
        }
    }

    pub(crate) fn clear(&mut self, cx: &mut Context<Self>) {
        if self.target.take().is_some() {
            cx.notify();
        }
    }
}
