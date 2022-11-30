use librashader_reflect::reflect::semantics::TextureBinding;
use crate::gl::BindTexture;
use crate::samplers::SamplerSet;
use crate::texture::Texture;

pub struct Gl46BindTexture;

impl BindTexture for Gl46BindTexture {
    fn bind_texture(samplers: &SamplerSet, binding: &TextureBinding, texture: &Texture) {
        unsafe {
            // eprintln!("setting {} to texunit {}", texture.image.handle, binding.binding);
            gl::BindTextureUnit(binding.binding, texture.image.handle);
            gl::BindSampler(binding.binding,
                            samplers.get(texture.wrap_mode, texture.filter, texture.mip_filter));
        }
    }
}