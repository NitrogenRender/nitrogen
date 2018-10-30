use nitrogen::graph::PassImpl;

extern crate image as img;
extern crate nitrogen;
extern crate winit;

use nitrogen::graph;
use nitrogen::image;

#[macro_use]
extern crate log;

fn main() {
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

            used_as_transfer_dst: true,
            used_for_sampling: true,
            ..Default::default()
        };

        let img = ntg.image_create(&[create_info])
            .remove(0)
            .unwrap();

        debug!("width {}, height {}", width, height);

        {
            let data = image::ImageUploadInfo {
                data: &(*image),
                format: image::ImageFormat::RgbaUnorm,
                dimension,
                target_offset: (0, 0, 0),
            };

            ntg.image_upload_data(&[(img, data)])
                .remove(0)
                .unwrap()
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

            ntg.sampler_create(&[sampler_create])
                .remove(0)
        };

        (img, sampler)
    };

    ntg.displays[display].setup_swapchain(&ntg.device_ctx);

    let buffer = {
        let create_info = nitrogen::buffer::BufferCreateInfo {
            size: 64,
            is_transient: false,
            usage: nitrogen::buffer::BufferUsage::TRANSFER_SRC,
            properties: nitrogen::resources::MemoryProperties::DEVICE_LOCAL,
        };
        ntg.buffer_storage
            .create(&ntg.device_ctx, &[create_info])
            .remove(0)
            .unwrap()
    };

    {
        let upload_data = nitrogen::buffer::BufferUploadInfo {
            offset: 0,
            data: &[],
        };

        ntg.buffer_storage.upload_data(
            &ntg.device_ctx,
            &mut ntg.transfer,
            &[(buffer, upload_data)],
        );
    }

    let graph = setup_graphs(&mut ntg);

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

        let graph_constructed = ntg.graph_construct(graph);

        ntg.displays[display].present(
            &ntg.device_ctx,
            &ntg.image_storage,
            image,
            &ntg.sampler_storage,
            sampler,
        );
    }

    ntg.buffer_storage.destroy(&ntg.device_ctx, &[buffer]);

    ntg.sampler_storage.destroy(&ntg.device_ctx, &[sampler]);
    ntg.image_storage.destroy(&ntg.device_ctx, &[image]);

    ntg.release();
}

fn setup_graphs(ntg: &mut nitrogen::Context) -> graph::GraphHandle {
    let graph = ntg.graph_create();

    {
        let pass_info = nitrogen::graph::PassInfo::Graphics {
            vertex_desc: None,
            shaders: nitrogen::graph::Shaders {
                vertex: include_str!("../shaders/test.vert").into(),
                fragment: Some(include_str!("../shaders/test.frag").into()),
            },
            primitive: nitrogen::pipeline::Primitive::TriangleList,
            blend_mode: nitrogen::render_pass::BlendMode::Alpha,
        };

        struct TestPass {};

        impl PassImpl for TestPass {
            fn setup(&mut self, builder: &mut graph::GraphBuilder) {
                let image_create = nitrogen::graph::ImageCreateInfo {
                    format: nitrogen::image::ImageFormat::RgbaUnorm,
                    size_mode: nitrogen::image::ImageSizeMode::SwapChainRelative {
                        width: 1.0,
                        height: 1.0,
                    }
                };

                builder.create_image("TestColor", image_create);

                builder.write_image("TestColor");

                builder.enable();
            }

            fn execute(&self, command_buffer: &mut graph::CommandBuffer) {
                command_buffer.draw(0..6, 0..1)
            }
        }

        ntg.graph_add_pass(graph, "TestPass", pass_info, Box::new(TestPass {}));

        ntg.graph_add_output_image(graph, "TestColor");
    };

    graph

    /*
    var pass = NtgDefaultPasses.create(NtgPasses.SCREEN_IMAGE)
    pass.fragment = "shader code ........."
    pass.set_input("modulate", color.red)
    pass.set_output(mygraph.outputs.get("screen_texture"))
    pass.set_uniform_input(mygraph.outputs.get("3d_world_rendered"), "my_uniform_name")
    pass.set_instance_data("vertex_buffer", my_vert_buffer)
    my_graph.add_pass(pass)


    my_object.subscribe_to_pass(get_graph().get_pass("albedo"))
    */

    // bind_shader()
    // bind_quad()
    // render()
    // blit_to_screen()

    /*

    class my_screen_pass{
        func exposes():
            return string ("vertex", "Vec3"; "uv", vec2)

    func bind_inputs("name", value){
        inputs[name] = value
    }

    func bind_outputs(){...}

    func execute_pass(){
        for uniforms in uniformarray:
            ntg::bind(uniform, value)
        for instance in instancedataaray:
            ntg::bind\
        //horrible graphics code goes here



    }
    }
    */
}
