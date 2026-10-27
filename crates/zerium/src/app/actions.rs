gpui::actions!(
    zerium,
    [
        DeleteSelectedItem,
        CopySelectedItems,
        CutSelectedItems,
        PasteItems,
        SplitSelectedItems,
        StampPlacement,
        TogglePlayback,
        TogglePlaybackInPlace,
        PreviousFrame,
        NextFrame,
        Undo,
        Redo,
        NewProject,
        OpenProject,
        SaveProject,
        SaveProjectAs,
        SaveSceneFile,
        OpenProjectSettings,
        OpenSettings,
        OpenSceneLibrary,
        OpenExportDialog,
        OpenLogFolder,
        OpenAboutDialog,
        OpenItemPicker,
        OpenEffectPicker,
        HidePreviewOverlay
    ]
);

#[derive(Clone, Default, gpui::Action, PartialEq, Eq, serde::Deserialize)]
#[action(namespace = zerium, no_json)]
pub(crate) struct ImportScene {
    pub(crate) path: Option<std::path::PathBuf>,
}

#[derive(Clone, gpui::Action, PartialEq, Eq, serde::Deserialize)]
#[action(namespace = zerium, no_json)]
pub(crate) struct OpenPlacement {
    pub(crate) kind: PlacementKind,
    pub(crate) layer: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, serde::Deserialize)]
pub(crate) enum PlacementKind {
    Text,
    Audio,
}
