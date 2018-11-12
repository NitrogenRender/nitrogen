use nitrogen::graph::PassImpl;

extern crate env_logger;
extern crate image as img;
extern crate log;
extern crate nitrogen;
extern crate winit;

use nitrogen::graph;
use nitrogen::image;

use log::debug;

use std::borrow::Cow;

#[derive(Debug, Clone, Copy)]
struct Vertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
}

const TRIANGLE: [Vertex; 3] = [
    Vertex {
        pos: [0.0, -0.5],
        uv: [0.0, 0.0],
    }, // TOP
    Vertex {
        pos: [-0.5, 0.5],
        uv: [0.0, 0.0],
    }, // LEFT
    Vertex {
        pos: [0.5, 0.5],
        uv: [0.0, 0.0],
    }, // RIGHT
];

fn main() {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();

    let mut events = winit::EventsLoop::new();
    let window = winit::Window::new(&events).unwrap();

    let mut ntg = nitrogen::Context::new("nitrogen test", 1);

    let display = ntg.add_display(&window);

    let (image, sampler) = {
        let image_data = include_bytes!("../test.png");

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

            .. Default::default()
        };

        let img = ntg.image_create(&[create_info]).remove(0).unwrap();

        debug!("width {}, height {}", width, height);

        {
            let data = image::ImageUploadInfo {
                data: &(*image),
                format: image::ImageFormat::RgbaUnorm,
                dimension,
                target_offset: (0, 0, 0),
            };

            ntg.image_upload_data(&[(img, data)]).remove(0).unwrap()
        }

        drop(image);

        let sampler = {
            use nitrogen::sampler::{Filter, WrapMode};

            let sampler_create = nitrogen::sampler::SamplerCreateInfo {
                min_filter: Filter::Linear,
                mag_filter: Filter::Linear,
                mip_filter: Filter::Linear,
                wrap_mode: (WrapMode::Clamp, WrapMode::Clamp, WrapMode::Clamp),
            };

            ntg.sampler_create(&[sampler_create]).remove(0)
        };

        (img, sampler)
    };

    ntg.displays[display].setup_swapchain(&ntg.device_ctx);

    let buffer = {
        let create_info = nitrogen::buffer::BufferCreateInfo {
            size: std::mem::size_of_val(&TRIANGLE) as u64,
            is_transient: false,
            usage: nitrogen::buffer::BufferUsage::TRANSFER_SRC,
            properties: nitrogen::resources::MemoryProperties::DEVICE_LOCAL,
        };
        let buffer = ntg
            .buffer_storage
            .create(&ntg.device_ctx, &[create_info])
            .remove(0)
            .unwrap();

        let upload_data = nitrogen::buffer::BufferUploadInfo {
            offset: 0,
            data: &TRIANGLE,
        };

        ntg.buffer_storage.upload_data(
            &ntg.device_ctx,
            &mut ntg.transfer,
            &[(buffer, upload_data)],
        );

        buffer
    };

    let vertex_attrib = {
        let info = nitrogen::vertex_attrib::VertexAttribInfo {
            buffer_infos: &[nitrogen::vertex_attrib::VertexAttribBufferInfo {
                index: 0,
                elements: &[
                    nitrogen::vertex_attrib::VertexAttribBufferElementInfo {
                        location: 0,
                        format: nitrogen::gfx::format::Format::Rg32Float,
                        offset: 0,
                    },
                    nitrogen::vertex_attrib::VertexAttribBufferElementInfo {
                        location: 0,
                        format: nitrogen::gfx::format::Format::Rg32Float,
                        offset: 8,
                    },
                ],
            }],
        };

        ntg.vertex_attribs_create(&[info]).remove(0)
    };

    let graph = setup_graphs(&mut ntg, vertex_attrib);

    let mut running = true;
    let mut resized = true;

    while running {
        events.poll_events(|event| match event {
            winit::Event::WindowEvent { event, .. } => match event {
                winit::WindowEvent::CloseRequested => {
                    running = false;
                }
                winit::WindowEvent::Resized(_size) => {
                    resized = true;
                }
                _ => {}
            },
            _ => {}
        });

        if resized {
            debug!("resize!");

            ntg.displays[display].setup_swapchain(&ntg.device_ctx);

            resized = false;
        }

        ntg.graph_compile(graph);
        /*
        if let Err(errs) = ntg.graph_compile(graph) {
            println!("Errors occured while compiling the old_graph");
            for err in errs {
                println!("{:?}", err);
            }
        }
        */

        let exec_context = nitrogen::graph::ExecutionContext {
            reference_size: (400, 400),
        };

        ntg.render_graph(graph, &exec_context);

        ntg.display_present(display, graph);


        // ntg.displays[display].present(&ntg.device_ctx, &ntg.image_storage, image, &ntg.sampler_storage, sampler);

        // running = false;
    }

    ntg.buffer_storage.destroy(&ntg.device_ctx, &[buffer]);

    ntg.sampler_storage.destroy(&ntg.device_ctx, &[sampler]);
    ntg.image_storage.destroy(&ntg.device_ctx, &[image]);

    ntg.release();
}

