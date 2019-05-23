/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

extern crate image as img;

use nitrogen::{buffer, graph, image, material, shader, submit_group, vertex_attrib, Context};

use log::debug;

use nitrogen::graph::{
    GraphExecError, GraphicsDispatcher, GraphicsPass, GraphicsPipelineInfo, ImageClearValue,
    ResourceDescriptor, Store,
};
use nitrogen_examples_common::main_loop;

const QUAD_POS: [[f32; 2]; 4] = [
    [-1.0, -1.0], // LEFT TOP
    [-1.0, 1.0],  // LEFT BOTTOM
    [1.0, -1.0],  // RIGHT TOP
    [1.0, 1.0],   // RIGHT BOTTOM
];

const QUAD_UV: [[f32; 2]; 4] = [
    [0.0, 0.0], // LEFT TOP
    [0.0, 1.0], // LEFT BOTTOM
    [1.0, 0.0], // RIGHT TOP
    [1.0, 1.0], // RIGHT BOTTOM
];

fn main() {
    std::env::set_var("RUST_LOG", "warn");
    env_logger::init();

    let mut ml =
        unsafe { main_loop::MainLoop::new("Nitrogen - Opaque-Alpha example", init_resources) }
            .unwrap();

    while ml.running() {
        unsafe {
            ml.iterate();
        }
    }

    unsafe {
        ml.release();
    }
}

struct Resources {
    graph: graph::GraphHandle,
}

impl main_loop::UserData for Resources {
    fn graph(&self) -> Option<graph::GraphHandle> {
        Some(self.graph)
    }

    fn output_image(&self) -> Option<graph::ResourceName> {
        Some("IOutput".into())
    }
}

struct QuadData {
    vtx_pos: buffer::BufferHandle,
    vtx_uv: buffer::BufferHandle,

    mat_instance: material::MaterialInstanceHandle,
}

fn init_resources(
    store: &mut graph::Store,
    ctx: &mut Context,
    group: &mut submit_group::SubmitGroup,
) -> Option<Resources> {
    let material = unsafe {
        let create_info = nitrogen::material::MaterialCreateInfo {
            parameters: &[
                (0, nitrogen::material::MaterialParameterType::SampledImage),
                (1, nitrogen::material::MaterialParameterType::Sampler),
                (2, nitrogen::material::MaterialParameterType::UniformBuffer),
            ],
        };

        ctx.material_create(create_info).unwrap()
    };

    let mat_example_instance = unsafe { ctx.material_create_instance(material).unwrap() };

    let (image, sampler) = {
        let image_data = include_bytes!("assets/test.png");

        let image = img::load(std::io::Cursor::new(&image_data[..]), img::PNG)
            .unwrap()
            .to_rgba();

        let (width, height) = image.dimensions();
        let dimension = image::ImageDimension::D2 {
            x: width,
            y: height,
        };

        let create_info = image::ImageCreateInfo {
            dimension,
            num_layers: 1,
            num_samples: 1,
            num_mipmaps: 1,

            usage: image::ImageUsage {
                transfer_dst: true,
                sampling: true,
                ..Default::default()
            },

            ..Default::default()
        };

        let img = unsafe { ctx.image_create(create_info).unwrap() };

        debug!("width {}, height {}", width, height);

        unsafe {
            let data = image::ImageUploadInfo {
                data: &(*image),
                format: image::ImageFormat::RgbaUnorm,
                dimension,
                target_offset: (0, 0, 0),
            };

            group.image_upload_data(ctx, img, data).unwrap();
            group.wait(ctx);
        }

        drop(image);

        let sampler = unsafe {
            use nitrogen::sampler::{Filter, WrapMode};

            let sampler_create = nitrogen::sampler::SamplerCreateInfo {
                min_filter: Filter::Linear,
                mag_filter: Filter::Linear,
                mip_filter: Filter::Linear,
                wrap_mode: (WrapMode::Clamp, WrapMode::Clamp, WrapMode::Clamp),
            };

            ctx.sampler_create(sampler_create)
        };

        (img, sampler)
    };

    let buffer_pos = unsafe {
        let create_info = nitrogen::buffer::CpuVisibleCreateInfo {
            size: std::mem::size_of_val(&QUAD_POS) as u64,
            is_transient: false,
            usage: nitrogen::buffer::BufferUsage::TRANSFER_SRC
                | nitrogen::buffer::BufferUsage::VERTEX,
        };
        let buffer = ctx.buffer_cpu_visible_create(create_info).unwrap();

        let upload_data = nitrogen::buffer::BufferUploadInfo {
            offset: 0,
            data: &QUAD_POS,
        };

        group
            .buffer_cpu_visible_upload(ctx, buffer, upload_data)
            .unwrap();

        buffer
    };

    let buffer_uv = unsafe {
        let create_info = nitrogen::buffer::CpuVisibleCreateInfo {
            size: std::mem::size_of_val(&QUAD_UV) as u64,
            is_transient: false,
            usage: nitrogen::buffer::BufferUsage::TRANSFER_SRC
                | nitrogen::buffer::BufferUsage::VERTEX,
        };
        let buffer = ctx.buffer_cpu_visible_create(create_info).unwrap();

        let upload_data = nitrogen::buffer::BufferUploadInfo {
            offset: 0,
            data: &QUAD_UV,
        };

        group
            .buffer_cpu_visible_upload(ctx, buffer, upload_data)
            .unwrap();

        buffer
    };

    unsafe {
        group.wait(ctx);

        ctx.material_write_instance(
            mat_example_instance,
            &[
                nitrogen::material::InstanceWrite {
                    binding: 0,
                    data: nitrogen::material::InstanceWriteData::Image { image },
                },
                nitrogen::material::InstanceWrite {
                    binding: 1,
                    data: nitrogen::material::InstanceWriteData::Sampler { sampler },
                },
            ],
        );
    }

    let quad_data = QuadData {
        vtx_pos: buffer_pos,
        vtx_uv: buffer_uv,
        mat_instance: mat_example_instance,
    };

    store.insert(quad_data);

    let builder = create_graph(ctx, material);

    let graph = unsafe { ctx.graph_create(builder).unwrap() };

    Some(Resources { graph })
}

