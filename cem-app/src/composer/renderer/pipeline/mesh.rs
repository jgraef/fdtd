use crate::composer::renderer::{
    DepthState,
    Renderer,
    RendererConfig,
    Stencil,
    StencilTest,
};

pub struct MeshPipelineDescriptor<'a> {
    pub label: &'a str,
    pub renderer_config: &'a RendererConfig,
    pub camera_bind_group_layout: &'a wgpu::BindGroupLayout,
    pub mesh_bind_group_layout: &'a wgpu::BindGroupLayout,
    pub shader_module: &'a wgpu::ShaderModule,
    pub depth_state: DepthState,
    pub stencil_state: wgpu::StencilState,
    pub topology: wgpu::PrimitiveTopology,
    pub vertex_shader_entry_point: &'a str,
    pub fragment_shader_entry_point: &'a str,
    pub alpha_blending: bool,
}

#[derive(Debug)]
pub struct MeshPipeline {
    pub layout: wgpu::PipelineLayout,
    pub pipeline: wgpu::RenderPipeline,
}

impl MeshPipeline {
    pub fn new(device: &wgpu::Device, descriptor: &MeshPipelineDescriptor) -> Self {
        // enable/disable back-face culling. the mesh shader will paint back-faces
        // bright pink, so we'll know if something is flipped.
        const CULL_BACK_FACES: bool = true;

        let cull_back_faces = match descriptor.topology {
            wgpu::PrimitiveTopology::PointList
            | wgpu::PrimitiveTopology::LineList
            | wgpu::PrimitiveTopology::LineStrip => false,
            wgpu::PrimitiveTopology::TriangleList | wgpu::PrimitiveTopology::TriangleStrip => {
                CULL_BACK_FACES
            }
        };
        let cull_mode = cull_back_faces.then_some(wgpu::Face::Back);

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(descriptor.label),
            bind_group_layouts: &[
                descriptor.camera_bind_group_layout,
                descriptor.mesh_bind_group_layout,
            ],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(descriptor.label),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: descriptor.shader_module,
                entry_point: Some(descriptor.vertex_shader_entry_point),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: descriptor.topology,
                strip_index_format: None,
                front_face: Renderer::FRONT_FACE,
                cull_mode,
                unclipped_depth: false,
                polygon_mode: Default::default(),
                conservative: false,
            },
            depth_stencil: descriptor.renderer_config.depth_texture_format.map(
                |depth_texture_format| {
                    wgpu::DepthStencilState {
                        format: depth_texture_format,
                        depth_write_enabled: descriptor.depth_state.write_enable,
                        depth_compare: descriptor.depth_state.compare,
                        stencil: descriptor.stencil_state.clone(),
                        bias: descriptor.depth_state.bias,
                    }
                },
            ),
            multisample: wgpu::MultisampleState {
                count: descriptor.renderer_config.multisample_count.get(),
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: descriptor.shader_module,
                entry_point: Some(descriptor.fragment_shader_entry_point),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: descriptor.renderer_config.target_texture_format,
                    blend: descriptor
                        .alpha_blending
                        .then_some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });

        Self { layout, pipeline }
    }
}

pub trait StencilStateExt {
    fn new(
        write_to_stencil_buffer: Option<Stencil>,
        test_against_stencil_buffer: Option<StencilTest>,
    ) -> Self;
}

impl StencilStateExt for wgpu::StencilState {
    fn new(
        write_to_stencil_buffer: Option<Stencil>,
        test_against_stencil_buffer: Option<StencilTest>,
    ) -> Self {
        let mut stencil_state = wgpu::StencilState::default();
        if let Some(write_mask) = write_to_stencil_buffer {
            // write stencil reference to stencil buffer
            // if read_mask is None, this will be always written, if the depth-test passes.
            stencil_state.front.pass_op = wgpu::StencilOperation::Replace;
            // only write the bits that we want to manipulate
            stencil_state.write_mask = write_mask.into();

            // remove this and the selected object will shine through
            // anything occluding with the outline color.
            // give it a try :) but unfortunately if we
            // render something without stencil afterwards
            // it'll replace the value in the buffer, so the
            // outline will also shine through the object we
            // want outlined :(
            //
            // stencil_state.front.depth_fail_op =
            // wgpu::StencilOperation::Replace;
        }
        if let Some(stencil_test) = test_against_stencil_buffer {
            // check masked stencil buffer against stencil reference.
            stencil_state.front.compare = stencil_test.compare;
            stencil_state.read_mask = stencil_test.read_mask.into();
        }

        stencil_state
    }
}
