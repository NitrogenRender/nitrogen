/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use nitrogen::*;

use nitrogen_examples_common::*;

struct QuadData {
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub color: [f32; 4],
    pub depth: f32,
}

#[derive(Default)]
struct Quads {
    pub quads: Vec<QuadData>,
}

#[derive(Default)]
struct QuadsAlpha {
    pub quads: Vec<QuadData>,
}

fn main() {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();

    let mut ml =
        unsafe { main_loop::MainLoop::new("Nitrogen - Opaque-Alpha example", init_resources) };

    while ml.running() {
        println!("frame start");
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
    fn graph(&self) -> graph::GraphHandle {
        self.graph
    }

    fn output_image(&self) -> graph::ResourceName {
        "CanvasFinal".into()
    }
}

fn init_resources(
    store: &mut graph::Store,
    ctx: &mut Context,
    _submit: &mut submit_group::SubmitGroup,
) -> Resources {
    store.insert(Quads::default());
    store.insert(QuadsAlpha::default());

    store.entry::<Quads>().and_modify(|quads| {
        {
            let quad = QuadData {
                pos: [150.0, 150.0],
                size: [150.0, 150.0],
                color: [0.3, 1.0, 0.3, 1.0],
                depth: 0.5,
            };

            quads.quads.push(quad);
        }

        {
            let quad = QuadData {
                pos: [100.0, 100.0],
                size: [150.0, 150.0],
                color: [0.3, 0.3, 1.0, 1.0],
                depth: 0.8,
            };

            quads.quads.push(quad);
        }
    });

    store.entry::<QuadsAlpha>().and_modify(|quads| {
        let quad = QuadData {
            pos: [125.0, 175.0],
            size: [150.0, 150.0],
            color: [1.0, 0.2, 0.1, 0.3],
            depth: 0.6,
        };

        quads.quads.push(quad);
    });
    let graph = create_graph(ctx);

    Resources { graph }
}

fn create_graph(ctx: &mut Context) -> graph::GraphHandle {
    use std::borrow::Cow;

    let graph = ctx.graph_create();

    // Opaque pass
    {
        let info = graph::GraphicsPassInfo {
            vertex_attrib: None,
            depth_mode: Some(graph::DepthMode {
                func: graph::Comparison::Less,
                write: true,
            }),
            stencil_mode: None,
            shaders: graph::Shaders {
                vertex: graph::ShaderInfo {
                    entry: "VertexMain".into(),
                    content: Cow::Borrowed(include_bytes!(concat!(
                        env!("OUT_DIR"),
                        "/opaque-alpha/canvas.hlsl.vert.spirv",
                    ))),
                },
                fragment: Some(graph::ShaderInfo {
                    entry: "FragmentMain".into(),
                    content: Cow::Borrowed(include_bytes!(concat!(
                        env!("OUT_DIR"),
                        "/opaque-alpha/canvas.hlsl.frag.spirv",
                    ))),
                }),
                geometry: None,
            },
            primitive: graph::Primitive::TriangleStrip,
            blend_modes: vec![graph::BlendMode::Alpha],
            materials: vec![],
            push_constants: vec![
                // (0..2) canvas_size
                // (2..4) quad_pos
                // (4..6) quad_size
                // (6..7) quad_depth
                // (7..8) padding
                // (8..12) quad_color
                (0..12),
            ],
        };

        struct OpaquePass;

        impl graph::GraphicsPassImpl for OpaquePass {
            fn setup(&mut self, builder: &mut graph::GraphBuilder) {
                builder.image_create(
                    "Canvas",
                    graph::ImageCreateInfo {
                        clear: graph::ImageClearValue::Color([0.7, 0.7, 1.0, 1.0]),
                        size_mode: image::ImageSizeMode::ContextRelative {
                            width: 1.0,
                            height: 1.0,
                        },
                        format: image::ImageFormat::RgbaUnorm,
                    },
                );

                builder.image_write_color("Canvas", 0);

                builder.image_create(
                    "Depth",
                    graph::ImageCreateInfo {
                        format: image::ImageFormat::D32FloatS8Uint,
                        size_mode: image::ImageSizeMode::ContextRelative {
                            width: 1.0,
                            height: 1.0,
                        },
                        clear: graph::ImageClearValue::DepthStencil(1.0, 0),
                    },
                );

                builder.image_write_depth_stencil("Depth");

                builder.enable();
            }

            fn execute(&self, store: &graph::Store, cmd: &mut graph::GraphicsCommandBuffer<'_>) {
                let size = store.get::<main_loop::CanvasSize>().unwrap();
                let quads = store.get::<Quads>().unwrap();

                unsafe {
                    cmd.push_constant::<[f32; 2]>(0, [size.0, size.1]);
                }

                for quad in &quads.quads {
                    unsafe {
                        cmd.push_constant::<[f32; 2]>(2, quad.pos);
                        cmd.push_constant::<[f32; 2]>(4, quad.size);

                        cmd.push_constant::<f32>(6, quad.depth);

                        cmd.push_constant::<[f32; 4]>(8, quad.color);

                        cmd.draw(0..4, 0..1);
                    }
                }
            }
        }

        ctx.graph_add_graphics_pass(graph, "OpaquePass", info, OpaquePass);
    }

    // Alpha pass
    {
        let info = graph::GraphicsPassInfo {
            vertex_attrib: None,
            depth_mode: Some(graph::DepthMode {
                func: graph::Comparison::Less,
                write: false,
            }),
            stencil_mode: None,
            shaders: graph::Shaders {
                vertex: graph::ShaderInfo {
                    entry: "VertexMain".into(),
                    content: Cow::Borrowed(include_bytes!(concat!(
                        env!("OUT_DIR"),
                        "/opaque-alpha/canvas.hlsl.vert.spirv",
                    ))),
                },
                fragment: Some(graph::ShaderInfo {
                    entry: "FragmentMain".into(),
                    content: Cow::Borrowed(include_bytes!(concat!(
                        env!("OUT_DIR"),
                        "/opaque-alpha/canvas.hlsl.frag.spirv",
                    ))),
                }),
                geometry: None,
            },
            primitive: graph::Primitive::TriangleStrip,
            blend_modes: vec![graph::BlendMode::Alpha],
            materials: vec![],
            push_constants: vec![
                // (0..2) canvas_size
                // (2..4) quad_pos
                // (4..6) quad_size
                // (6..7) quad_depth
                // (7..8) padding
                // (8..12) quad_color
                (0..12),
            ],
        };

        struct AlphaPass;

        impl graph::GraphicsPassImpl for AlphaPass {
            fn setup(&mut self, builder: &mut graph::GraphBuilder) {
                builder.image_move("Canvas", "CanvasFinal");

                builder.image_write_color("CanvasFinal", 0);

                builder.image_read_depth_stencil("Depth");

                builder.enable();
            }

            fn execute(&self, store: &graph::Store, cmd: &mut graph::GraphicsCommandBuffer<'_>) {
                let size = store.get::<main_loop::CanvasSize>().unwrap();
                let quads = store.get::<QuadsAlpha>().unwrap();

                unsafe {
                    cmd.push_constant::<[f32; 2]>(0, [size.0, size.1]);
                }

                for quad in &quads.quads {
                    unsafe {
                        cmd.push_constant::<[f32; 2]>(2, quad.pos);
                        cmd.push_constant::<[f32; 2]>(4, quad.size);

                        cmd.push_constant::<f32>(6, quad.depth);

                        cmd.push_constant::<[f32; 4]>(8, quad.color);

                        cmd.draw(0..4, 0..1);
                    }
                }
            }
        }

        ctx.graph_add_graphics_pass(graph, "AlphaPass", info, AlphaPass);
    }

    ctx.graph_add_output(graph, "CanvasFinal");

    graph
}
