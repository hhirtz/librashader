use crate::ctypes::{
    config_struct, libra_d3d12_filter_chain_t, libra_shader_preset_t, libra_viewport_t, FromUninit,
};
use crate::error::{assert_non_null, assert_some_ptr, LibrashaderError};
use crate::ffi::extern_fn;
use std::ffi::c_char;
use std::ffi::CStr;
use std::mem::{ManuallyDrop, MaybeUninit};
use std::ptr::NonNull;
use std::slice;
use windows::Win32::Graphics::Direct3D12::{
    ID3D12Device, ID3D12GraphicsCommandList, ID3D12Resource, D3D12_CPU_DESCRIPTOR_HANDLE,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT;

use librashader::runtime::d3d12::capi::options::FilterChainOptionsD3D12;
use librashader::runtime::d3d12::capi::options::FrameOptionsD3D12;

use crate::LIBRASHADER_API_VERSION;
use librashader::runtime::d3d12::{D3D12InputImage, D3D12OutputView};
use librashader::runtime::{FilterChainParameters, Size, Viewport};

/// Direct3D 12 parameters for the source image.
#[repr(C)]
pub struct libra_source_image_d3d12_t {
    /// The resource containing the image.
    pub resource: ManuallyDrop<ID3D12Resource>,
    /// A CPU descriptor handle to a shader resource view of the image.
    pub descriptor: D3D12_CPU_DESCRIPTOR_HANDLE,
    /// The format of the image.
    pub format: DXGI_FORMAT,
    /// The width of the source image.
    pub width: u32,
    /// The height of the source image.
    pub height: u32,
}

/// Direct3D 12 parameters for the output image.
#[repr(C)]
pub struct libra_output_image_d3d12_t {
    /// A CPU descriptor handle to a shader resource view of the image.
    pub descriptor: D3D12_CPU_DESCRIPTOR_HANDLE,
    /// The format of the image.
    pub format: DXGI_FORMAT,
}

/// Options for each Direct3D 12 shader frame.
#[repr(C)]
#[derive(Default, Debug, Clone)]
pub struct frame_d3d12_opt_t {
    /// The librashader API version.
    pub version: LIBRASHADER_API_VERSION,
    /// Whether or not to clear the history buffers.
    pub clear_history: bool,
    /// The direction of rendering.
    /// -1 indicates that the frames are played in reverse order.
    pub frame_direction: i32,
}

config_struct! {
    impl FrameOptionsD3D12 => frame_d3d12_opt_t {
        0 => [clear_history, frame_direction];
    }
}

/// Options for Direct3D11 filter chain creation.
#[repr(C)]
#[derive(Default, Debug, Clone)]
pub struct filter_chain_d3d12_opt_t {
    /// The librashader API version.
    pub version: LIBRASHADER_API_VERSION,

    /// Force the HLSL shader pipeline. This may reduce shader compatibility.
    pub force_hlsl_pipeline: bool,

    /// Whether or not to explicitly disable mipmap
    /// generation for intermediate passes regardless
    /// of shader preset settings.
    pub force_no_mipmaps: bool,
}

config_struct! {
    impl FilterChainOptionsD3D12 => filter_chain_d3d12_opt_t {
        0 =>  [force_hlsl_pipeline, force_no_mipmaps];
    }
}

impl TryFrom<libra_source_image_d3d12_t> for D3D12InputImage {
    type Error = LibrashaderError;

    fn try_from(value: libra_source_image_d3d12_t) -> Result<Self, Self::Error> {
        let resource = value.resource.clone();

        Ok(D3D12InputImage {
            resource: ManuallyDrop::into_inner(resource),
            descriptor: value.descriptor,
            size: Size::new(value.width, value.height),
            format: value.format,
        })
    }
}

extern_fn! {
    /// Create the filter chain given the shader preset.
    ///
    /// The shader preset is immediately invalidated and must be recreated after
    /// the filter chain is created.
    ///
    /// ## Safety:
    /// - `preset` must be either null, or valid and aligned.
    /// - `options` must be either null, or valid and aligned.
    /// - `device` must not be null.
    /// - `out` must be aligned, but may be null, invalid, or uninitialized.
    fn libra_d3d12_filter_chain_create(
        preset: *mut libra_shader_preset_t,
        opt: *const MaybeUninit<filter_chain_d3d12_opt_t>,
        device: ManuallyDrop<ID3D12Device>,
        out: *mut MaybeUninit<libra_d3d12_filter_chain_t>
    ) {
        assert_non_null!(preset);
        let preset = unsafe {
            let preset_ptr = &mut *preset;
            let preset = preset_ptr.take();
            Box::from_raw(preset.unwrap().as_ptr())
        };

        let opt = if opt.is_null() {
            None
        } else {
            Some(unsafe { opt.read() })
        };

        let opt = opt.map(FromUninit::from_uninit);
        let chain = librashader::runtime::d3d12::capi::FilterChainD3D12::load_from_preset(
            &device,
            *preset,
            opt.as_ref(),
        )?;

        unsafe {
            out.write(MaybeUninit::new(NonNull::new(Box::into_raw(Box::new(
                chain,
            )))))
        }
    }
}

extern_fn! {
    /// Records rendering commands for a frame with the given parameters for the given filter chain
    /// to the input command list.
    ///
    /// * The input image must be in the `D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE` resource state.
    /// * The output image must be in `D3D12_RESOURCE_STATE_RENDER_TARGET` resource state.
    ///
    /// librashader **will not** create a resource barrier for the final pass. The output image will
    /// remain in `D3D12_RESOURCE_STATE_RENDER_TARGET` after all shader passes. The caller must transition
    /// the output image to the final resource state.
    ///
    /// ## Safety
    /// - `chain` may be null, invalid, but not uninitialized. If `chain` is null or invalid, this
    ///    function will return an error.
    /// - `mvp` may be null, or if it is not null, must be an aligned pointer to 16 consecutive `float`
    ///    values for the model view projection matrix.
    /// - `opt` may be null, or if it is not null, must be an aligned pointer to a valid `frame_gl_opt_t`
    ///    struct.
    /// - `out` must be a descriptor handle to a render target view.
    /// - `image.resource` must not be null.
    /// - `command_list` must be a non-null pointer to a `ID3D12GraphicsCommandList` that is open,
    ///    and must be associated with the `ID3D12Device` this filter chain was created with.
    /// - You must ensure that only one thread has access to `chain` before you call this function. Only one
    ///   thread at a time may call this function.
    fn libra_d3d12_filter_chain_frame(
        chain: *mut libra_d3d12_filter_chain_t,
        command_list: ManuallyDrop<ID3D12GraphicsCommandList>,
        frame_count: usize,
        image: libra_source_image_d3d12_t,
        viewport: libra_viewport_t,
        out: libra_output_image_d3d12_t,
        mvp: *const f32,
        options: *const MaybeUninit<frame_d3d12_opt_t>
    ) mut |chain| {
        assert_some_ptr!(mut chain);

        let mvp = if mvp.is_null() {
            None
        } else {
            Some(<&[f32; 16]>::try_from(unsafe { slice::from_raw_parts(mvp, 16) }).unwrap())
        };

        let options = if options.is_null() {
            None
        } else {
            Some(unsafe { options.read() })
        };

        let options = options.map(FromUninit::from_uninit);
        let viewport = Viewport {
            x: viewport.x,
            y: viewport.y,
            output: unsafe { D3D12OutputView::new_from_raw(out.descriptor, Size::new(viewport.width, viewport.height), out.format) },
            mvp,
        };

        let image = image.try_into()?;
        chain.frame(&command_list, image, &viewport, frame_count, options.as_ref())?;
    }
}

extern_fn! {
    /// Sets a parameter for the filter chain.
    ///
    /// If the parameter does not exist, returns an error.
    /// ## Safety
    /// - `chain` must be either null or a valid and aligned pointer to an initialized `libra_d3d12_filter_chain_t`.
    /// - `param_name` must be either null or a null terminated string.
    fn libra_d3d12_filter_chain_set_param(
        chain: *mut libra_d3d12_filter_chain_t,
        param_name: *const c_char,
        value: f32
    ) mut |chain| {
        assert_some_ptr!(mut chain);
        assert_non_null!(param_name);
        unsafe {
            let name = CStr::from_ptr(param_name);
            let name = name.to_str()?;

            if chain.set_parameter(name, value).is_none() {
                return LibrashaderError::UnknownShaderParameter(param_name).export()
            }
        }
    }
}

extern_fn! {
    /// Gets a parameter for the filter chain.
    ///
    /// If the parameter does not exist, returns an error.
    /// ## Safety
    /// - `chain` must be either null or a valid and aligned pointer to an initialized `libra_d3d12_filter_chain_t`.
    /// - `param_name` must be either null or a null terminated string.
    fn libra_d3d12_filter_chain_get_param(
        chain: *mut libra_d3d12_filter_chain_t,
        param_name: *const c_char,
        out: *mut MaybeUninit<f32>
    ) mut |chain| {
        assert_some_ptr!(mut chain);
        assert_non_null!(param_name);
        unsafe {
            let name = CStr::from_ptr(param_name);
            let name = name.to_str()?;

            let Some(value) = chain.get_parameter(name) else {
                return LibrashaderError::UnknownShaderParameter(param_name).export()
            };

            out.write(MaybeUninit::new(value));
        }
    }
}

extern_fn! {
    /// Sets the number of active passes for this chain.
    ///
    /// ## Safety
    /// - `chain` must be either null or a valid and aligned pointer to an initialized `libra_d3d12_filter_chain_t`.
    fn libra_d3d12_filter_chain_set_active_pass_count(
        chain: *mut libra_d3d12_filter_chain_t,
        value: u32
    ) mut |chain| {
        assert_some_ptr!(mut chain);
        chain.set_enabled_pass_count(value as usize);
    }
}

extern_fn! {
    /// Gets the number of active passes for this chain.
    ///
    /// ## Safety
    /// - `chain` must be either null or a valid and aligned pointer to an initialized `libra_d3d12_filter_chain_t`.
    fn libra_d3d12_filter_chain_get_active_pass_count(
        chain: *mut libra_d3d12_filter_chain_t,
        out: *mut MaybeUninit<u32>
    ) mut |chain| {
        assert_some_ptr!(mut chain);
        unsafe {
            let value = chain.get_enabled_pass_count();
            out.write(MaybeUninit::new(value as u32))
        }
    }
}

extern_fn! {
    /// Free a D3D12 filter chain.
    ///
    /// The resulting value in `chain` then becomes null.
    /// ## Safety
    /// - `chain` must be either null or a valid and aligned pointer to an initialized `libra_d3d12_filter_chain_t`.
    fn libra_d3d12_filter_chain_free(chain: *mut libra_d3d12_filter_chain_t) {
        assert_non_null!(chain);
        unsafe {
            let chain_ptr = &mut *chain;
            let chain = chain_ptr.take();
            drop(Box::from_raw(chain.unwrap().as_ptr()))
        };
    }
}
