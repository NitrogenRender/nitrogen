/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use nitrogen::*;

use nitrogen_examples_common::main_loop;
use nitrogen_examples_common::resource::*;

use cgmath::*;

static MODEL_DATA: &[u8] = include_bytes!("assets/bunny.obj");

fn main() {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();

    let mut ml = unsafe { main_loop::MainLoop::new("Nitrogen - Model example", init_resources) };

    while ml.running() {
        unsafe { ml.iterate() };
    }

    unsafe {
        ml.release();
    }
}

struct Data {
    _b_position: buffer::BufferHandle,
    _b_normal: buffer::BufferHandle,
    graph: graph::GraphHandle,
}

impl main_loop::UserData for Data {
    fn iteration(&mut self, store: &mut graph::Store, delta: f64) {
        store.entry::<Rad<f32>>().and_modify(|rad| {
            rad.0 += delta as f32 * 1.0;
        });
    }

    fn graph(&self) -> graph::GraphHandle {
        self.graph
    }

    fn output_image(&self) -> graph::ResourceName {
        "Base".into()
    }
}

fn init_resources(
    store: &mut graph::Store,
    ctx: &mut Context,
    submit: &mut submit_group::SubmitGroup,
) -> Data {
    store.insert(Rad(0 as f32));

    let mesh = {
        let mut reader = std::io::Cursor::new(MODEL_DATA);

        let (mut models, _) = tobj::load_obj_buf(&mut reader, |_| unimplemented!()).unwrap();

        models.remove(0).mesh
    };

    let b_position =
        unsafe { device_local_buffer_vertex(ctx, submit, &mesh.positions[..]).unwrap() };

    let b_normal = unsafe { device_local_buffer_vertex(ctx, submit, &mesh.normals[..]).unwrap() };

    let b_index = unsafe { device_local_buffer_index(ctx, submit, &mesh.indices[..]).unwrap() };

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
        ctx.vertex_attribs_create(&[create_info]).remove(0)
    };

    let graph = create_graph(
        ctx,
        vertex_def,
        b_position,
        b_normal,
        b_index,
        mesh.indices.len(),
    );

    Data {
        _b_position: b_position,
        _b_normal: b_normal,
        graph,
    }
}

fn create_graph(
    ctx: &mut Context,
    vert: vertex_attrib::VertexAttribHandle,
    position: buffer::BufferHandle,
    normal: buffer::BufferHandle,
    index: buffer::BufferHandle,
    num_vertices: usize,
) -> graph::GraphHandle {
    use std::borrow::Cow;

    let graph = ctx.graph_create();

    // base pass
    {
        let info = graph::GraphicsPassInfo {
            vertex_attrib: Some(vert),
            depth_mode: Some(graph::DepthMode {
                write: true,
                func: graph::Comparison::Less,
            }),
            stencil_mode: None,
            shaders: graph::Shaders {
                vertex: graph::ShaderInfo {
                    entry: "VertexMain".into(),
                    content: Cow::Borrowed(include_bytes!(concat!(
                        env!("OUT_DIR"),
                        "/model/model.hlsl.vert.spirv",
                    ))),
                },
                fragment: Some(graph::ShaderInfo {
                    entry: "FragmentMain".into(),
                    content: Cow::Borrowed(include_bytes!(concat!(
                        env!("OUT_DIR"),
                        "/model/model.hlsl.frag.spirv",
                    ))),
                }),
                geometry: None,
            },
            primitive: graph::Primitive::TriangleList,
            blend_modes: vec![graph::BlendMode::Alpha],
            materials: vec![],
            push_constants: vec![
                // (0..1) scale
                (0..32),
            ],
        };

        struct Pass {
            position: buffer::BufferHandle,
            normal: buffer::BufferHandle,
            index: buffer::BufferHandle,
            vertices: usize,
        };

        impl graph::GraphicsPassImpl for Pass {
            fn setup(&mut self, builder: &mut graph::GraphBuilder) {
                builder.image_create(
                    "Base",
                    graph::ImageCreateInfo {
                        size_mode: image::ImageSizeMode::ContextRelative {
                            width: 1.0,
                            height: 1.0,
                        },
                        format: image::ImageFormat::RgbaUnorm,
                        clear: graph::ImageClearValue::Color([0.1, 0.1, 0.1, 1.0]),
                    },
                );

                builder.image_create(
                    "Depth",
                    graph::ImageCreateInfo {
                        size_mode: image::ImageSizeMode::ContextRelative {
                            width: 1.0,
                            height: 1.0,
                        },
                        format: image::ImageFormat::D32Float,
                        clear: graph::ImageClearValue::DepthStencil(1.0, 0),
                    },
                );

                builder.image_write_color("Base", 0);
                builder.image_write_depth_stencil("Depth");

                builder.enable();
            }

            fn execute(&self, store: &graph::Store, cmd: &mut graph::GraphicsCommandBuffer<'_>) {
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

                unsafe {
                    push_matrix(cmd, 0, mvp);
                    push_matrix(cmd, 16, m);

                    cmd.bind_vertex_buffers(&[(self.position, 0), (self.normal, 0)]);

                    cmd.bind_index_buffer(self.index, 0, graph::IndexType::U32);

                    cmd.draw_indexed(0..self.vertices as u32, 0, 0..1);
                }
            }
        }

        let pass = Pass {
            position,
            normal,
            index,
            vertices: num_vertices,
        };

        ctx.graph_add_graphics_pass(graph, "BasePass", info, pass);
    }

    ctx.graph_add_output(graph, "Base");

    graph
}
