/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

extern crate image as img;

use nitrogen::graph;
use nitrogen::graph::GraphicsPassImpl;
use nitrogen::image;

use log::debug;

use std::borrow::Cow;

const TRIANGLE_POS: [[f32; 2]; 4] = [
    [-1.0, -1.0], // LEFT TOP
    [-1.0, 1.0],  // LEFT BOTTOM
    [1.0, -1.0],  // RIGHT TOP
    [1.0, 1.0],   // RIGHT BOTTOM
];

const TRIANGLE_UV: [[f32; 2]; 4] = [
    [0.0, 0.0], // LEFT TOP
    [0.0, 1.0], // LEFT BOTTOM
    [1.0, 0.0], // RIGHT TOP
    [1.0, 1.0], // RIGHT BOTTOM
];

fn main() {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();

    let mut events = winit::EventsLoop::new();
    let window = winit::Window::new(&events).unwrap();

    let mut ntg = unsafe { nitrogen::Context::new("nitrogen test", 1) };

    let mut submit = unsafe { ntg.create_submit_group() };

    let display = ntg.display_add(&window);

    let material = unsafe {
        let create_info = nitrogen::material::MaterialCreateInfo {
            parameters: &[
                (0, nitrogen::material::MaterialParameterType::SampledImage),
                (1, nitrogen::material::MaterialParameterType::Sampler),
                (2, nitrogen::material::MaterialParameterType::UniformBuffer),
            ],
        };

        ntg.material_create(&[create_info]).remove(0).unwrap()
    };

    let mat_example_instance =
        unsafe { ntg.material_create_instance(&[material]).remove(0).unwrap() };

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

        let img = unsafe { ntg.image_create(create_info).unwrap() };

        debug!("width {}, height {}", width, height);

        unsafe {
            let data = image::ImageUploadInfo {
                data: &(*image),
                format: image::ImageFormat::RgbaUnorm,
                dimension,
                target_offset: (0, 0, 0),
            };

            submit.image_upload_data(&mut ntg, img, data).unwrap()
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

            ntg.sampler_create(&[sampler_create]).remove(0)
        };

        (img, sampler)
    };

    unsafe {
        submit.display_setup_swapchain(&mut ntg, display);
    }

    let buffer_pos = unsafe {
        let create_info = nitrogen::buffer::CpuVisibleCreateInfo {
            size: std::mem::size_of_val(&TRIANGLE_POS) as u64,
            is_transient: false,
            usage: nitrogen::buffer::BufferUsage::TRANSFER_SRC
                | nitrogen::buffer::BufferUsage::VERTEX,
        };
        let buffer = ntg
            .buffer_cpu_visible_create(&[create_info])
            .remove(0)
            .unwrap();

        let upload_data = nitrogen::buffer::BufferUploadInfo {
            offset: 0,
            data: &TRIANGLE_POS,
        };

        submit
            .buffer_cpu_visible_upload(&mut ntg, &[(buffer, upload_data)])
            .remove(0)
            .unwrap();

        buffer
    };

    let buffer_uv = unsafe {
        let create_info = nitrogen::buffer::CpuVisibleCreateInfo {
            size: std::mem::size_of_val(&TRIANGLE_UV) as u64,
            is_transient: false,
            usage: nitrogen::buffer::BufferUsage::TRANSFER_SRC
                | nitrogen::buffer::BufferUsage::VERTEX,
        };
        let buffer = ntg
            .buffer_cpu_visible_create(&[create_info])
            .remove(0)
            .unwrap();

        let upload_data = nitrogen::buffer::BufferUploadInfo {
            offset: 0,
            data: &TRIANGLE_UV,
        };

        submit
            .buffer_cpu_visible_upload(&mut ntg, &[(buffer, upload_data)])
            .remove(0)
            .unwrap();

        buffer
    };

    let vertex_attrib = {
        let info = nitrogen::vertex_attrib::VertexAttribInfo {
            buffer_infos: &[
                // pos
                nitrogen::vertex_attrib::VertexAttribBufferInfo {
                    stride: std::mem::size_of::<[f32; 2]>(),
                    index: 0,
                    elements: &[nitrogen::vertex_attrib::VertexAttribBufferElementInfo {
                        location: 0,
                        format: nitrogen::gfx::format::Format::Rg32Float,
                        offset: 0,
                    }],
                },
                // uv
                nitrogen::vertex_attrib::VertexAttribBufferInfo {
                    stride: std::mem::size_of::<[f32; 2]>(),
                    index: 1,
                    elements: &[nitrogen::vertex_attrib::VertexAttribBufferElementInfo {
                        location: 1,
                        format: nitrogen::gfx::format::Format::Rg32Float,
                        offset: 0,
                    }],
                },
            ],
        };

        ntg.vertex_attribs_create(&[info]).remove(0)
    };

    let graph = setup_graphs(
        &mut ntg,
        Some(vertex_attrib),
        buffer_pos,
        buffer_uv,
        material,
        mat_example_instance,
    );

    let mut running = true;
    let mut resized = true;

    #[derive(Copy, Clone)]
    struct UniformData {
        _color: [f32; 4],
    }

    let uniform_data = UniformData {
        _color: [0.3, 0.5, 1.0, 1.0],
    };

    let uniform_buffer = unsafe {
        let create_info = nitrogen::buffer::CpuVisibleCreateInfo {
            size: std::mem::size_of::<UniformData>() as u64,
            is_transient: false,
            usage: nitrogen::buffer::BufferUsage::TRANSFER_SRC
                | nitrogen::buffer::BufferUsage::UNIFORM,
        };
        let buffer = ntg
            .buffer_cpu_visible_create(&[create_info])
            .remove(0)
            .unwrap();

        let upload_data = nitrogen::buffer::BufferUploadInfo {
            offset: 0,
            data: &[uniform_data],
        };

        submit
            .buffer_cpu_visible_upload(&mut ntg, &[(buffer, upload_data)])
            .remove(0)
            .unwrap();

        buffer
    };

    unsafe {
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

    let mut submits = vec![submit, unsafe { ntg.create_submit_group() }];

    let mut frame_num = 0;
    let mut frame_idx = 0;

    let exec_context = nitrogen::graph::ExecutionContext {
        reference_size: (1920, 1080),
    };

    let mut store = graph::Store::new();

    let mut backbuffer = graph::Backbuffer::new();

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

        // ntg.graph_compile(graph);
        if let Err(errs) = ntg.graph_compile(graph, &mut backbuffer, &mut store) {
            println!("Errors occured while compiling the graph");
            println!("{:?}", errs);
        }

        // wait for previous frame
        {
            let last_idx = (frame_num + (submits.len() - 1)) % submits.len();

            unsafe {
                submits[last_idx].wait(&mut ntg);
            }
        }

        unsafe {
            if resized {
                submits[frame_idx].display_setup_swapchain(&mut ntg, display);
                resized = false;
            }

            submits[frame_idx].graph_execute(
                &mut ntg,
                &mut backbuffer,
                graph,
                &store,
                &exec_context,
            );

            let img = submits[frame_idx]
                .graph_get_image(&ntg, graph, "IOutput")
                .unwrap();

            submits[frame_idx].display_present(&mut ntg, display, img);
        }

        frame_num += 1;
        frame_idx = frame_num % submits.len();
    }

    unsafe {
        ntg.wait_idle();
    }

    submits[0].backbuffer_destroy(&mut ntg, backbuffer);
    submits[0].buffer_destroy(&mut ntg, &[buffer_pos, buffer_uv, uniform_buffer]);
    submits[0].image_destroy(&mut ntg, &[image]);
    submits[0].sampler_destroy(&mut ntg, &[sampler]);
    submits[0].graph_destroy(&mut ntg, &[graph]);
    submits[0].material_destroy(&[material]);

    for mut submit in submits {
        unsafe {
            submit.wait(&mut ntg);
            submit.release(&mut ntg);
        }
    }

    unsafe {
        ntg.release();
    }
}

