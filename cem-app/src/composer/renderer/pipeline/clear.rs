use crate::composer::renderer::{
    Renderer,
    RendererConfig,
};

pub struct ClearPipelineDescriptor<'a> {
    pub renderer_config: &'a RendererConfig,
    pub camera_bind_group_layout: &'a wgpu::BindGroupLayout,
    pub shader_module: &'a wgpu::ShaderModule,
}

#[derive(Debug)]
pub struct ClearPipeline {
    pub layout: wgpu::PipelineLayout,
    pub pipeline: wgpu::RenderPipeline,
}

impl ClearPipeline {
    pub fn new(device: &wgpu::Device, descriptor: &ClearPipelineDescriptor) -> Self {
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("render/clear"),
            bind_group_layouts: &[descriptor.camera_bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("render/clear"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: descriptor.shader_module,
                entry_point: Some("vs_main_clear"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: Renderer::FRONT_FACE,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: descriptor.renderer_config.depth_texture_format.map(
                |depth_texture_format| {
                    wgpu::DepthStencilState {
                        format: depth_texture_format,
                        // egui clears the depth buffer with a value of 1.0 when starting the render
                        // pass. but we could enable this to write to the
                        // depth buffer with the clear shader - if
                        // we want to clear to another value.
                        depth_write_enabled: false,
                        // ignore current state of depth buffer
                        depth_compare: wgpu::CompareFunction::Always,
                        stencil: {
                            // this should always zero the stencil state
                            let stencil_face_state = wgpu::StencilFaceState {
                                compare: wgpu::CompareFunction::Always,
                                fail_op: wgpu::StencilOperation::Zero,
                                depth_fail_op: wgpu::StencilOperation::Zero,
                                pass_op: wgpu::StencilOperation::Zero,
                            };
                            wgpu::StencilState {
                                // same stencil face state for both faces
                                front: stencil_face_state,
                                back: stencil_face_state,
                                // ignore buffer state
                                read_mask: 0x00,
                                // clear bits
                                write_mask: 0xff,
                            }
                        },
                        bias: Default::default(),
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
                entry_point: Some("fs_main_single_color"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: descriptor.renderer_config.target_texture_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });

        Self { layout, pipeline }
    }
}
