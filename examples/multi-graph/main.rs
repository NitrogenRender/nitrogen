/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use nitrogen::{self as nit, buffer, graph, submit_group, vertex_attrib as vtx};

use nitrogen_examples_common::{main_loop as ml, resource as res};

struct Transform3d {
    origin: cgmath::Vector3<f32>,
    scale: cgmath::Vector3<f32>,
    rotation: cgmath::Quaternion<f32>,
}

struct Mesh {
    vertex_format: vtx::VertexAttribHandle,

    positions: buffer::BufferHandle,
    normals: buffer::BufferHandle,
    indices: Option<buffer::BufferHandle>,

    color: [f32; 3],
}

struct SpatialElement {
    mesh: Mesh,
    transform: Transform3d,
}

struct Transform2d {
    origin: cgmath::Vector2<f32>,
    scale: cgmath::Vector2<f32>,
    rotation: cgmath::Rad<f32>,
}

struct CanvasElement {
    size: (f32, f32),
    color: [f32; 4],
    transform: Transform2d,
}

struct ClearColor([f32; 4]);

fn main() {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();

    let mut main_loop = unsafe { ml::MainLoop::new("multi-graph", init) }.unwrap();

    while main_loop.running() {
        unsafe {
            main_loop.iterate();
        }
    }

    unsafe {
        main_loop.release();
    }
}

struct Data {
    backbuffer: graph::Backbuffer,

    clear_graph: graph::GraphHandle,
    graph_2d: graph::GraphHandle,
    graph_post: graph::GraphHandle,
}

impl ml::UserData for Data {
    fn iteration(&mut self, store: &mut graph::Store, delta: f64) {
        let elements = store.get_mut::<Vec<CanvasElement>>().unwrap();

        for element in elements {
            element.transform.rotation.0 += delta as f32;
        }
    }

    unsafe fn execute(
        &mut self,
        store: &mut graph::Store,
        ctx: &mut nit::Context,
        submit: &mut submit_group::SubmitGroup,
        context: &graph::ExecutionContext,
        display: nit::DisplayHandle,
    ) -> Option<()> {
        let backbuffer = &mut self.backbuffer;

        if let Err(e) = ctx.graph_compile(self.clear_graph, backbuffer, store) {
            eprintln!("{:?}", e);
            return None;
        }

        if let Err(e) = ctx.graph_compile(self.graph_2d, backbuffer, store) {
            eprintln!("{:?}", e);
            return None;
        }

        if let Err(e) = ctx.graph_compile(self.graph_post, backbuffer, store) {
            eprintln!("{:?}", e);
            return None;
        }

        submit.graph_execute(ctx, backbuffer, self.clear_graph, store, context);

        submit.graph_execute(ctx, backbuffer, self.graph_2d, store, context);

        submit.graph_execute(ctx, backbuffer, self.graph_post, store, context);

        let img = submit
            .graph_get_image(ctx, self.graph_post, "Emissive")
            .unwrap();

        // let img = backbuffer.image_get("Canvas")?;

        submit.display_present(ctx, display, img);

        Some(())
    }

    fn release(self, ctx: &mut nit::Context, submit: &mut submit_group::SubmitGroup) {
        submit.graph_destroy(ctx, &[self.clear_graph, self.graph_2d, self.graph_post]);
        submit.backbuffer_destroy(ctx, self.backbuffer);
    }
}

fn init(
    store: &mut graph::Store,
    ctx: &mut nit::Context,
    submit: &mut submit_group::SubmitGroup,
) -> Option<Data> {
    store.insert(ClearColor([0.3, 0.3, 0.3, 1.0]));

    store.insert::<Vec<CanvasElement>>((|| {
        let mut elements = vec![];

        elements.push(CanvasElement {
            size: (200.0, 150.0),
            color: [5.0, 0.0, 0.0, 1.0],
            transform: Transform2d {
                rotation: cgmath::Rad(0.0),
                scale: cgmath::Vector2::new(1.0, 1.0),
                origin: cgmath::Vector2::new(500.0, 450.0),
            },
        });

        elements.push(CanvasElement {
            size: (150.0, 200.0),
            color: [0.3, 0.4, 20.0, 0.7],
            transform: Transform2d {
                rotation: cgmath::Rad(0.0),
                scale: cgmath::Vector2::new(1.0, 1.0),
                origin: cgmath::Vector2::new(500.0, 800.0),
            },
        });

        elements
    })());

    let clear_graph = setup_graph_clear(ctx);

    let graph_2d = setup_graph_2d(ctx);

    let graph_post = setup_graph_post(ctx);

    let backbuffer = graph::Backbuffer::new();

    Some(Data {
        clear_graph,
        graph_2d,
        graph_post,
        backbuffer,
    })
}

