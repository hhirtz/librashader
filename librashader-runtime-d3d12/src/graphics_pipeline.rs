use crate::error::assume_d3d12_init;
use crate::error::FilterChainError::Direct3DOperationError;
use crate::quad_render::DrawQuad;
use crate::{error, util};
use librashader_reflect::back::cross::CrossHlslContext;
use librashader_reflect::back::dxil::DxilObject;
use librashader_reflect::back::ShaderCompilerOutput;
use librashader_reflect::reflect::semantics::BindingStage;
use windows::Win32::Foundation::BOOL;
use windows::Win32::Graphics::Direct3D::Dxc::{IDxcBlob, IDxcCompiler, IDxcUtils, IDxcValidator};
use windows::Win32::Graphics::Direct3D12::{
    D3D12SerializeVersionedRootSignature, ID3D12Device, ID3D12PipelineState, ID3D12RootSignature,
    D3D12_BLEND_DESC, D3D12_BLEND_INV_SRC_ALPHA, D3D12_BLEND_OP_ADD, D3D12_BLEND_SRC_ALPHA,
    D3D12_COLOR_WRITE_ENABLE_ALL, D3D12_CULL_MODE_NONE, D3D12_DESCRIPTOR_RANGE1,
    D3D12_DESCRIPTOR_RANGE_FLAGS, D3D12_DESCRIPTOR_RANGE_FLAG_DATA_VOLATILE,
    D3D12_DESCRIPTOR_RANGE_FLAG_DESCRIPTORS_VOLATILE, D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER,
    D3D12_DESCRIPTOR_RANGE_TYPE_SRV, D3D12_FILL_MODE_SOLID, D3D12_GRAPHICS_PIPELINE_STATE_DESC,
    D3D12_INPUT_LAYOUT_DESC, D3D12_LOGIC_OP_NOOP, D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
    D3D12_RASTERIZER_DESC, D3D12_RENDER_TARGET_BLEND_DESC, D3D12_ROOT_DESCRIPTOR1,
    D3D12_ROOT_DESCRIPTOR_FLAG_NONE, D3D12_ROOT_DESCRIPTOR_TABLE1, D3D12_ROOT_PARAMETER1,
    D3D12_ROOT_PARAMETER1_0, D3D12_ROOT_PARAMETER_TYPE_CBV,
    D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE, D3D12_ROOT_SIGNATURE_DESC1,
    D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT, D3D12_SHADER_BYTECODE,
    D3D12_SHADER_VISIBILITY_ALL, D3D12_SHADER_VISIBILITY_PIXEL,
    D3D12_VERSIONED_ROOT_SIGNATURE_DESC, D3D12_VERSIONED_ROOT_SIGNATURE_DESC_0,
    D3D_ROOT_SIGNATURE_VERSION_1_1,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};

pub struct D3D12GraphicsPipeline {
    pub(crate) handle: ID3D12PipelineState,
}

