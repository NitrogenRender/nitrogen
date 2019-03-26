/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use nitrogen::*;

use nitrogen_examples_common::main_loop;
use nitrogen_examples_common::resource::*;

use cgmath::*;
use nitrogen::graph::builder::resource_descriptor::ImageClearValue;
use nitrogen::graph::builder::GraphBuilder;
use nitrogen::resources::shader::{FragmentShaderHandle, ShaderInfo, VertexShaderHandle};
use nitrogen::resources::vertex_attrib::VertexAttribHandle;

static MODEL_DATA: &[u8] = include_bytes!("assets/bunny.obj");

fn main() {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();

    let mut ml =
        unsafe { main_loop::MainLoop::new("Nitrogen - Model example", init_resources) }.unwrap();

    while ml.running() {
        unsafe { ml.iterate() };
    }

    unsafe {
        ml.release();
    }
}

struct Delta(f64);

struct Data {
    buf_position: buffer::BufferHandle,
    buf_normal: buffer::BufferHandle,
    buf_index: buffer::BufferHandle,

    vtx_def: vertex_attrib::VertexAttribHandle,

    graph: graph::GraphHandle,
}

impl main_loop::UserData for Data {
    fn iteration(&mut self, store: &mut graph::Store, delta: f64) {
        store.entry::<Rad<f32>>().and_modify(|rad| {
            rad.0 += delta as f32 * 1.0;
        });

        store.insert(Delta(delta));
    }

    fn graph(&self) -> Option<graph::GraphHandle> {
        Some(self.graph)
    }

    fn output_image(&self) -> Option<graph::ResourceName> {
        Some("Base".into())
    }

    fn release(self, ctx: &mut Context, submit: &mut submit_group::SubmitGroup) {
        submit.graph_destroy(ctx, &[self.graph]);

        submit.buffer_destroy(ctx, &[self.buf_normal, self.buf_position, self.buf_index]);

        unsafe {
            submit.wait(ctx);
        }

        ctx.vertex_attribs_destroy(&[self.vtx_def]);
    }
}

fn init_resources(
    store: &mut graph::Store,
    ctx: &mut Context,
    submit: &mut submit_group::SubmitGroup,
) -> Option<Data> {
    store.insert(Rad(0 as f32));

    let mesh = {
        let mut reader = std::io::Cursor::new(MODEL_DATA);

        let (mut models, _) = tobj::load_obj_buf(&mut reader, |_| unimplemented!()).unwrap();

        models.remove(0).mesh
    };

    let b_position =
        unsafe { buffer_device_local_vertex(ctx, submit, &mesh.positions[..]).unwrap() };

    let b_normal = unsafe { buffer_device_local_vertex(ctx, submit, &mesh.normals[..]).unwrap() };

    let b_index = unsafe { buffer_device_local_index(ctx, submit, &mesh.indices[..]).unwrap() };

    let vertex_def = {
        let create_info = vertex_attrib::VertexAttribInfo {
            buffer_infos: &[
                vertex_attrib::VertexAttribBufferInfo {
                    index: 0,
                    stride: std::mem::size_of::<[f32; 3]>(),
                    elements: &[vertex_attrib::VertexAttribBufferElementInfo {
                        location: 0,
                        offset: 0,
                        format: gfx::format::Format::Rgb32Float,
                    }],
                },
                vertex_attrib::VertexAttribBufferInfo {
                    index: 1,
                    stride: std::mem::size_of::<[f32; 3]>(),
                    elements: &[vertex_attrib::VertexAttribBufferElementInfo {
                        location: 1,
                        offset: 0,
                        format: gfx::format::Format::Rgb32Float,
                    }],
                },
            ],
        };
        ctx.vertex_attribs_create(create_info)
    };

    let graph = unsafe {
        create_graph(
            ctx,
            vertex_def,
            b_position,
            b_normal,
            b_index,
            mesh.indices.len(),
        )
    };

    Some(Data {
        buf_position: b_position,
        buf_normal: b_normal,
        buf_index: b_index,

        vtx_def: vertex_def,

        graph,
    })
}