fn setup_graph_clear(ctx: &mut nit::Context) -> graph::GraphHandle {
    let graph = ctx.graph_create();

    let mut pass_info = graph::GraphicsPassInfo::default();
    pass_info.shaders.vertex = graph::ShaderInfo {
        entry: "VertexMain".into(),
        content: std::borrow::Cow::Borrowed(include_bytes!(concat!(
            env!("OUT_DIR"),
            "/multi-graph/clear.hlsl.vert.spirv",
        ),)),
    };

    struct PassImpl;

    impl graph::GraphicsPassImpl for PassImpl {
        fn setup(&mut self, _: &mut graph::Store, builder: &mut graph::GraphBuilder) {
            builder.image_backbuffer_create(
                "Canvas",
                "Clear",
                graph::ImageCreateInfo {
                    format: nit::image::ImageFormat::Rgba32Float,
                    size_mode: nit::image::ImageSizeMode::ContextRelative {
                        width: 1.0,
                        height: 1.0,
                    },
                },
                nit::image::ImageUsage {
                    color_attachment: true,
                    transfer_src: true,
                    transfer_dst: true,
                    ..Default::default()
                },
            );
            // we don't actually write to it, we just do this so the COLOR_ATTACHMENT flag is set
            builder.image_write_color("Clear", 0);
            builder.enable();
        }

        fn execute(&self, store: &graph::Store, command_buffer: &mut graph::GraphicsCommandBuffer) {
            let clear_color = store.get::<ClearColor>().unwrap();

            unsafe {
                command_buffer.begin_render_pass(&[graph::ImageClearValue::Color(clear_color.0)]);
            }
        }
    }

    let pass_impl = PassImpl;

    ctx.graph_add_graphics_pass(graph, "ClearPass", pass_info, pass_impl);

    ctx.graph_add_output(graph, "Clear");

    graph
}

fn setup_graph_post(ctx: &mut nit::Context) -> graph::GraphHandle {
    let graph = ctx.graph_create();

    // Separate pass
    {
        let info = graph::ComputePassInfo {
            push_constants: vec![],
            materials: vec![],
            shader: graph::ShaderInfo {
                entry: "ComputeMain".into(),
                content: std::borrow::Cow::Borrowed(include_bytes!(concat!(
                    env!("OUT_DIR"),
                    "/multi-graph/post_separate.hlsl.comp.spirv",
                ))),
            },
        };

        struct PassImpl;

        impl graph::ComputePassImpl for PassImpl {
            fn setup(&mut self, store: &mut graph::Store, builder: &mut graph::GraphBuilder) {
                builder.image_create(
                    "Emissive",
                    graph::ImageCreateInfo {
                        format: nit::image::ImageFormat::Rgba32Float,
                        size_mode: nit::image::ImageSizeMode::ContextRelative {
                            width: 1.0,
                            height: 1.0,
                        },
                    },
                );
                builder.image_backbuffer_get("Canvas", "Canvas");

                builder.image_write_storage("Emissive", 0);

                builder.image_read_color("Canvas", 1, 2);

                builder.enable();
            }

            fn execute(&self, store: &graph::Store, cmd: &mut graph::ComputeCommandBuffer) {
                let ml::CanvasSize(w, h) = store.get().unwrap();

                unsafe {
                    cmd.dispatch([*w as u32, *h as u32, 1]);
                }
            }
        }

        ctx.graph_add_compute_pass(graph, "Separate", info, PassImpl);
    }

    ctx.graph_add_output(graph, "Emissive");

    graph
}

