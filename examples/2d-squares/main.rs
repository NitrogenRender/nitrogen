/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

extern crate nitrogen;
extern crate rand;
extern crate winit;

extern crate log;
extern crate env_logger;

use nitrogen::*;

use std::sync::mpsc::{channel, Receiver, Sender};

#[derive(Copy, Clone, Debug)]
#[repr(C)]
struct InstanceData {
    pos: [f32; 2],
    size: [f32; 2],
    color: [f32; 4],
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
struct VertexData {
    pos: [f32; 2],
}

const VERTEX_DATA: [VertexData; 4] = [
    VertexData { pos: [-1.0, -1.0] },
    VertexData { pos: [1.0, -1.0] },
    VertexData { pos: [-1.0, 1.0] },
    VertexData { pos: [1.0, 1.0] },
];

const NUM_THINGS: usize = 1;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();

    // boring window stuff

    let mut events_loop = winit::EventsLoop::new();
    let window = winit::Window::new(&events_loop)?;

    // cool and fun nitrogen stuff

    let mut ctx = Context::new("2d-squares", 1);
    let display = ctx.add_display(&window);

    let material = {
        let create_info = material::MaterialCreateInfo {
            parameters: &[(0, material::MaterialParameterType::UniformBuffer)],
        };
        ctx.material_create(&[create_info]).remove(0).unwrap()
    };

    let vtx_def = {
        let create_info = vertex_attrib::VertexAttribInfo {
            buffer_stride: ::std::mem::size_of::<VertexData>(),
            buffer_infos: &[vertex_attrib::VertexAttribBufferInfo {
                index: 0,
                elements: &[vertex_attrib::VertexAttribBufferElementInfo {
                    location: 0,
                    format: nitrogen::gfx::format::Format::Rg32Float,
                    offset: 0,
                }],
            }],
        };
        ctx.vertex_attribs_create(&[create_info]).remove(0)
    };

    let vertex_buffer = {
        use buffer::BufferUsage;
        use resources::MemoryProperties;

        let create_info = buffer::BufferCreateInfo {
            size: ::std::mem::size_of_val(&VERTEX_DATA) as u64,
            is_transient: false,
            properties: MemoryProperties::CPU_VISIBLE | MemoryProperties::COHERENT,
            usage: BufferUsage::TRANSFER_DST | BufferUsage::VERTEX,
        };
        let buffer = ctx.buffer_create(&[create_info]).remove(0).unwrap();

        let upload_info = buffer::BufferUploadInfo {
            offset: 0,
            data: &VERTEX_DATA,
        };

        ctx.buffer_upload_data(&[(buffer, upload_info)]);

        buffer
    };

    let num_squares = NUM_THINGS as _;

    let mut instance_data = create_instance_data(num_squares);
    let mut instance_vel = create_instance_velocity(num_squares);

    let instance_material = ctx.material_create_instance(&[material]).remove(0).unwrap();

    let instance_buffer = {
        use buffer::BufferUsage;
        use resources::MemoryProperties;

        let create_info = buffer::BufferCreateInfo {
            size: ::std::mem::size_of::<InstanceData>() as u64 * num_squares as u64,
            is_transient: false,
            properties: MemoryProperties::CPU_VISIBLE | MemoryProperties::COHERENT,
            usage: BufferUsage::TRANSFER_DST | BufferUsage::UNIFORM,
        };
        ctx.buffer_create(&[create_info]).remove(0).unwrap()
    };

    write_to_instance_buffer(&mut ctx, &instance_data, instance_buffer);

    {
        let write = material::InstanceWrite {
            binding: 0,
            data: material::InstanceWriteData::Buffer {
                buffer: instance_buffer,
                region: None..None,
            },
        };

        ctx.material_write_instance(instance_material, std::iter::once(write));
    }

    let (graph) = create_graph(&mut ctx, vtx_def, material, instance_material, vertex_buffer);

    let mut running = true;
    let mut resized = true;



    let mut exec_context = {
        let initial_size = window.get_inner_size().unwrap();
        graph::ExecutionContext {
            reference_size: (initial_size.width as u32, initial_size.height as u32),
        }
    };

    let mut submits = vec![
        ctx.create_submit_group(),
        ctx.create_submit_group(),
    ];

    let mut flights = Vec::with_capacity(submits.len());
    {
        for _ in 0..submits.len() {
            flights.push(None);
        }
    }

    let mut frame_num = 0;
    let mut frame_idx = 0;

    while running {
        events_loop.poll_events(|ev| match ev {
            winit::Event::WindowEvent { event, .. } => match event {
                winit::WindowEvent::CloseRequested => {
                    running = false;
                }
                winit::WindowEvent::Resized(size) => {

                    exec_context.reference_size = (size.width as u32, size.height as u32);

                    resized = true;
                }
                _ => {}
            },
            _ => {}
        });

        if resized {
            ctx.displays[display].setup_swapchain(&ctx.device_ctx);
        }

        // render stuff
        let res = ctx.graph_compile(graph);
        if let Err(err) = res {
            println!("{:?}", err);
            continue;
        }

        // wait for prev frame
        {
            let last_idx = (frame_num + (submits.len() - 1)) % submits.len();

            if let Some(res) = flights[last_idx].take() {
                submits[last_idx].wait(&mut ctx);
                ctx.graph_exec_resource_destroy(res);
            }
        }


        let res = {
            let res = submits[frame_idx].graph_render(&mut ctx, graph, &exec_context);

            submits[frame_idx].display_present(&mut ctx, display, &res);

            res
        };

        flights[frame_idx] = Some(res);

        update_instance_data(&mut instance_data, &mut instance_vel);

        write_to_instance_buffer(&mut ctx, &instance_data, instance_buffer);

        frame_num += 1;
        frame_idx = frame_num % submits.len();
    }

