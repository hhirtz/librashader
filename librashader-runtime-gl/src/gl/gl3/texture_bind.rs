use librashader_reflect::reflect::semantics::TextureBinding;
use crate::gl::BindTexture;
use crate::samplers::SamplerSet;
use crate::texture::Texture;

pub struct Gl3BindTexture;

impl BindTexture for Gl3BindTexture {
    fn bind_texture(samplers: &SamplerSet, binding: &TextureBinding, texture: &Texture) {
        unsafe {
            // eprintln!("setting {} to texunit {}", texture.image.handle, binding.binding);
            gl::ActiveTexture(gl::TEXTURE0 + binding.binding);

            gl::BindTexture(gl::TEXTURE_2D, texture.image.handle);
            gl::BindSampler(binding.binding,
                            samplers.get(texture.wrap_mode, texture.filter, texture.mip_filter));
        }
    }
}