fn setup_graphs(
    ntg: &mut nitrogen::Context,
    vertex_attrib: nitrogen::vertex_attrib::VertexAttribHandle,
) -> graph::GraphHandle {
    let graph = ntg.graph_create();

    {
        let pass_info = nitrogen::graph::PassInfo::Graphics {
            vertex_attrib: None, //Some(vertex_attrib),
            shaders: nitrogen::graph::Shaders {
                vertex: nitrogen::graph::ShaderInfo {
                    content: Cow::Borrowed(include_bytes!(concat!(
                        env!("OUT_DIR"),
                        "/test.hlsl.vert.spirv"
                    ))),
                    entry: "VertexMain".into(),
                },
                fragment: Some(nitrogen::graph::ShaderInfo {
                    content: Cow::Borrowed(include_bytes!(concat!(
                        env!("OUT_DIR"),
                        "/test.hlsl.frag.spirv"
                    ))),
                    entry: "FragmentMain".into(),
                }),
                geometry: None,
            },
            primitive: nitrogen::pipeline::Primitive::TriangleList,
            blend_mode: nitrogen::render_pass::BlendMode::Alpha,
        };

        struct TestPass {};

        impl PassImpl for TestPass {
            fn setup(&mut self, builder: &mut graph::GraphBuilder) {
                let create_info = graph::ImageCreateInfo {
                    format: image::ImageFormat::RgbaUnorm,
                    size_mode: image::ImageSizeMode::ContextRelative {
                        width: 1.0,
                        height: 1.0,
                    },
                };
                builder.image_create("ImageTest", create_info);

                let create_info = graph::ImageCreateInfo {
                    format: image::ImageFormat::RgbaUnorm,
                    size_mode: image::ImageSizeMode::ContextRelative {
                        width: 1.0,
                        height: 1.0,
                    },
                };
                builder.image_create("ImageMask", create_info);

                builder.image_write_color("ImageTest", 0);
                builder.image_write_color("ImageMask", 1);

                builder.enable();

                /*
                let image_create = nitrogen::graph::ImageCreateInfo {
                    format: nitrogen::image::ImageFormat::RgbaUnorm,
                    size_mode: nitrogen::image::ImageSizeMode::ContextRelative {
                        width: 1.0,
                        height: 1.0,
                    },
                };

                builder.image_create("TestColor0".into(), image_create);
                builder.backbuffer_image("TestColor0".into());

                builder.image_write("TestColor0".into(), 0);

                builder.enable();
                */
            }

            fn execute(&self, command_buffer: &mut graph::CommandBuffer) {
                command_buffer.draw(0..3, 0..1)
            }
        }

        ntg.graph_add_pass(graph, "TestPass", pass_info, Box::new(TestPass {}));
    }

    {
        let pass_info = nitrogen::graph::PassInfo::Graphics {
            vertex_attrib: None, //Some(vertex_attrib),
            shaders: nitrogen::graph::Shaders {
                vertex: nitrogen::graph::ShaderInfo {
                    content: Cow::Borrowed(include_bytes!(concat!(
                        env!("OUT_DIR"),
                        "/test.hlsl.vert.spirv"
                    ))),
                    entry: "VertexMain".into(),
                },
                fragment: Some(nitrogen::graph::ShaderInfo {
                    content: Cow::Borrowed(include_bytes!(concat!(
                        env!("OUT_DIR"),
                        "/test.hlsl.frag.spirv"
                    ))),
                    entry: "FragmentMain".into(),
                }),
                geometry: None,
            },
            primitive: nitrogen::pipeline::Primitive::TriangleList,
            blend_mode: nitrogen::render_pass::BlendMode::Alpha,
        };

        struct TestPass {};

        impl PassImpl for TestPass {
            fn setup(&mut self, builder: &mut graph::GraphBuilder) {

                let create_info = graph::ImageCreateInfo {
                    format: image::ImageFormat::RgbaUnorm,
                    size_mode: image::ImageSizeMode::ContextRelative {
                        width: 1.0,
                        height: 1.0,
                    },
                };
                builder.image_create("ImageTestTinted", create_info);

                builder.image_move("ImageMask", "ImageMaskTinted");

                builder.image_write_color("ImageTestTinted", 0);
                builder.image_write_color("ImageMaskTinted", 1);

                builder.image_read_color("ImageTest", 0);

                builder.enable();
            }

            fn execute(&self, command_buffer: &mut graph::CommandBuffer) {
                command_buffer.draw(3..6, 0..1)
            }
        }

        ntg.graph_add_pass(graph, "TestPass2", pass_info, Box::new(TestPass {}));
    }

    ntg.graph_add_output_image(graph, "ImageTestTinted");

    graph
}
