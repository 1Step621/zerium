gpui::actions!(
    zerium,
    [
        DeleteSelectedItem,
        CopySelectedItems,
        CutSelectedItems,
        PasteItems,
        SplitSelectedItems,
        StampTextPlacement,
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
        ImportScene,
        SaveSceneFile,
        OpenProjectSettings,
        OpenSettings,
        OpenExportDialog,
        OpenLogFolder,
        OpenAboutDialog,
        OpenItemPicker,
        OpenEffectPicker,
        HidePreviewOverlay
    ]
);

#[derive(Clone, gpui::Action, PartialEq, Eq, serde::Deserialize)]
#[action(namespace = zerium, no_json)]
pub(crate) struct OpenTextPlacement {
    pub(crate) layer: u64,
}
