import package::generated::_internal::effect_state::_pass_state;

struct ZeriumTemporalInfo {
    sample_index: u32,
    sample_count: u32,
    frame_offset: f32,
    sample_progress: f32,
}

fn info() -> ZeriumTemporalInfo {
    return ZeriumTemporalInfo(
        _pass_state.sample_index,
        _pass_state.sample_count,
        _pass_state.frame_offset,
        _pass_state.sample_progress,
    );
}
