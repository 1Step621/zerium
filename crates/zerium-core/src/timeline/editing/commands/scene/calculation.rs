use crate::{
    script::Script,
    timeline::{
        SceneCalculation, SceneCalculationInput, SceneVariablePreset, TimelineEditor,
        editing::history::{EditImpact, HistoryKey},
    },
};

impl TimelineEditor {
    pub fn create_scene_calculation(
        &mut self,
        preset: SceneVariablePreset,
        label: impl FnMut(u64) -> String,
    ) -> Option<String> {
        if preset == SceneVariablePreset::File {
            return None;
        }
        let source = match preset {
            SceneVariablePreset::Number => "0.0",
            SceneVariablePreset::SignedInteger | SceneVariablePreset::UnsignedInteger => "0",
            SceneVariablePreset::Boolean => "false",
            SceneVariablePreset::Text => "\"\"",
            SceneVariablePreset::Color => "rgba(0.0, 0.0, 0.0, 1.0)",
            _ => unreachable!("calculation presets were checked"),
        };
        self.create_scene_variable(
            preset,
            label,
            Some(SceneCalculation {
                inputs: Vec::new(),
                script: Script::new(source),
            }),
        )
    }

    pub fn update_scene_calculation_script(&mut self, id: &str, source: String) -> bool {
        let scene_id = self.active_scene_id();
        self.edit_project_if_changed(
            EditImpact::Values,
            Some(HistoryKey::SceneCalculationCode(scene_id, id.to_owned())),
            |editor| {
                let Some(calculation) = editor
                    .project_mut()
                    .scenes_mut()
                    .get_mut(&scene_id)
                    .and_then(|scene| scene.variable_mut(id))
                    .and_then(|variable| variable.calculation_mut())
                else {
                    return false;
                };
                if calculation.script.source() == source {
                    return false;
                }
                calculation.script = Script::new(source);
                true
            },
        )
    }

    pub fn update_scene_calculation_inputs(
        &mut self,
        id: &str,
        inputs: Vec<SceneCalculationInput>,
    ) -> Result<bool, String> {
        let scene_id = self.active_scene_id();
        let mut candidate = self.scene(scene_id).ok_or("Scene not found")?.clone();
        let calculation = candidate
            .variable_mut(id)
            .and_then(|variable| variable.calculation_mut())
            .ok_or("Calculation not found")?;
        if calculation.inputs == inputs {
            return Ok(false);
        }
        calculation.inputs = inputs;
        candidate.validate_calculations()?;
        Ok(self.edit_project_if_changed(
            EditImpact::Values,
            Some(HistoryKey::SceneCalculationInputs(scene_id, id.to_owned())),
            |editor| {
                editor
                    .project_mut()
                    .scenes_mut()
                    .insert(scene_id, candidate);
                true
            },
        ))
    }
}
