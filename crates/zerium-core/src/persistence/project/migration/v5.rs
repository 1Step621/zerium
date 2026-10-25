//! Version 5 → 6: store main as a scene with the same variable and item contract.
use crate::persistence::ProjectError;
use serde_json::{Map, Value, json};

pub(super) fn migrate(file: &mut Map<String, Value>) -> Result<(), ProjectError> {
    let items = file
        .remove("items")
        .ok_or_else(|| ProjectError::invalid_data("Project items are missing"))?;
    let scenes = file
        .get_mut("scenes")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| ProjectError::invalid_data("Project scenes are missing"))?;
    // Earlier formats reserve zero, so it cannot collide with a reusable scene.
    let main_scene = 0;
    if scenes
        .iter()
        .any(|scene| scene.get("id").and_then(Value::as_u64) == Some(main_scene))
    {
        return Err(ProjectError::invalid_data("Invalid legacy scene ID"));
    }
    let name = std::iter::once("Main".to_owned())
        .chain((2_u64..).map(|index| format!("Main {index}")))
        .find(|name| {
            scenes
                .iter()
                .all(|scene| scene.get("name").and_then(Value::as_str) != Some(name))
        })
        .expect("a scene name is available");
    scenes.push(json!({
        "id": main_scene,
        "name": name,
        "stretch_point": 0,
        "variables": [],
        "items": items,
    }));
    file.insert("main_scene".into(), Value::from(main_scene));
    file.insert("format_version".into(), Value::from(6));
    Ok(())
}
