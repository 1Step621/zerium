//! Common catalog metadata exposed by item and effect schemas.

pub(crate) trait PluginCatalogEntry {
    fn id(&self) -> &str;
    fn label(&self) -> &str;
    fn category(&self) -> &str;
    fn tags(&self) -> &[String];
}
