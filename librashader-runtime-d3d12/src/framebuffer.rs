use crate::descriptor_heap::{CpuStagingHeap, D3D12DescriptorHeap, RenderTargetHeap};
use crate::error::assume_d3d12_init;
use crate::texture::{D3D12OutputView, InputTexture};
use crate::util::d3d12_get_closest_format;
use crate::{error, util};
use librashader_common::{FilterMode, ImageFormat, Size, WrapMode};
use librashader_presets::Scale2D;
use librashader_runtime::scaling::{MipmapSize, ViewportSize};
use std::ops::Deref;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D12::{
    ID3D12Device, ID3D12GraphicsCommandList, ID3D12Resource, D3D12_BOX,
    D3D12_CPU_PAGE_PROPERTY_UNKNOWN, D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
    D3D12_FEATURE_DATA_FORMAT_SUPPORT, D3D12_FORMAT_SUPPORT1_MIP,
    D3D12_FORMAT_SUPPORT1_RENDER_TARGET, D3D12_FORMAT_SUPPORT1_SHADER_SAMPLE,
    D3D12_FORMAT_SUPPORT1_TEXTURE2D, D3D12_HEAP_FLAG_NONE, D3D12_HEAP_PROPERTIES,
    D3D12_HEAP_TYPE_DEFAULT, D3D12_MEMORY_POOL_UNKNOWN, D3D12_RENDER_TARGET_VIEW_DESC,
    D3D12_RENDER_TARGET_VIEW_DESC_0, D3D12_RESOURCE_DESC, D3D12_RESOURCE_DIMENSION_TEXTURE2D,
    D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET, D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
    D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_COPY_SOURCE,
    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_RENDER_TARGET,
    D3D12_RTV_DIMENSION_TEXTURE2D, D3D12_SHADER_RESOURCE_VIEW_DESC,
    D3D12_SHADER_RESOURCE_VIEW_DESC_0, D3D12_SRV_DIMENSION_TEXTURE2D, D3D12_TEX2D_RTV,
    D3D12_TEX2D_SRV, D3D12_TEXTURE_COPY_LOCATION, D3D12_TEXTURE_COPY_LOCATION_0,
    D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;

#[derive(Debug, Clone)]
pub(crate) struct OwnedImage {
    pub(crate) handle: ID3D12Resource,
    pub(crate) size: Size<u32>,
    pub(crate) format: ImageFormat,
    device: ID3D12Device,
    max_mipmap: u16,
}

static CLEAR: &[f32; 4] = &[0.0, 0.0, 0.0, 0.0];

impl OwnedImage {
    pub fn new(
        device: &ID3D12Device,
        size: Size<u32>,
        format: ImageFormat,
        mipmap: bool,
    ) -> error::Result<OwnedImage> {
        unsafe {
            let miplevels = if mipmap {
                size.calculate_miplevels()
            } else {
                1
            };
            let mut desc = D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
                Alignment: 0,
                Width: size.width as u64,
                Height: size.height,
                DepthOrArraySize: 1,
                MipLevels: miplevels as u16,
                Format: format.into(),
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Layout: Default::default(),
                Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
            };

            let mut format_support = D3D12_FEATURE_DATA_FORMAT_SUPPORT {
                Format: desc.Format,
                Support1: D3D12_FORMAT_SUPPORT1_TEXTURE2D
                    | D3D12_FORMAT_SUPPORT1_SHADER_SAMPLE
                    | D3D12_FORMAT_SUPPORT1_RENDER_TARGET,
                ..Default::default()
            };

            if mipmap {
                desc.Flags |= D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
                format_support.Support1 |= D3D12_FORMAT_SUPPORT1_MIP;
            }

            desc.Format = d3d12_get_closest_format(device, desc.Format, format_support);
            let mut resource: Option<ID3D12Resource> = None;
            unsafe {
                device.CreateCommittedResource(
                    &D3D12_HEAP_PROPERTIES {
                        Type: D3D12_HEAP_TYPE_DEFAULT,
                        CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
                        MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
                        CreationNodeMask: 1,
                        VisibleNodeMask: 1,
                    },
                    D3D12_HEAP_FLAG_NONE,
                    &desc,
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                    None,
                    &mut resource,
                )?;
            }
            assume_d3d12_init!(resource, "CreateCommittedResource");

            Ok(OwnedImage {
                handle: resource,
                size,
                format,
                device: device.clone(),
                max_mipmap: miplevels as u16,
            })
        }
    }

    /// SAFETY: self must fit the source image
    /// source must be in D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE
    pub unsafe fn copy_from(
        &self,
        cmd: &ID3D12GraphicsCommandList,
        input: &InputTexture,
    ) -> error::Result<()> {
        util::d3d12_resource_transition(
            cmd,
            &input.resource,
            D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
            D3D12_RESOURCE_STATE_COPY_SOURCE,
        );

        util::d3d12_resource_transition(
            cmd,
            &self.handle,
            D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
            D3D12_RESOURCE_STATE_COPY_DEST,
        );
        unsafe {
            cmd.CopyTextureRegion(
                &D3D12_TEXTURE_COPY_LOCATION {
                    pResource: windows::core::ManuallyDrop::new(&self.handle),
                    Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                        SubresourceIndex: 0,
                    },
                },
                0,
                0,
                0,
                &D3D12_TEXTURE_COPY_LOCATION {
                    pResource: windows::core::ManuallyDrop::new(&input.resource),
                    Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                        SubresourceIndex: 0,
                    },
                },
                Some(&D3D12_BOX {
                    left: 0,
                    top: 0,
                    front: 0,
                    right: input.size.width,
                    bottom: input.size.height,
                    back: 1,
                }),
            );
        }

        util::d3d12_resource_transition(
            cmd,
            &input.resource,
            D3D12_RESOURCE_STATE_COPY_SOURCE,
            D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
        );
        util::d3d12_resource_transition(
            cmd,
            &self.handle,
            D3D12_RESOURCE_STATE_COPY_DEST,
            D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
        );

        Ok(())
    }

    pub fn clear(
        &self,
        cmd: &ID3D12GraphicsCommandList,
        heap: &mut D3D12DescriptorHeap<RenderTargetHeap>,
    ) -> error::Result<()> {
        util::d3d12_resource_transition(
            cmd,
            &self.handle,
            D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
            D3D12_RESOURCE_STATE_RENDER_TARGET,
        );

        let rtv = self.create_render_target_view(heap)?;

        let rect = RECT {
            left: 0,
            top: 0,
            right: self.size.width as i32,
            bottom: self.size.height as i32,
        };

        unsafe {
            // more efficient if we don't pass the rect but
            cmd.ClearRenderTargetView(*rtv.descriptor.as_ref(), CLEAR.as_ptr(), &[rect])
        }

        util::d3d12_resource_transition(
            cmd,
            &self.handle,
            D3D12_RESOURCE_STATE_RENDER_TARGET,
            D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
        );

        Ok(())
    }

    pub(crate) fn create_shader_resource_view(
        &self,
        heap: &mut D3D12DescriptorHeap<CpuStagingHeap>,
        filter: FilterMode,
        wrap_mode: WrapMode,
    ) -> error::Result<InputTexture> {
        let descriptor = heap.alloc_slot()?;

        unsafe {
            let srv_desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
                Format: self.format.into(),
                ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
                Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
                Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                    Texture2D: D3D12_TEX2D_SRV {
                        MipLevels: u32::MAX,
                        ..Default::default()
                    },
                },
            };

            self.device.CreateShaderResourceView(
                &self.handle,
                Some(&srv_desc),
                *descriptor.deref().as_ref(),
            );
        }

        Ok(InputTexture::new(
            self.handle.clone(),
            descriptor,
            self.size,
            self.format,
            filter,
            wrap_mode,
        ))
    }

    pub(crate) fn create_render_target_view(
        &self,
        heap: &mut D3D12DescriptorHeap<RenderTargetHeap>,
    ) -> error::Result<D3D12OutputView> {
        let descriptor = heap.alloc_slot()?;

        unsafe {
            let rtv_desc = D3D12_RENDER_TARGET_VIEW_DESC {
                Format: self.format.into(),
                ViewDimension: D3D12_RTV_DIMENSION_TEXTURE2D,
                Anonymous: D3D12_RENDER_TARGET_VIEW_DESC_0 {
                    Texture2D: D3D12_TEX2D_RTV {
                        MipSlice: 0,
                        ..Default::default()
                    },
                },
            };

            self.device.CreateRenderTargetView(
                &self.handle,
                Some(&rtv_desc),
                *descriptor.deref().as_ref(),
            );
        }

        Ok(D3D12OutputView::new(descriptor, self.size))
    }

    pub fn scale(
        &mut self,
        scaling: Scale2D,
        format: ImageFormat,
        viewport_size: &Size<u32>,
        source_size: &Size<u32>,
        mipmap: bool,
    ) -> error::Result<Size<u32>> {
        let size = source_size.scale_viewport(scaling, *viewport_size);
        if self.size != size
            || (mipmap && self.max_mipmap == 1)
            || (!mipmap && self.max_mipmap != 1)
            || format != self.format
        {
            let mut new = OwnedImage::new(&self.device, size, format, mipmap)?;

            std::mem::swap(self, &mut new);
        }
        Ok(size)
    }
}