fn setup_graphs(
    ntg: &mut nitrogen::Context,
    vertex_attrib: Option<nitrogen::vertex_attrib::VertexAttribHandle>,
    buffer_pos: nitrogen::buffer::BufferHandle,
    buffer_uv: nitrogen::buffer::BufferHandle,
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
                content: Cow::Borrowed(include_bytes!(concat!(
                    env!("OUT_DIR"),
                    "/two-pass/test.hlsl.vert.spirv"
                ))),
                entry: "VertexMain".into(),
            },
            fragment: Some(nitrogen::graph::ShaderInfo {
                content: Cow::Borrowed(include_bytes!(concat!(
                    env!("OUT_DIR"),
                    "/two-pass/test.hlsl.frag.spirv"
                ))),
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
            move |cmd| unsafe {
                cmd.bind_vertex_buffers(&[(buffer_pos, 0), (buffer_uv, 0)]);

                cmd.bind_material(0, material_instance);

                cmd.draw(0..4, 0..1);
            },
            vertex_attrib.clone(),
            vec![(0, material)],
        );

        ntg.graph_add_graphics_pass(graph, "TestPass", info, pass_impl);
    }

    {
        let shaders = nitrogen::graph::Shaders {
            vertex: nitrogen::graph::ShaderInfo {
                content: Cow::Borrowed(include_bytes!(concat!(
                    env!("OUT_DIR"),
                    "/two-pass/read.hlsl.vert.spirv"
                ))),
                entry: "VertexMain".into(),
            },
            fragment: Some(nitrogen::graph::ShaderInfo {
                content: Cow::Borrowed(include_bytes!(concat!(
                    env!("OUT_DIR"),
                    "/two-pass/read.hlsl.frag.spirv"
                ))),
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
            move |cmd| unsafe {
                cmd.bind_vertex_buffers(&[(buffer_pos, 0), (buffer_uv, 0)]);

                cmd.draw(0..4, 0..1);
            },
            vertex_attrib,
            vec![],
        );

        ntg.graph_add_graphics_pass(graph, "ReadPass", info, pass_impl);
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
) -> (impl GraphicsPassImpl, graph::GraphicsPassInfo)
where
    FSetUp: FnMut(&mut graph::GraphBuilder),
    FExec: Fn(&mut graph::RenderPassEncoder),
{
    let pass_info = nitrogen::graph::GraphicsPassInfo {
        vertex_attrib: vert,
        depth_mode: None,
        stencil_mode: None,
        shaders,
        primitive: nitrogen::graph::Primitive::TriangleStrip,
        blend_modes: vec![nitrogen::graph::BlendMode::Alpha],
        materials,
        push_constants: vec![],
    };

    struct TestPass<FSetUp, FExec> {
        setup: FSetUp,
        exec: FExec,
    }

    impl<FSetUp, FExec> GraphicsPassImpl for TestPass<FSetUp, FExec>
    where
        FSetUp: FnMut(&mut graph::GraphBuilder),
        FExec: Fn(&mut graph::RenderPassEncoder),
    {
        fn setup(&mut self, _: &mut graph::Store, builder: &mut graph::GraphBuilder) {
            (self.setup)(builder);
        }

        fn execute(&self, _: &graph::Store, command_buffer: &mut graph::GraphicsCommandBuffer) {
            let mut cmd = unsafe {
                command_buffer
                    .begin_render_pass(&[graph::ImageClearValue::Color([0.0, 0.0, 0.0, 1.0])])
                    .unwrap()
            };
            (self.exec)(&mut cmd);
        }
    }

    let pass = TestPass {
        setup,
        exec: execute,
    };

    (pass, pass_info)
}
