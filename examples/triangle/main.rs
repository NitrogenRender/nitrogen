/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use nitrogen::Context;

struct AppState {
    graph: nitrogen::graph::GraphHandle,
    _vertex_buffer: nitrogen::buffer::BufferHandle,
    groups: Vec<nitrogen::SubmitGroup>,
}

fn main() {
    std::env::set_var("RUST_LOG", "warn");
    env_logger::init();

    let mut ctx = unsafe { Context::new("triangle example", 1) };

    let mut events_loop = winit::EventsLoop::new();
    let window = winit::Window::new(&events_loop).unwrap();

    let display = ctx.display_add(&window);

    let mut appstate = unsafe { create_appstate(&mut ctx) };

    let mut group_idx = 0;

    let mut is_running = true;
    let mut resized = true;

    let mut ctx_size = {
        let s = window.get_inner_size().unwrap();
        (s.width, s.height)
    };

    while is_running {
        events_loop.poll_events(|ev| match ev {
            winit::Event::WindowEvent { event, .. } => match event {
                winit::WindowEvent::CloseRequested => {
                    is_running = false;
                }
                winit::WindowEvent::Resized(size) => {
                    resized = true;
                    ctx_size = (size.width, size.height);
                }
                _ => {}
            },
            _ => {}
        });

        let group = &mut appstate.groups[group_idx];
        unsafe {
            group.wait(&mut ctx);
        }

        if resized {
            unsafe {
                group.display_setup_swapchain(&mut ctx, display);
            }

            resized = false;
        }

        let mut backbuffer = nitrogen::graph::Backbuffer::new();
        let mut store = nitrogen::graph::Store::new();

        let exec_context = nitrogen::graph::ExecutionContext {
            reference_size: (ctx_size.0 as _, ctx_size.1 as _),
        };

        unsafe {
            group
                .graph_execute(
                    &mut ctx,
                    &mut backbuffer,
                    appstate.graph,
                    &mut store,
                    &exec_context,
                )
                .expect("Graph execution");

            let img = group
                .graph_get_image(&ctx, appstate.graph, "Output")
                .unwrap();

            group.display_present(&mut ctx, display, img);
        }

        group_idx = (group_idx + 1) % appstate.groups.len();
    }

    unsafe {
        appstate.groups[0].graph_destroy(&mut ctx, &[appstate.graph]);

        for mut group in appstate.groups {
            group.wait(&mut ctx);
            group.release(&mut ctx);
        }

        ctx.release();
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
struct Vertex {
    pos: [f32; 2],
    uv: [f32; 2],
}

unsafe fn create_appstate(ctx: &mut Context) -> AppState {
    let mut groups = vec![ctx.create_submit_group(), ctx.create_submit_group()];

    let vertex_attribs = {
        use nitrogen::vertex_attrib::*;

        VertexAttrib {
            buffer_infos: vec![VertexAttribBufferInfo {
                index: 0,
                stride: std::mem::size_of::<Vertex>(),
                elements: vec![
                    // position
                    VertexAttribBufferElementInfo {
                        location: 0,
                        format: nitrogen::gfx::format::Format::Rg32Sfloat,
                        offset: 0,
                    },
                    VertexAttribBufferElementInfo {
                        location: 1,
                        format: nitrogen::gfx::format::Format::Rg32Sfloat,
                        offset: std::mem::size_of::<[f32; 2]>() as _,
                    },
                ],
            }],
        }
    };
    let vertex_buffer = {
        let data = [
            // bottom left
            Vertex {
                pos: [0.75, 0.75],
                uv: [0.0, 0.0],
            },
            // top
            Vertex {
                pos: [0.0, -0.75],
                uv: [0.0, 1.0],
            },
            // bottom right
            Vertex {
                pos: [-0.75, 0.75],
                uv: [1.0, 1.0],
            },
        ];

        let info = nitrogen::buffer::CpuVisibleCreateInfo {
            size: (std::mem::size_of::<Vertex>() * 3) as _,
            is_transient: false,
            usage: {
                use nitrogen::gfx::buffer::Usage;

                Usage::TRANSFER_DST | Usage::TRANSFER_SRC | Usage::VERTEX
            },
        };

        let buf = ctx
            .buffer_cpu_visible_create(info)
            .expect("Can't create buffer");

        let upload = nitrogen::buffer::BufferUploadInfo {
            offset: 0,
            data: &data[..],
        };

        groups[0]
            .buffer_cpu_visible_upload(ctx, buf, upload)
            .unwrap();

        groups[0].wait(ctx);

        buf
    };

    let graph = {
        let builder = create_graph(ctx, vertex_attribs, vertex_buffer);

        ctx.graph_create(builder).expect("Can't create graph")
    };

    AppState {
        graph,
        _vertex_buffer: vertex_buffer,
        groups,
    }
}

fn create_graph(
    ctx: &mut Context,
    vtx: nitrogen::vertex_attrib::VertexAttrib,
    vtx_buf: nitrogen::buffer::BufferHandle,
) -> nitrogen::graph::GraphBuilder {
    use nitrogen::*;

    let mut builder = graph::GraphBuilder::new("Triangle");

    struct TrianglePass {
        vtx: vertex_attrib::VertexAttrib,
        vertex_buffer: buffer::BufferHandle,

        shader_vertex: shader::VertexShaderHandle,
        shader_fragment: shader::FragmentShaderHandle,
    }

    impl graph::GraphicsPass for TrianglePass {
        type Config = ();

        fn describe(&mut self, res: &mut graph::ResourceDescriptor) {
            res.image_create(
                "Output",
                graph::ImageCreateInfo {
                    format: image::ImageFormat::Rgba32Float,
                    size_mode: image::ImageSizeMode::ContextRelative {
                        width: 1.0,
                        height: 1.0,
                    },
                },
            );

            res.image_write_color("Output", 0);
        }

        fn configure(&self, _config: &Self::Config) -> graph::GraphicsPipelineInfo {
            graph::GraphicsPipelineInfo {
                vertex_attrib: Some(self.vtx.clone()),
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
                primitive: graph::Primitive::TriangleList,
                blend_modes: vec![graph::BlendMode::Alpha],
                materials: vec![],
                push_constants: None,
            }
        }

        unsafe fn execute(
            &self,
            _store: &graph::Store,
            dispatcher: &mut graph::GraphicsDispatcher<Self>,
        ) -> Result<(), graph::GraphExecError> {
            let output = dispatcher.image_write_ref("Output")?;

            dispatcher.clear_image(output, graph::ImageClearValue::Color([0.0, 0.0, 0.0, 0.0]));

            dispatcher.with_config((), |cmd| {
                cmd.bind_vertex_buffers(&[(self.vertex_buffer, 0)]);

                cmd.draw(0..3, 0..1);
            })?;

            Ok(())
        }
    }

    let shader_vertex = {
        let info = nitrogen::shader::ShaderInfo {
            spirv_content: include_bytes!(concat!(
                env!("OUT_DIR"),
                "/triangle/triangle.hlsl.vert.spirv",
            ),),
            entry_point: "VertexMain".into(),
        };

        ctx.vertex_shader_create(info)
    };

    let shader_fragment = {
        let info = nitrogen::shader::ShaderInfo {
            spirv_content: include_bytes!(concat!(
                env!("OUT_DIR"),
                "/triangle/triangle.hlsl.frag.spirv",
            ),),
            entry_point: "FragmentMain".into(),
        };

        ctx.fragment_shader_create(info)
    };

    let pass = TrianglePass {
        vtx,
        vertex_buffer: vtx_buf,

        shader_vertex,
        shader_fragment,
    };

    builder.add_graphics_pass("TrianglePass", pass);
    builder.add_target("Output");

    builder
}