fn create_graph(ctx: &mut Context, mat: material::MaterialHandle) -> graph::GraphBuilder {
    let mut builder = graph::GraphBuilder::new("TwoPass");

    fn image_create_info() -> graph::ImageCreateInfo {
        graph::ImageCreateInfo {
            format: image::ImageFormat::RgbaUnorm,
            size_mode: image::ImageSizeMode::ContextRelative {
                width: 1.0,
                height: 1.0,
            },
        }
    }
    // test pass
    {
        let vertex = {
            let info = shader::ShaderInfo {
                entry_point: "VertexMain".into(),
                spirv_content: include_bytes!(concat!(
                    env!("OUT_DIR"),
                    "/two-pass/test.hlsl.vert.spirv"
                )),
            };

            ctx.vertex_shader_create(info)
        };

        let fragment = {
            let info = shader::ShaderInfo {
                entry_point: "FragmentMain".into(),
                spirv_content: include_bytes!(concat!(
                    env!("OUT_DIR"),
                    "/two-pass/test.hlsl.frag.spirv"
                )),
            };

            ctx.fragment_shader_create(info)
        };

        struct TestPass {
            shader_vertex: shader::VertexShaderHandle,
            shader_fragment: shader::FragmentShaderHandle,
            mat: material::MaterialHandle,
        }

        impl GraphicsPass for TestPass {
            type Config = ();

            fn configure(&self, (): &()) -> GraphicsPipelineInfo {
                GraphicsPipelineInfo {
                    vertex_attrib: Some(vertex_attrib::VertexAttrib {
                        buffer_infos: vec![
                            vertex_attrib::VertexAttribBufferInfo {
                                stride: std::mem::size_of::<[f32; 2]>(),
                                index: 0,
                                elements: vec![
                                    nitrogen::vertex_attrib::VertexAttribBufferElementInfo {
                                        location: 0,
                                        format: nitrogen::gfx::format::Format::Rg32Sfloat,
                                        offset: 0,
                                    },
                                ],
                            },
                            // uv
                            vertex_attrib::VertexAttribBufferInfo {
                                stride: std::mem::size_of::<[f32; 2]>(),
                                index: 1,
                                elements: vec![
                                    nitrogen::vertex_attrib::VertexAttribBufferElementInfo {
                                        location: 1,
                                        format: nitrogen::gfx::format::Format::Rg32Sfloat,
                                        offset: 0,
                                    },
                                ],
                            },
                        ],
                    }),
                    depth_mode: None,
                    stencil_mode: None,
                    shaders: graph::GraphicShaders {
                        vertex: graph::Shader {
                            handle: self.shader_vertex,
                            specialization: vec![],
                        },
                        fragment: Some(graph::Shader {
                            handle: self.shader_fragment,
                            specialization: vec![],
                        }),
                        geometry: None,
                    },
                    primitive: graph::Primitive::TriangleStrip,
                    blend_modes: vec![graph::BlendMode::Alpha],
                    materials: vec![(0, self.mat)],
                    push_constants: None,
                }
            }

            fn describe(&mut self, res: &mut ResourceDescriptor) {
                res.image_create("ITest", image_create_info());

                res.image_write_color("ITest", 0);
            }

            unsafe fn execute(
                &self,
                store: &Store,
                dispatcher: &mut GraphicsDispatcher<Self>,
            ) -> Result<(), GraphExecError> {
                let quad = store.get::<QuadData>().unwrap();

                let canvas = dispatcher.image_write_ref("ITest")?;

                dispatcher.clear_image(canvas, ImageClearValue::Color([0.0, 0.0, 0.0, 0.0]));

                dispatcher.with_config((), |cmd| {
                    cmd.bind_vertex_buffers(&[(quad.vtx_pos, 0), (quad.vtx_uv, 0)]);
                    cmd.bind_material(0, quad.mat_instance);
                    cmd.draw(0..4, 0..1);
                })?;

                Ok(())
            }
        }

        let pass = TestPass {
            shader_vertex: vertex,
            shader_fragment: fragment,
            mat,
        };

        builder.add_graphics_pass("Test", pass);
    }

    // read pass
    {
        let vertex = {
            let info = shader::ShaderInfo {
                entry_point: "VertexMain".into(),
                spirv_content: include_bytes!(concat!(
                    env!("OUT_DIR"),
                    "/two-pass/read.hlsl.vert.spirv"
                )),
            };

            ctx.vertex_shader_create(info)
        };

        let fragment = {
            let info = shader::ShaderInfo {
                entry_point: "FragmentMain".into(),
                spirv_content: include_bytes!(concat!(
                    env!("OUT_DIR"),
                    "/two-pass/read.hlsl.frag.spirv"
                )),
            };

            ctx.fragment_shader_create(info)
        };

        struct ReadPass {
            shader_vertex: shader::VertexShaderHandle,
            shader_fragment: shader::FragmentShaderHandle,
        }

        impl GraphicsPass for ReadPass {
            type Config = ();

            fn configure(&self, (): &()) -> GraphicsPipelineInfo {
                GraphicsPipelineInfo {
                    vertex_attrib: Some(vertex_attrib::VertexAttrib {
                        buffer_infos: vec![
                            vertex_attrib::VertexAttribBufferInfo {
                                stride: std::mem::size_of::<[f32; 2]>(),
                                index: 0,
                                elements: vec![
                                    nitrogen::vertex_attrib::VertexAttribBufferElementInfo {
                                        location: 0,
                                        format: nitrogen::gfx::format::Format::Rg32Sfloat,
                                        offset: 0,
                                    },
                                ],
                            },
                            // uv
                            vertex_attrib::VertexAttribBufferInfo {
                                stride: std::mem::size_of::<[f32; 2]>(),
                                index: 1,
                                elements: vec![
                                    nitrogen::vertex_attrib::VertexAttribBufferElementInfo {
                                        location: 1,
                                        format: nitrogen::gfx::format::Format::Rg32Sfloat,
                                        offset: 0,
                                    },
                                ],
                            },
                        ],
                    }),
                    depth_mode: None,
                    stencil_mode: None,
                    shaders: graph::GraphicShaders {
                        vertex: graph::Shader {
                            handle: self.shader_vertex,
                            specialization: vec![],
                        },
                        fragment: Some(graph::Shader {
                            handle: self.shader_fragment,
                            specialization: vec![],
                        }),
                        geometry: None,
                    },
                    primitive: graph::Primitive::TriangleStrip,
                    blend_modes: vec![graph::BlendMode::Alpha],
                    materials: vec![],
                    push_constants: None,
                }
            }

            fn describe(&mut self, res: &mut ResourceDescriptor) {
                res.image_create("IOutput", image_create_info());

                res.image_write_color("IOutput", 0);

                res.image_read_color("ITest", 0, Some(1));
            }

            unsafe fn execute(
                &self,
                store: &Store,
                dispatcher: &mut GraphicsDispatcher<Self>,
            ) -> Result<(), GraphExecError> {
                let quad = store.get::<QuadData>().unwrap();

                let canvas = dispatcher.image_write_ref("IOutput")?;

                dispatcher.clear_image(canvas, ImageClearValue::Color([0.0, 0.0, 0.0, 0.0]));

                dispatcher.with_config((), |cmd| {
                    cmd.bind_vertex_buffers(&[(quad.vtx_pos, 0), (quad.vtx_uv, 0)]);
                    cmd.draw(0..4, 0..1);
                })?;

                Ok(())
            }
        }

        let pass = ReadPass {
            shader_vertex: vertex,
            shader_fragment: fragment,
        };

        builder.add_graphics_pass("Read", pass);
    }

    builder.add_target("IOutput");
    builder
}