const D3D12_SLANG_ROOT_PARAMETERS: &[D3D12_ROOT_PARAMETER1; 4] = &[
    // srvs
    D3D12_ROOT_PARAMETER1 {
        ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
        Anonymous: D3D12_ROOT_PARAMETER1_0 {
            DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE1 {
                NumDescriptorRanges: 1,
                pDescriptorRanges: &D3D12_DESCRIPTOR_RANGE1 {
                    RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
                    NumDescriptors: 16,
                    BaseShaderRegister: 0,
                    RegisterSpace: 0,
                    Flags: D3D12_DESCRIPTOR_RANGE_FLAGS(
                        D3D12_DESCRIPTOR_RANGE_FLAG_DATA_VOLATILE.0
                            | D3D12_DESCRIPTOR_RANGE_FLAG_DESCRIPTORS_VOLATILE.0,
                    ),
                    OffsetInDescriptorsFromTableStart: 0,
                },
            },
        },
        ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
    },
    // samplers
    D3D12_ROOT_PARAMETER1 {
        ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
        Anonymous: D3D12_ROOT_PARAMETER1_0 {
            DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE1 {
                NumDescriptorRanges: 1,
                pDescriptorRanges: &D3D12_DESCRIPTOR_RANGE1 {
                    RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER,
                    NumDescriptors: 16,
                    BaseShaderRegister: 0,
                    RegisterSpace: 0,
                    Flags: D3D12_DESCRIPTOR_RANGE_FLAG_DESCRIPTORS_VOLATILE,
                    OffsetInDescriptorsFromTableStart: 0,
                },
            },
        },
        ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
    },
    // UBO
    D3D12_ROOT_PARAMETER1 {
        ParameterType: D3D12_ROOT_PARAMETER_TYPE_CBV,
        Anonymous: D3D12_ROOT_PARAMETER1_0 {
            Descriptor: D3D12_ROOT_DESCRIPTOR1 {
                ShaderRegister: 0,
                RegisterSpace: 0,
                Flags: D3D12_ROOT_DESCRIPTOR_FLAG_NONE,
            },
        },
        ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
    },
    // push
    D3D12_ROOT_PARAMETER1 {
        ParameterType: D3D12_ROOT_PARAMETER_TYPE_CBV,
        Anonymous: D3D12_ROOT_PARAMETER1_0 {
            Descriptor: D3D12_ROOT_DESCRIPTOR1 {
                ShaderRegister: 1,
                RegisterSpace: 0,
                Flags: D3D12_ROOT_DESCRIPTOR_FLAG_NONE,
            },
        },
        ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
    },
];

const D3D12_SLANG_VERSIONED_ROOT_SIGNATURE: &D3D12_VERSIONED_ROOT_SIGNATURE_DESC =
    &D3D12_VERSIONED_ROOT_SIGNATURE_DESC {
        Version: D3D_ROOT_SIGNATURE_VERSION_1_1,
        Anonymous: D3D12_VERSIONED_ROOT_SIGNATURE_DESC_0 {
            Desc_1_1: D3D12_ROOT_SIGNATURE_DESC1 {
                NumParameters: D3D12_SLANG_ROOT_PARAMETERS.len() as u32,
                pParameters: D3D12_SLANG_ROOT_PARAMETERS.as_ptr(),
                NumStaticSamplers: 0,
                pStaticSamplers: std::ptr::null(),
                Flags: D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
            },
        },
    };

pub struct D3D12RootSignature {
    pub(crate) handle: ID3D12RootSignature,
}

