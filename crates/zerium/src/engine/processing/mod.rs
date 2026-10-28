//! Signal processing grouped by the signal being produced.
pub(crate) mod audio;
pub(crate) mod visual;

/// A failed scene calculation either reports an error or keeps its authored fallback.
#[derive(Clone, Copy)]
pub(crate) enum CalculationPolicy {
    Report,
    Fallback,
}
