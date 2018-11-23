/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

extern crate env_logger;
extern crate image as img;
extern crate log;
extern crate nitrogen;
extern crate winit;

use nitrogen::graph;
use nitrogen::image;
use nitrogen::graph::PassImpl;

use log::debug;

use std::borrow::Cow;

#[derive(Debug, Clone, Copy)]
struct Vertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
}

const TRIANGLE: [Vertex; 4] = [
    Vertex {
        pos: [-1.0, -1.0],
        uv: [0.0, 0.0],
    }, // LEFT TOP
    Vertex {
        pos: [-1.0, 1.0],
        uv: [0.0, 1.0],
    }, // LEFT BOTTOM
    Vertex {
        pos: [1.0, -1.0],
        uv: [1.0, 0.0],
    }, // RIGHT TOP
    Vertex {
        pos: [1.0, 1.0],
        uv: [1.0, 1.0],
    }, // RIGHT BOTTOM
];

fn main() {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();

    let mut events = winit::EventsLoop::new();
    let window = winit::Window::new(&events).unwrap();

    let mut ntg = nitrogen::Context::new("nitrogen test", 1);

    let display = ntg.add_display(&window);

    let material = {
        let create_info = nitrogen::material::MaterialCreateInfo {
            parameters: &[
                (0, nitrogen::material::MaterialParameterType::SampledImage),
                (1, nitrogen::material::MaterialParameterType::Sampler),
                (2, nitrogen::material::MaterialParameterType::UniformBuffer),
            ],
        };

        ntg.material_create(&[create_info]).remove(0).unwrap()
    };

    let mat_example_instance = { ntg.material_create_instance(&[material]).remove(0).unwrap() };

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
            usage: nitrogen::buffer::BufferUsage::TRANSFER_SRC
                | nitrogen::buffer::BufferUsage::VERTEX,
            properties: nitrogen::resources::MemoryProperties::CPU_VISIBLE
                | nitrogen::resources::MemoryProperties::COHERENT,
        };
        let buffer = ntg.buffer_create(&[create_info]).remove(0).unwrap();

        let upload_data = nitrogen::buffer::BufferUploadInfo {
            offset: 0,
            data: &TRIANGLE,
        };

        let result = ntg.buffer_upload_data(&[(buffer, upload_data)]).remove(0);

        println!("{:?}", result);

        buffer
    };

    let vertex_attrib = {
        let info = nitrogen::vertex_attrib::VertexAttribInfo {
            buffer_stride: std::mem::size_of::<Vertex>(),
            buffer_infos: &[nitrogen::vertex_attrib::VertexAttribBufferInfo {
                index: 0,
                elements: &[
                    nitrogen::vertex_attrib::VertexAttribBufferElementInfo {
                        location: 0,
                        format: nitrogen::gfx::format::Format::Rg32Float,
                        offset: 0,
                    },
                    nitrogen::vertex_attrib::VertexAttribBufferElementInfo {
                        location: 1,
                        format: nitrogen::gfx::format::Format::Rg32Float,
                        offset: 8,
                    },
                ],
            }],
        };

        ntg.vertex_attribs_create(&[info]).remove(0)
    };

    let graph = setup_graphs(
        &mut ntg,
        vertex_attrib,
        buffer,
        material,
        mat_example_instance,
    );

    let mut running = true;
    let mut resized = true;

    #[derive(Copy, Clone)]
    struct UniformData {
        color: [f32; 4],
    }

    let uniform_data = UniformData {
        color: [0.3, 0.5, 1.0, 1.0],
    };

    let uniform_buffer = {
        let create_info = nitrogen::buffer::BufferCreateInfo {
            size: std::mem::size_of::<UniformData>() as u64,
            is_transient: false,
            usage: nitrogen::buffer::BufferUsage::TRANSFER_SRC
                | nitrogen::buffer::BufferUsage::UNIFORM,
            properties: nitrogen::resources::MemoryProperties::CPU_VISIBLE
                | nitrogen::resources::MemoryProperties::COHERENT,
        };
        let buffer = ntg.buffer_create(&[create_info]).remove(0).unwrap();

        let upload_data = nitrogen::buffer::BufferUploadInfo {
            offset: 0,
            data: &[uniform_data],
        };

        let result = ntg.buffer_upload_data(&[(buffer, upload_data)]).remove(0);

        println!("{:?}", result);

        buffer
    };

    {
        ntg.material_write_instance(
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
                nitrogen::material::InstanceWrite {
                    binding: 2,
                    data: nitrogen::material::InstanceWriteData::Buffer {
                        buffer: uniform_buffer,
                        region: None..None,
                    },
                },
            ],
        );
    }

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

        // ntg.graph_compile(graph);
        if let Err(errs) = ntg.graph_compile(graph) {
            println!("Errors occured while compiling the old_graph");
            println!("{:?}", errs);
        }

        let exec_context = nitrogen::graph::ExecutionContext {
            reference_size: (1920, 1080),
        };

        let res = ntg.render_graph(graph, &exec_context);

        ntg.display_present(display, &res);

        ntg.graph_exec_resource_destroy(res);
    }

    ntg.graph_destroy(graph);

    ntg.buffer_destroy(&[buffer, uniform_buffer]);

    ntg.sampler_destroy(&[sampler]);
    ntg.image_destroy(&[image]);

    ntg.release();
}

