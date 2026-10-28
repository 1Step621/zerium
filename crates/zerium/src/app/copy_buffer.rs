use std::rc::Rc;

use zerium_core::{
    animation::{AnimationTiming, SegmentInterpolation},
    timeline::EffectInstance,
};

/// Copies shared within this application, independently of the system clipboard.
#[derive(Default)]
pub(crate) struct CopyBuffer {
    pub(crate) effects: Option<Rc<Vec<EffectInstance>>>,
    pub(crate) curve: Option<SegmentInterpolation>,
    pub(crate) animation_timing: Option<Rc<AnimationTiming>>,
}

impl gpui::Global for CopyBuffer {}
