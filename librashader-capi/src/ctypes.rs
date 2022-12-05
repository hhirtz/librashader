use std::ptr::NonNull;
use librashader::presets::ShaderPreset;
use crate::error::LibrashaderError;

pub type libra_shader_preset_t = Option<NonNull<ShaderPreset>>;
pub type libra_error_t = Option<NonNull<LibrashaderError>>;

#[cfg(feature = "runtime-opengl")]
pub type libra_gl_filter_chain_t = Option<NonNull<librashader::runtime::gl::FilterChainGL>>;

/// Parameters for the output viewport.
#[repr(C)]
pub struct libra_viewport_t {
    pub x: f32,
    pub y: f32,
    pub width: u32,
    pub height: u32,
}