    for submit in &mut submits {
        submit.wait(&mut ctx);
    }

    for flight in flights {
        if let Some(res) = flight {
            ctx.graph_exec_resource_destroy(res);
        }
    }

    for submit in submits {
        submit.release(&mut ctx);
    }

    ctx.graph_destroy(graph);
    ctx.buffer_destroy(&[vertex_buffer, instance_buffer]);
    ctx.vertex_attribs_destroy(&[vtx_def]);
    ctx.material_destroy(&[material]);
    ctx.remove_display(display);

    ctx.release();

    Ok(())
}

fn create_graph(
    ctx: &mut Context,
    vertex_attrib: vertex_attrib::VertexAttribHandle,
    material: material::MaterialHandle,
    mat_instance: material::MaterialInstanceHandle,
    buffer: buffer::BufferHandle,
) -> graph::GraphHandle {
    use std::borrow::Cow;

    let graph = ctx.graph_create();

    {
        let info = graph::PassInfo::Graphics {
            vertex_attrib: Some(vertex_attrib),
            shaders: graph::Shaders {
                vertex: graph::ShaderInfo {
                    content: Cow::Borrowed(include_bytes!(concat!(
                        env!("OUT_DIR"),
                        "/2d-squares/quad.hlsl.vert.spirv"
                    ),)),
                    entry: "VertexMain".into(),
                },
                fragment: Some(graph::ShaderInfo {
                    content: Cow::Borrowed(include_bytes!(concat!(
                        env!("OUT_DIR"),
                        "/2d-squares/quad.hlsl.frag.spirv"
                    ),)),
                    entry: "FragmentMain".into(),
                }),
                geometry: None,
            },
            primitive: pipeline::Primitive::TriangleStrip,
            blend_mode: render_pass::BlendMode::Alpha,
            materials: vec![(1, material)],
        };

        struct Pass2D {
            buffer: buffer::BufferHandle,
            mat_instance: material::MaterialInstanceHandle,
        }

        impl graph::PassImpl for Pass2D {
            fn setup(&mut self, builder: &mut graph::GraphBuilder) {
                builder.image_create(
                    "Canvas",
                    graph::ImageCreateInfo {
                        size_mode: image::ImageSizeMode::ContextRelative {
                            width: 1.0,
                            height: 1.0,
                        },
                        format: image::ImageFormat::RgbaUnorm,
                    },
                );

                builder.image_write_color("Canvas", 0);

                builder.enable();
            }

            fn execute(&self, cmd: &mut graph::CommandBuffer) {
                let things = NUM_THINGS;

                cmd.bind_vertex_array(self.buffer);
                cmd.bind_graphics_descriptor_set(1, self.mat_instance);

                cmd.draw(0..4, 0..things as u32);
            }
        }

        let pass = Pass2D { buffer, mat_instance, };

        ctx.graph_add_pass(graph, "2D Pass", info, Box::new(pass));
    }

    ctx.graph_add_output(graph, "Canvas");

    graph
}

fn create_instance_data(num: u32) -> Vec<InstanceData> {
    use rand::{thread_rng, Rng};

    let mut rng = thread_rng();

    let mut result = Vec::with_capacity(num as usize);

    for _i in 0..num {
        let size = [rng.gen_range(0.05, 0.1), rng.gen_range(0.05, 0.1)];



        let pos = [
            rng.gen_range(-1.0, 1.0 - size[0]),
            rng.gen_range(-1.0, 1.0 - size[1]),
        ];

        let color = [
            rng.gen_range(0.3, 1.0),
            rng.gen_range(0.5, 1.0),
            rng.gen_range(0.8, 1.0),
            1.0,
        ];

        result.push(InstanceData { pos, size, color });
    }

    result
}

fn create_instance_velocity(num: u32) -> Vec<[f32; 2]> {
    use rand::{thread_rng, Rng};

    let mut rng = thread_rng();

    let mut result = Vec::with_capacity(num as usize);

    for _ in 0..num {
        let vel = [rng.gen_range(-0.05, 0.05), rng.gen_range(-0.05, 0.05)];
        result.push(vel);
    }

    result
}

fn update_instance_data(data: &mut [InstanceData], velocities: &mut [[f32; 2]]) {
    assert_eq!(data.len(), velocities.len());

    for i in 0..data.len() {

        let mut new_pos = [
            data[i].pos[0] + velocities[i][0],
            data[i].pos[1] + velocities[i][1],
        ];

        if new_pos[0] < -1.0 || new_pos[0] > 1.0 {
            new_pos[0] = new_pos[0].max(-1.0).min(1.0);
            velocities[i][0] *= -1.0;
        }
        if new_pos[1] < -1.0 || new_pos[1] > 1.0 {
            new_pos[1] = new_pos[1].max(-1.0).min(1.0);
            velocities[i][1] *= -1.0;
        }

        data[i].pos = new_pos;

    }
}

fn write_to_instance_buffer(
    ctx: &mut Context,
    data: &[InstanceData],
    buffer: buffer::BufferHandle,
) {
    let upload_info = buffer::BufferUploadInfo { offset: 0, data };
    ctx.buffer_upload_data(&[(buffer, upload_info)]);
}