unsafe fn create_graph(
    ctx: &mut Context,
    vert: vertex_attrib::VertexAttribHandle,
    position: buffer::BufferHandle,
    normal: buffer::BufferHandle,
    index: buffer::BufferHandle,
    num_vertices: usize,
) -> graph::GraphHandle {
    let mut builder = GraphBuilder::new("Pass3d");

    // base pass
    {
        let vertex_shader = {
            let info = ShaderInfo {
                entry_point: "VertexMain".into(),
                spirv_content: include_bytes!(concat!(
                    env!("OUT_DIR"),
                    "/model/model.hlsl.vert.spirv"
                )),
            };

            ctx.vertex_shader_create(info)
        };

        let fragment_shader = {
            let info = ShaderInfo {
                entry_point: "FragmentMain".into(),
                spirv_content: include_bytes!(concat!(
                    env!("OUT_DIR"),
                    "/model/model.hlsl.frag.spirv"
                )),
            };

            ctx.fragment_shader_create(info)
        };

        struct Pass {
            position: buffer::BufferHandle,
            normal: buffer::BufferHandle,
            index: buffer::BufferHandle,
            vertices: usize,
            vertex_attr: VertexAttribHandle,
            vertex_shader: VertexShaderHandle,
            fragment_shader: FragmentShaderHandle,

            run_time: f64,
            draw_lines: bool,
        };

        #[derive(Copy, Clone)]
        struct PassConfig {
            primitive: graph::Primitive,
            blend_mode: graph::BlendMode,
            use_depth: bool,
        }

        impl graph::GraphicsPass for Pass {
            type Config = PassConfig;

            fn prepare(&mut self, store: &mut graph::Store) {
                let Delta(f) = store.get().unwrap();

                self.run_time += *f;

                let seconds = self.run_time as u64;
                let seconds = seconds % 8;

                if seconds < 4 {
                    self.draw_lines = false;
                } else {
                    self.draw_lines = true;
                }
            }

            fn configure(&self, config: Self::Config) -> graph::GraphicsPipelineInfo {
                let depth = if config.use_depth {
                    Some(graph::DepthMode {
                        write: true,
                        func: graph::Comparison::Less,
                    })
                } else {
                    None
                };

                graph::GraphicsPipelineInfo {
                    vertex_attrib: Some(self.vertex_attr),
                    depth_mode: depth,
                    stencil_mode: None,
                    shaders: graph::GraphicShaders {
                        vertex: graph::Shader {
                            handle: self.vertex_shader,
                            specialization: vec![],
                        },
                        fragment: Some(graph::Shader {
                            handle: self.fragment_shader,
                            specialization: vec![],
                        }),
                        geometry: None,
                    },
                    primitive: config.primitive,
                    blend_modes: vec![config.blend_mode],
                    materials: vec![],
                    push_constants: Some(0..32),
                }
            }

            fn describe(&mut self, res: &mut graph::ResourceDescriptor) {
                res.image_create(
                    "Base",
                    graph::ImageCreateInfo {
                        size_mode: image::ImageSizeMode::ContextRelative {
                            width: 1.0,
                            height: 1.0,
                        },
                        format: image::ImageFormat::RgbaUnorm,
                    },
                );

                res.image_create(
                    "Depth",
                    graph::ImageCreateInfo {
                        size_mode: image::ImageSizeMode::ContextRelative {
                            width: 1.0,
                            height: 1.0,
                        },
                        format: image::ImageFormat::D32Float,
                    },
                );

                res.image_write_color("Base", 0);
                res.image_write_depth_stencil("Depth");
            }

            unsafe fn execute(
                &self,
                store: &graph::Store,
                dispatcher: &mut graph::GraphicsDispatcher<Self>,
            ) -> Result<(), graph::GraphExecError> {
                let m = {
                    let Rad(rad) = store.get().unwrap();

                    let rotate = Matrix4::from_axis_angle(Vector3::new(0.0, 1.0, 0.0), Rad(*rad));

                    let scale = Matrix4::from_scale(12.0f32);
                    let scale = scale * Matrix4::from_nonuniform_scale(1.0, -1.0, 1.0);
                    let translation = Matrix4::from_translation(Vector3::new(0.0, 1.2, -2.5));

                    translation * rotate * scale
                };

                let p = {
                    let main_loop::CanvasSize(width, height) = store.get().unwrap();

                    perspective::<f32, _>(Deg(70.0), width / height, 0.003, 100.0)
                };

                let mvp = p * m;

                let color = dispatcher.image_write_ref("Base")?;
                let depth = dispatcher.image_write_ref("Depth")?;

                dispatcher.clear_image(color, ImageClearValue::Color([0.1, 0.1, 0.1, 1.0]));
                dispatcher.clear_image(depth, ImageClearValue::DepthStencil(1.0, 0));

                let mut config = PassConfig {
                    blend_mode: graph::BlendMode::Alpha,
                    primitive: graph::Primitive::TriangleList,
                    use_depth: true,
                };

                dispatcher.with_config(config, |cmd| {
                    unsafe fn push_matrix(
                        cmd: &mut graph::GraphicsCommandBuffer<'_>,
                        offset: u32,
                        mat: Matrix4<f32>,
                    ) {
                        cmd.push_constant(offset + 0, mat.x);
                        cmd.push_constant(offset + 4, mat.y);
                        cmd.push_constant(offset + 8, mat.z);
                        cmd.push_constant(offset + 12, mat.w);
                    }

                    push_matrix(cmd, 0, mvp);
                    push_matrix(cmd, 16, m);

                    cmd.bind_vertex_buffers(&[(self.position, 0), (self.normal, 0)]);

                    cmd.bind_index_buffer(self.index, 0, graph::IndexType::U32);

                    cmd.draw_indexed(0..self.vertices as u32, 0, 0..1);
                })?;

                if self.draw_lines {
                    config.primitive = graph::Primitive::LineList;
                    config.blend_mode = graph::BlendMode::Add;
                    config.use_depth = false;

                    dispatcher.with_config(config, |cmd| {
                        unsafe fn push_matrix(
                            cmd: &mut graph::GraphicsCommandBuffer<'_>,
                            offset: u32,
                            mat: Matrix4<f32>,
                        ) {
                            cmd.push_constant(offset + 0, mat.x);
                            cmd.push_constant(offset + 4, mat.y);
                            cmd.push_constant(offset + 8, mat.z);
                            cmd.push_constant(offset + 12, mat.w);
                        }

                        push_matrix(cmd, 0, mvp);
                        push_matrix(cmd, 16, m);

                        cmd.bind_vertex_buffers(&[(self.position, 0), (self.normal, 0)]);

                        cmd.bind_index_buffer(self.index, 0, graph::IndexType::U32);

                        cmd.draw_indexed(0..self.vertices as u32, 0, 0..1);
                    })?;
                }

                Ok(())
            }
        }

        let pass = Pass {
            position,
            normal,
            index,
            vertices: num_vertices,

            vertex_attr: vert,

            vertex_shader,
            fragment_shader,

            draw_lines: false,
            run_time: 0.0,
        };

        builder.add_graphics_pass("BasePass", pass);
    }

    builder.add_target("Base");

    ctx.graph_create(builder).unwrap()
}
