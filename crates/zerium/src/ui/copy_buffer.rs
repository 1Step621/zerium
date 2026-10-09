use std::rc::Rc;

use zerium_core::{
    animation::SegmentInterpolation,
    media::MediaMetadataCache,
    timeline::{EffectInstance, EffectInstanceId, TimelineEditor},
};

/// Copies shared within this application, independently of the system clipboard.
#[derive(Default)]
pub(super) struct CopyBuffer {
    pub effects: Option<Rc<EffectCopy>>,
    pub curve: Option<SegmentInterpolation>,
}

impl gpui::Global for CopyBuffer {}

pub(super) struct EffectCopy {
    pub effects: Vec<EffectInstance>,
    pub hidden: Vec<EffectInstanceId>,
    pub media_cache: MediaMetadataCache,
}

impl EffectCopy {
    pub(super) fn capture(editor: &TimelineEditor, effects: &[EffectInstance]) -> Self {
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