impl D3D12RootSignature {
    pub fn new(device: &ID3D12Device) -> error::Result<D3D12RootSignature> {
        let signature = unsafe {
            let mut rs_blob = None;

            D3D12SerializeVersionedRootSignature(
                D3D12_SLANG_VERSIONED_ROOT_SIGNATURE,
                &mut rs_blob,
                None,
            )?;

            assume_d3d12_init!(rs_blob, "D3D12SerializeVersionedRootSignature");
            let blob = std::slice::from_raw_parts(
                rs_blob.GetBufferPointer().cast(),
                rs_blob.GetBufferSize(),
            );
            let root_signature: ID3D12RootSignature = device.CreateRootSignature(0, blob)?;
            root_signature
        };

        Ok(D3D12RootSignature { handle: signature })
    }
}
impl D3D12GraphicsPipeline {
    fn new_from_blobs(
        device: &ID3D12Device,
        vertex_dxil: IDxcBlob,
        fragment_dxil: IDxcBlob,
        root_signature: &D3D12RootSignature,
        render_format: DXGI_FORMAT,
    ) -> error::Result<D3D12GraphicsPipeline> {
        let input_element = DrawQuad::get_spirv_cross_vbo_desc();

        let pipeline_state: ID3D12PipelineState = unsafe {
            let pipeline_desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
                pRootSignature: windows::core::ManuallyDrop::new(&root_signature.handle),
                VS: D3D12_SHADER_BYTECODE {
                    pShaderBytecode: vertex_dxil.GetBufferPointer(),
                    BytecodeLength: vertex_dxil.GetBufferSize(),
                },
                PS: D3D12_SHADER_BYTECODE {
                    pShaderBytecode: fragment_dxil.GetBufferPointer(),
                    BytecodeLength: fragment_dxil.GetBufferSize(),
                },
                StreamOutput: Default::default(),
                BlendState: D3D12_BLEND_DESC {
                    RenderTarget: [
                        D3D12_RENDER_TARGET_BLEND_DESC {
                            BlendEnable: BOOL::from(false),
                            LogicOpEnable: BOOL::from(false),
                            SrcBlend: D3D12_BLEND_SRC_ALPHA,
                            DestBlend: D3D12_BLEND_INV_SRC_ALPHA,
                            BlendOp: D3D12_BLEND_OP_ADD,
                            SrcBlendAlpha: D3D12_BLEND_SRC_ALPHA,
                            DestBlendAlpha: D3D12_BLEND_INV_SRC_ALPHA,
                            BlendOpAlpha: D3D12_BLEND_OP_ADD,
                            LogicOp: D3D12_LOGIC_OP_NOOP,
                            RenderTargetWriteMask: D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8,
                        },
                        Default::default(),
                        Default::default(),
                        Default::default(),
                        Default::default(),
                        Default::default(),
                        Default::default(),
                        Default::default(),
                    ],
                    ..Default::default()
                },
                SampleMask: u32::MAX,
                RasterizerState: D3D12_RASTERIZER_DESC {
                    FillMode: D3D12_FILL_MODE_SOLID,
                    CullMode: D3D12_CULL_MODE_NONE,
                    ..Default::default()
                },
                DepthStencilState: Default::default(),
                InputLayout: D3D12_INPUT_LAYOUT_DESC {
                    pInputElementDescs: input_element.as_ptr(),
                    NumElements: input_element.len() as u32,
                },
                PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
                NumRenderTargets: 1,
                RTVFormats: [
                    render_format,
                    DXGI_FORMAT_UNKNOWN,
                    DXGI_FORMAT_UNKNOWN,
                    DXGI_FORMAT_UNKNOWN,
                    DXGI_FORMAT_UNKNOWN,
                    DXGI_FORMAT_UNKNOWN,
                    DXGI_FORMAT_UNKNOWN,
                    DXGI_FORMAT_UNKNOWN,
                ],
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                NodeMask: 0,
                ..Default::default()
            };

            device.CreateGraphicsPipelineState(&pipeline_desc)?
        };

        Ok(D3D12GraphicsPipeline {
            handle: pipeline_state,
        })
    }

    pub fn new_from_dxil(
        device: &ID3D12Device,
        library: &IDxcUtils,
        validator: &IDxcValidator,
        shader_assembly: &ShaderCompilerOutput<DxilObject, ()>,
        root_signature: &D3D12RootSignature,
        render_format: DXGI_FORMAT,
    ) -> error::Result<D3D12GraphicsPipeline> {
        if shader_assembly.vertex.requires_runtime_data() {
            return Err(Direct3DOperationError(
                "Compiled DXIL Vertex shader needs unexpected runtime data",
            ));
        }
        if shader_assembly.fragment.requires_runtime_data() {
            return Err(Direct3DOperationError(
                "Compiled DXIL fragment shader needs unexpected runtime data",
            ));
        }
        let vertex_dxil = util::dxc_validate_shader(library, validator, &shader_assembly.vertex)?;
        let fragment_dxil =
            util::dxc_validate_shader(library, validator, &shader_assembly.fragment)?;

        Self::new_from_blobs(
            device,
            vertex_dxil,
            fragment_dxil,
            root_signature,
            render_format,
        )
    }

    pub fn new_from_hlsl(
        device: &ID3D12Device,
        library: &IDxcUtils,
        dxc: &IDxcCompiler,
        shader_assembly: &ShaderCompilerOutput<String, CrossHlslContext>,
        root_signature: &D3D12RootSignature,
        render_format: DXGI_FORMAT,
    ) -> error::Result<D3D12GraphicsPipeline> {
        unsafe {
            let vertex_dxil = util::dxc_compile_shader(
                library,
                dxc,
                &shader_assembly.vertex,
                BindingStage::VERTEX,
            )?;
            let fragment_dxil = util::dxc_compile_shader(
                library,
                dxc,
                &shader_assembly.fragment,
                BindingStage::FRAGMENT,
            )?;

            Self::new_from_blobs(
                device,
                vertex_dxil,
                fragment_dxil,
                root_signature,
                render_format,
            )
        }
    }
}