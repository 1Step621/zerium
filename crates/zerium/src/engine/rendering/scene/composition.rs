//! Resolve layer-relative capture and visibility rules before building GPU dependencies.

use zerium_core::{
    plugin::TextureInput,
    property::PropertyValues,
    timeline::{EffectInstanceId, ItemId, LayerId, RenderPlacement, RenderResultSettings},
};

pub(super) struct CaptureDefinition {
    item: ItemId,
    effect: Option<EffectInstanceId>,
    input: String,
    source_layer: LayerId,
    range: RenderResultSettings,
}

impl CaptureDefinition {
    pub(super) fn resolve(
        item: ItemId,
        effect: Option<EffectInstanceId>,
        source_layer: LayerId,
        input: &TextureInput,
        properties: &PropertyValues,
    ) -> Option<Self> {
        let TextureInput::RenderResult {
            id,
            start_offset,
            end_offset,
            hide_original,
            ..
        } = input
        else {
            return None;
        };
        let range = RenderResultSettings::from_properties(
            properties,
            start_offset,
            end_offset,
            hide_original,
        )
        .expect("render_result properties come from validated schemas");
        Some(Self {
            item,
            effect,
            input: id.clone(),
            source_layer,
            range,
        })
    }
}

pub(super) struct SceneCapturePlan {
    item: ItemId,
    effect: Option<EffectInstanceId>,
    input: String,
    nodes: Vec<usize>,
}

/// A scene's normal output and each capture are independent consumers of the
/// same nodes. Hiding an original never removes it from the dependency graph.
pub(super) struct SceneCompositionPlan {
    nodes: Vec<RenderPlacement>,
    normal_nodes: Vec<usize>,
    captures: Vec<SceneCapturePlan>,
}

impl SceneCompositionPlan {
    pub(super) fn new(nodes: Vec<RenderPlacement>, definitions: Vec<CaptureDefinition>) -> Self {
        let normal_nodes = nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| {
                !definitions.iter().any(|capture| {
                    capture.range.hide_original
                        && capture
                            .range
                            .includes(capture.source_layer, node.local_layer)
                })
            })
            .map(|(index, _)| index)
            .collect();

        let captures = definitions
            .iter()
            .map(|capture| {
                let captured = nodes
                    .iter()
                    .enumerate()
                    .filter(|(_, node)| {
                        capture
                            .range
                            .includes(capture.source_layer, node.local_layer)
                            && !definitions.iter().any(|nested| {
                                nested.source_layer != node.local_layer
                                    && capture
                                        .range
                                        .includes(capture.source_layer, nested.source_layer)
                                    && nested.range.hide_original
                                    && nested.range.includes(nested.source_layer, node.local_layer)
                            })
                    })
                    .map(|(index, _)| index)
                    .collect();
                SceneCapturePlan {
                    item: capture.item,
                    effect: capture.effect,
                    input: capture.input.clone(),
                    nodes: captured,
                }
            })
            .collect();
        Self {
            nodes,
            normal_nodes,
            captures,
        }
    }

    pub(super) fn normal_nodes(&self) -> impl Iterator<Item = &RenderPlacement> {
        self.normal_nodes.iter().map(|&index| &self.nodes[index])
    }

    pub(super) fn capture(
        &self,
        item: ItemId,
        effect: Option<EffectInstanceId>,
        input: &str,
    ) -> &SceneCapturePlan {
        self.captures
            .iter()
            .find(|capture| {
                capture.item == item && capture.effect == effect && capture.input == input
            })
            .expect("every render_result input has a resolved capture")
    }

    pub(super) fn captured_nodes<'a>(
        &'a self,
        capture: &'a SceneCapturePlan,
    ) -> impl Iterator<Item = &'a RenderPlacement> {
        capture.nodes.iter().map(|&index| &self.nodes[index])
    }
}
