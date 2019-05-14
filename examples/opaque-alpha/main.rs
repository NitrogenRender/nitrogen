/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use nitrogen::*;

use nitrogen::graph::builder::GraphBuilder;
use nitrogen::resources::shader;
use nitrogen::resources::shader::{FragmentShaderHandle, VertexShaderHandle};
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
        Some("CanvasFinal".into())
    }
}

fn init_resources(
    store: &mut graph::Store,
    ctx: &mut Context,
    _submit: &mut submit_group::SubmitGroup,
) -> Option<Resources> {
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

    Some(Resources { graph })
}

fn create_graph(ctx: &mut Context) -> graph::GraphHandle {
    let mut builder = GraphBuilder::new("OpaqueAlpha");

    let vertex = {
        let info = shader::ShaderInfo {
            entry_point: "VertexMain".into(),
            spirv_content: include_bytes!(concat!(
                env!("OUT_DIR"),
                "/opaque-alpha/canvas.hlsl.vert.spirv"
            )),
        };

        ctx.vertex_shader_create(info)
    };

    let fragment = {
        let info = shader::ShaderInfo {
            entry_point: "FragmentMain".into(),
            spirv_content: include_bytes!(concat!(
                env!("OUT_DIR"),
                "/opaque-alpha/canvas.hlsl.frag.spirv"
            )),
        };

        ctx.fragment_shader_create(info)
    };

    // Opaque pass
    {
        struct OpaquePass {
            vertex: VertexShaderHandle,
            fragment: FragmentShaderHandle,
        }

        impl graph::GraphicsPass for OpaquePass {
            type Config = ();

            fn configure(&self, _config: &Self::Config) -> graph::GraphicsPipelineInfo {
                graph::GraphicsPipelineInfo {
                    vertex_attrib: None,
                    depth_mode: Some(graph::DepthMode {
                        write: true,
                        func: graph::Comparison::Less,
                    }),
                    stencil_mode: None,
                    shaders: graph::GraphicShaders {
                        vertex: graph::Shader {
                            handle: self.vertex,
                            specialization: vec![],
                        },
                        fragment: Some(graph::Shader {
                            handle: self.fragment,
                            specialization: vec![],
                        }),
                        geometry: None,
                    },
                    primitive: graph::Primitive::TriangleStrip,
                    blend_modes: vec![graph::BlendMode::Alpha],
                    materials: vec![],
                    push_constants: Some(
                        // (0..8) canvas_size
                        // (8..16) quad_pos
                        // (16..24) quad_size
                        // (24..28) quad_depth
                        // (28..32) padding
                        // (32..48) quad_color
                        0..48,
                    ),
                }
            }

            fn describe(&mut self, res: &mut graph::ResourceDescriptor) {
                res.image_create(
                    "Canvas",
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
                        format: image::ImageFormat::D32FloatS8Uint,
                        size_mode: image::ImageSizeMode::ContextRelative {
                            width: 1.0,
                            height: 1.0,
                        },
                    },
                );

                res.image_write_color("Canvas", 0);
                res.image_write_depth_stencil("Depth");
            }

            unsafe fn execute(
                &self,
                store: &graph::Store,
                dispatcher: &mut graph::GraphicsDispatcher<Self>,
            ) -> Result<(), graph::GraphExecError> {
                use nitrogen::graph::ImageClearValue::*;

                let size = store.get::<main_loop::CanvasSize>().unwrap();
                let quads = store.get::<Quads>().unwrap();

                let canvas = dispatcher.image_write_ref("Canvas")?;
                let depth = dispatcher.image_write_ref("Depth")?;

                dispatcher.clear_image(canvas, Color([0.7, 0.7, 1.0, 1.0]));
                dispatcher.clear_image(depth, DepthStencil(1.0, 0));

                dispatcher.with_config((), |cmd| {
                    cmd.push_constant::<[f32; 2]>(0, [size.0, size.1]);

                    for quad in &quads.quads {
                        cmd.push_constant::<[f32; 2]>(8, quad.pos);
                        cmd.push_constant::<[f32; 2]>(16, quad.size);

                        cmd.push_constant::<f32>(24, quad.depth);

                        cmd.push_constant::<[f32; 4]>(32, quad.color);

                        cmd.draw(0..4, 0..1);
                    }
                })?;

                Ok(())
            }
        }

        let pass = OpaquePass { fragment, vertex };

        builder.add_graphics_pass("OpaquePass", pass);
    }

    // Alpha pass
    {
        struct AlphaPass {
            vertex: VertexShaderHandle,
            fragment: FragmentShaderHandle,
        }

        impl graph::GraphicsPass for AlphaPass {
            type Config = ();

            fn configure(&self, _config: &Self::Config) -> graph::GraphicsPipelineInfo {
                graph::GraphicsPipelineInfo {
                    vertex_attrib: None,
                    depth_mode: Some(graph::DepthMode {
                        write: false,
                        func: graph::Comparison::Less,
                    }),
                    stencil_mode: None,
                    shaders: graph::GraphicShaders {
                        vertex: graph::Shader {
                            handle: self.vertex,
                            specialization: vec![],
                        },
                        fragment: Some(graph::Shader {
                            handle: self.fragment,
                            specialization: vec![],
                        }),
                        geometry: None,
                    },
                    primitive: graph::Primitive::TriangleStrip,
                    blend_modes: vec![graph::BlendMode::Alpha],
                    materials: vec![],
                    push_constants: Some(
                        // (0..8) canvas_size
                        // (8..16) quad_pos
                        // (16..24) quad_size
                        // (24..28) quad_depth
                        // (28..32) padding
                        // (32..48) quad_color
                        0..48,
                    ),
                }
            }

            fn describe(&mut self, res: &mut graph::ResourceDescriptor) {
                res.image_move("Canvas", "CanvasFinal");

                res.image_write_color("CanvasFinal", 0);
                res.image_read_depth_stencil("Depth");
            }

            unsafe fn execute(
                &self,
                store: &graph::Store,
                dispatcher: &mut graph::GraphicsDispatcher<Self>,
            ) -> Result<(), graph::GraphExecError> {
                let size = store.get::<main_loop::CanvasSize>().unwrap();
                let quads = store.get::<QuadsAlpha>().unwrap();

                dispatcher.with_config((), |cmd| {
                    cmd.push_constant::<[f32; 2]>(0, [size.0, size.1]);

                    for quad in &quads.quads {
                        cmd.push_constant::<[f32; 2]>(8, quad.pos);
                        cmd.push_constant::<[f32; 2]>(16, quad.size);

                        cmd.push_constant::<f32>(24, quad.depth);

                        cmd.push_constant::<[f32; 4]>(32, quad.color);

                        cmd.draw(0..4, 0..1);
                    }
                })?;

                Ok(())
            }
        }

        let pass = AlphaPass { vertex, fragment };

        builder.add_graphics_pass("AlphaPass", pass);
    }

    builder.add_target("CanvasFinal");

    unsafe { ctx.graph_create(builder) }.unwrap()
}
