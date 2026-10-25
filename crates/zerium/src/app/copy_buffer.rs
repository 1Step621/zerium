use std::rc::Rc;

use zerium_core::{
    animation::SegmentInterpolation,
    timeline::{EffectInstance, EffectInstanceId, TimelineEditor},
};

/// Copies shared within this application, independently of the system clipboard.
#[derive(Default)]
pub(crate) struct CopyBuffer {
    pub(crate) effects: Option<Rc<EffectCopy>>,
    pub(crate) curve: Option<SegmentInterpolation>,
}

impl gpui::Global for CopyBuffer {}

pub(crate) struct EffectCopy {
    pub(crate) effects: Vec<EffectInstance>,
    pub(crate) hidden: Vec<EffectInstanceId>,
}

impl EffectCopy {
    pub(crate) fn capture(editor: &TimelineEditor, effects: &[EffectInstance]) -> Self {
        Self {
            hidden: effects
                .iter()
                .filter(|effect| editor.is_effect_hidden(effect.id))
                .map(|effect| effect.id)
                .collect(),
            effects: effects.to_vec(),
        }
    }
}
