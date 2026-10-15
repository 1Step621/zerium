use std::rc::Rc;

use zerium_core::{
    animation::SegmentInterpolation,
    media::MediaMetadataCache,
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
    pub(crate) media_cache: MediaMetadataCache,
}

impl EffectCopy {
    pub(crate) fn capture(editor: &TimelineEditor, effects: &[EffectInstance]) -> Self {
        Self {
            hidden: effects
                .iter()
                .filter(|effect| editor.is_effect_hidden(effect.id))
                .map(|effect| effect.id)
                .collect(),
            media_cache: editor.media_cache().retained_paths(
                effects
                    .iter()
                    .flat_map(|effect| effect.properties.files().map(|(_, path)| path)),
            ),
            effects: effects.to_vec(),
        }
    }
}