fn setup_graphs(
    ntg: &mut nitrogen::Context,
    vertex_attrib: nitrogen::vertex_attrib::VertexAttribHandle,
    buffer: nitrogen::buffer::BufferHandle,
    material: nitrogen::material::MaterialHandle,
    material_instance: nitrogen::material::MaterialInstanceHandle,
) -> graph::GraphHandle {
    let graph = ntg.graph_create();

    fn image_create_info() -> graph::ImageCreateInfo {
        graph::ImageCreateInfo {
            format: image::ImageFormat::RgbaUnorm,
            size_mode: image::ImageSizeMode::ContextRelative {
                width: 1.0,
                height: 1.0,
            },
        }
    }

    {
        let shaders = nitrogen::graph::Shaders {
            vertex: nitrogen::graph::ShaderInfo {
                content: Cow::Borrowed(include_bytes!(concat!(env!("OUT_DIR"), "/two-pass/test.hlsl.vert.spirv"))),
                entry: "VertexMain".into(),
            },
            fragment: Some(nitrogen::graph::ShaderInfo {
                content: Cow::Borrowed(include_bytes!(concat!(env!("OUT_DIR"), "/two-pass/test.hlsl.frag.spirv"))),
                entry: "FragmentMain".into(),
            }),
            geometry: None,
        };

        let (pass_impl, info) = create_test_pass(
            shaders,
            |builder| {
                builder.image_create("ITest", image_create_info());

                builder.image_write_color("ITest", 0);

                builder.enable();
            },
            move |cmd| {
                cmd.bind_vertex_array(buffer);

                cmd.bind_graphics_descriptor_set(1, material_instance);

                cmd.draw(0..4, 0..1);
            },
            Some(vertex_attrib),
            vec![(1, material)],
        );

        ntg.graph_add_pass(graph, "TestPass", info, Box::new(pass_impl));
    }

    {
        let shaders = nitrogen::graph::Shaders {
            vertex: nitrogen::graph::ShaderInfo {
                content: Cow::Borrowed(include_bytes!(concat!(env!("OUT_DIR"), "/two-pass/read.hlsl.vert.spirv"))),
                entry: "VertexMain".into(),
            },
            fragment: Some(nitrogen::graph::ShaderInfo {
                content: Cow::Borrowed(include_bytes!(concat!(env!("OUT_DIR"), "/two-pass/read.hlsl.frag.spirv"))),
                entry: "FragmentMain".into(),
            }),
            geometry: None,
        };

        let (pass_impl, info) = create_test_pass(
            shaders,
            |builder| {

                builder.image_create("IOutput", image_create_info());

                builder.image_write_color("IOutput", 0);

                builder.image_read_color("ITest", 0, 1);

                builder.enable();
            },
            move |cmd| {
                cmd.bind_vertex_array(buffer);

                cmd.draw(0..4, 0..1);
            },
            Some(vertex_attrib),
            vec![],
        );

        ntg.graph_add_pass(graph, "ReadPass", info, Box::new(pass_impl));
    }

    ntg.graph_add_output(graph, "IOutput");

    graph
}

fn create_test_pass<FSetUp, FExec>(
    shaders: nitrogen::graph::Shaders,
    setup: FSetUp,
    execute: FExec,
    vert: Option<nitrogen::vertex_attrib::VertexAttribHandle>,
    materials: Vec<(usize, nitrogen::material::MaterialHandle)>,
) -> (impl PassImpl, graph::PassInfo)
where
    FSetUp: FnMut(&mut graph::GraphBuilder),
    FExec: Fn(&mut graph::CommandBuffer),
{
    let pass_info = nitrogen::graph::PassInfo::Graphics {
        vertex_attrib: vert,
        shaders,
        primitive: nitrogen::pipeline::Primitive::TriangleStrip,
        blend_mode: nitrogen::render_pass::BlendMode::Alpha,
        materials,
    };

    struct TestPass<FSetUp, FExec> {
        setup: FSetUp,
        exec: FExec,
    }

    impl<FSetUp, FExec> PassImpl for TestPass<FSetUp, FExec>
    where
        FSetUp: FnMut(&mut graph::GraphBuilder),
        FExec: Fn(&mut graph::CommandBuffer),
    {
        fn setup(&mut self, builder: &mut graph::GraphBuilder) {
            (self.setup)(builder);
        }

        fn execute(&self, command_buffer: &mut graph::CommandBuffer) {
            (self.exec)(command_buffer);
        }
    }

    let pass = TestPass {
        setup,
        exec: execute,
    };

    (pass, pass_info)
}