fn setup_graph_2d(ctx: &mut nit::Context) -> graph::GraphHandle {
    let graph = ctx.graph_create();

    let mut pass_info = graph::GraphicsPassInfo::default();
    pass_info.shaders.vertex = graph::ShaderInfo {
        entry: "VertexMain".into(),
        content: std::borrow::Cow::Borrowed(include_bytes!(concat!(
            env!("OUT_DIR"),
            "/multi-graph/canvas.hlsl.vert.spirv",
        ),)),
    };
    pass_info.shaders.fragment = Some(graph::ShaderInfo {
        entry: "FragmentMain".into(),
        content: std::borrow::Cow::Borrowed(include_bytes!(concat!(
            env!("OUT_DIR"),
            "/multi-graph/canvas.hlsl.frag.spirv",
        ),)),
    });
    pass_info.blend_modes = vec![graph::BlendMode::Alpha];
    pass_info.primitive = graph::Primitive::TriangleStrip;

    // 0..11 view
    // 12..23 model
    // 24..27 color
    pass_info.push_constants = vec![(0..28)];

    struct PassImpl;

    impl graph::GraphicsPassImpl for PassImpl {
        fn setup(&mut self, store: &mut graph::Store, builder: &mut graph::GraphBuilder) {
            builder.image_backbuffer_get("Canvas", "Canvas");

            builder.image_write_color("Canvas", 0);

            builder.enable();
        }

        fn execute(&self, store: &graph::Store, command_buffer: &mut graph::GraphicsCommandBuffer) {
            use cgmath::{Matrix2, Matrix3, SquareMatrix, Vector3};

            let elems = store.get::<Vec<CanvasElement>>().unwrap();

            let mut cmd = unsafe {
                match command_buffer.begin_render_pass(&[]) {
                    Some(cmd) => cmd,
                    None => return,
                }
            };

            let view = {
                let ml::CanvasSize(width, height) = *store.get().unwrap();
                let scale = Matrix3::from_diagonal(Vector3::new(0.5 / width, 0.5 / height, 1.0));
                let mut translate = Matrix3::identity();
                translate.z = Vector3::new(-0.5, -0.5, 1.0);
                let scale_to_ndc = Matrix3::from_diagonal(Vector3::new(2.0, 2.0, 1.0));

                scale_to_ndc * translate * scale
            };

            unsafe {
                push_matrix_3x3(&mut cmd, 0, view);
            }

            for elem in elems {
                let trans = {
                    let scale = Matrix3::from_diagonal(Vector3::new(0.5, 0.5, 1.0));

                    let scale = Matrix3::identity();

                    let scale =
                        scale * Matrix3::from_diagonal(Vector3::new(elem.size.0, elem.size.1, 1.0));
                    let scale = scale
                        * Matrix3::from_diagonal(Vector3::new(
                            elem.transform.scale.x,
                            elem.transform.scale.y,
                            1.0,
                        ));

                    let mut translate = Matrix3::identity();
                    translate.z =
                        Vector3::new(elem.transform.origin.x, elem.transform.origin.y, 1.0);

                    let rotate = Matrix2::from_angle(elem.transform.rotation);
                    let rotate = {
                        let mut new = Matrix3::identity();

                        new.x = Vector3::new(rotate.x.x, rotate.x.y, 0.0);
                        new.y = Vector3::new(rotate.y.x, rotate.y.y, 0.0);

                        new
                    };

                    // translate * rotate * scale
                    translate * rotate * scale
                };

                unsafe {
                    push_matrix_3x3(&mut cmd, 12, trans);
                    cmd.push_constant::<[f32; 4]>(24, elem.color);

                    cmd.draw(0..4, 0..1);
                }
            }

            unsafe fn push_matrix_3x3(
                cmd: &mut graph::RenderPassEncoder<'_>,
                offset: u32,
                mat: Matrix3<f32>,
            ) {
                use cgmath::Vector2;

                cmd.push_constant(offset + 0, mat.x);
                cmd.push_constant(offset + 4, mat.y);
                cmd.push_constant(offset + 8, mat.z);
            }
        }
    }

    let pass_impl = PassImpl;

    ctx.graph_add_graphics_pass(graph, "CanvasPass", pass_info, pass_impl);

    ctx.graph_add_output(graph, "Canvas");

    graph
}
