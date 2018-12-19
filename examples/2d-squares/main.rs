/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use std::time::Instant;

use nitrogen::*;

use nitrogen::submit_group::SubmitGroup;

struct Delta(pub f64);
struct Scale(pub f32);

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

const NUM_THINGS: usize = 1_024 * 1_024;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();

    // boring window stuff

    let mut events_loop = winit::EventsLoop::new();
    let window = winit::Window::new(&events_loop)?;

    // cool and fun nitrogen stuff

    let mut ctx = Context::new("2d-squares", 1);
    let display = ctx.display_add(&window);

    let mut submit = ctx.create_submit_group();

    let material = {
        let create_info = material::MaterialCreateInfo {
            parameters: &[
                // positions
                (0, material::MaterialParameterType::StorageBuffer),
                // velocities
                (1, material::MaterialParameterType::StorageBuffer),
            ],
        };
        ctx.material_create(&[create_info]).remove(0).unwrap()
    };

    let vtx_def = {
        let create_info = vertex_attrib::VertexAttribInfo {
            buffer_infos: &[vertex_attrib::VertexAttribBufferInfo {
                stride: ::std::mem::size_of::<VertexData>(),
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
        use crate::buffer::BufferUsage;

        create_buffer(
            &mut ctx,
            &mut submit,
            &VERTEX_DATA[..],
            BufferUsage::TRANSFER_DST | BufferUsage::VERTEX,
        )
    };

    let num_squares = NUM_THINGS as _;

    let instance_data = create_instance_data(num_squares);
    let instance_vel = create_instance_velocity(num_squares);

    let instance_material = ctx.material_create_instance(&[material]).remove(0).unwrap();

    let instance_buffer = {
        use crate::buffer::BufferUsage;

        create_buffer(
            &mut ctx,
            &mut submit,
            &instance_data[..],
            BufferUsage::TRANSFER_DST | BufferUsage::UNIFORM | BufferUsage::STORAGE,
        )
    };

    let velocity_buffer = {
        use crate::buffer::BufferUsage;

        create_buffer(
            &mut ctx,
            &mut submit,
            &instance_vel[..],
            BufferUsage::TRANSFER_DST | BufferUsage::STORAGE,
        )
    };

    submit.wait(&mut ctx);

    drop(instance_data);
    drop(instance_vel);

    {
        let writes = &[
            material::InstanceWrite {
                binding: 0,
                data: material::InstanceWriteData::Buffer {
                    buffer: instance_buffer,
                    region: None..None,
                },
            },
            material::InstanceWrite {
                binding: 1,
                data: material::InstanceWriteData::Buffer {
                    buffer: velocity_buffer,
                    region: None..None,
                },
            },
        ];

        ctx.material_write_instance(instance_material, writes);
    }

    let graph = create_graph(
        &mut ctx,
        vtx_def,
        material,
        instance_material,
        vertex_buffer,
    );

    let mut running = true;
    let mut resized = true;

    let mut exec_context = {
        let initial_size = window.get_inner_size().unwrap();
        graph::ExecutionContext {
            reference_size: (initial_size.width as u32, initial_size.height as u32),
        }
    };

    let mut submits = vec![submit, ctx.create_submit_group()];

    let mut frame_num = 0;
    let mut frame_idx = 0;

    let mut instant = Instant::now();

    let mut store = graph::Store::new();

    {
        let scale = 0.25 / 8.0;
        store.insert(Scale(scale));
    }

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

        // render stuff
        let res = ctx.graph_compile(graph);
        if let Err(err) = res {
            println!("{:?}", err);
            continue;
        }

        // wait for prev frame
        {
            let last_idx = frame_idx;

            submits[last_idx].wait(&mut ctx);
        }

        // update delta time

        let delta = {
            let new_instant = Instant::now();
            let dur = new_instant.duration_since(instant);
            instant = new_instant;

            const NANOS_PER_SEC: u32 = 1_000_000_000;

            let secs = dur.as_secs() as f64;
            let subsecs = dur.subsec_nanos() as f64 / NANOS_PER_SEC as f64;

            secs + subsecs
        };

        store.insert(Delta(delta));

        {
            if resized {
                submits[frame_idx].display_setup_swapchain(&mut ctx, display);
                resized = false;
            }

            submits[frame_idx].graph_execute(&mut ctx, graph, &store, &exec_context);

            let img = ctx.graph_get_output_image(graph, "Canvas").unwrap();

            submits[frame_idx].display_present(&mut ctx, display, img);
        }

        frame_num += 1;
        frame_idx = frame_num % submits.len();
    }

    submits[0].buffer_destroy(&mut ctx, &[vertex_buffer, instance_buffer, velocity_buffer]);
    submits[0].graph_destroy(&mut ctx, &[graph]);

    for mut submit in submits {
        submit.wait(&mut ctx);
        submit.release(&mut ctx);
    }

    ctx.vertex_attribs_destroy(&[vtx_def]);
    ctx.material_destroy(&[material]);
    ctx.display_remove(display);

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
        let info = graph::ComputePassInfo {
            shader: graph::ShaderInfo {
                content: Cow::Borrowed(include_bytes!(concat!(
                    env!("OUT_DIR"),
                    "/2d-squares/move.hlsl.comp.spirv"
                ),)),
                entry: "ComputeMain".into(),
            },
            materials: vec![(1, material)],
            push_constants: vec![(0..4)],
        };

        struct MovePass {
            mat_instance: material::MaterialInstanceHandle,
        }

        impl graph::ComputePassImpl for MovePass {
            fn setup(&mut self, builder: &mut graph::GraphBuilder) {
                builder.extern_create("Positions");

                builder.enable();
            }

            fn execute(&self, store: &graph::Store, cmd: &mut graph::ComputeCommandBuffer<'_>) {
                let mut batch_size = 1024;
                let mut wide = NUM_THINGS as u32 / batch_size;

                if (NUM_THINGS as u32 % batch_size) != 0 {
                    batch_size += 1;

                    if wide == 0 {
                        wide = 1;
                    }
                }

                cmd.push_constant::<u32>(0, wide);
                cmd.push_constant::<u32>(1, NUM_THINGS as u32);

                let Delta(delta) = store.get::<Delta>().unwrap();

                cmd.push_constant::<f32>(2, *delta as f32);

                cmd.bind_material(1, self.mat_instance);

                cmd.dispatch([wide, batch_size, 1]);
            }
        }

        let pass = MovePass { mat_instance };

        ctx.graph_add_compute_pass(graph, "MovePass", info, pass);
    }

    {
        let info = graph::GraphicsPassInfo {
            vertex_attrib: Some(vertex_attrib),
            depth_mode: None,
            stencil_mode: None,
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
            primitive: graph::Primitive::TriangleStrip,
            blend_modes: vec![graph::BlendMode::Alpha],
            materials: vec![(1, material)],
            push_constants: vec![(0..5)],
        };

        struct Pass2D {
            buffer: buffer::BufferHandle,
            mat_instance: material::MaterialInstanceHandle,
        }

        impl graph::GraphicsPassImpl for Pass2D {
            fn setup(&mut self, builder: &mut graph::GraphBuilder) {
                builder.extern_read("Positions");

                builder.image_create(
                    "Canvas",
                    graph::ImageCreateInfo {
                        size_mode: image::ImageSizeMode::ContextRelative {
                            width: 1.0,
                            height: 1.0,
                        },
                        format: image::ImageFormat::RgbaUnorm,
                        clear: graph::ImageClearValue::Color([0.1, 0.1, 0.2, 1.0]),
                    },
                );

                builder.image_write_color("Canvas", 0);

                builder.enable();
            }

            fn execute(&self, store: &graph::Store, cmd: &mut graph::GraphicsCommandBuffer<'_>) {
                let things = NUM_THINGS;

                cmd.push_constant::<[f32; 4]>(0, [1.0, 1.0, 1.0, 1.0]);

                let Scale(s) = store.get().unwrap();
                cmd.push_constant(4, *s);

                cmd.bind_vertex_buffers(&[(self.buffer, 0)]);
                cmd.bind_material(1, self.mat_instance);

                cmd.draw(0..4, 0..things as u32);
            }
        }

        let pass = Pass2D {
            buffer,
            mat_instance,
        };

        ctx.graph_add_graphics_pass(graph, "2D Pass", info, pass);
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

        let size = [size[0], size[0]];

        let pos = [
            rng.gen_range(-1.0, 1.0 - size[0]),
            rng.gen_range(-1.0, 1.0 - size[1]),
        ];

        let color = [
            rng.gen_range(0.4, 0.8),
            rng.gen_range(0.0, 0.7),
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
        let vel = [rng.gen_range(-0.5, 0.5), rng.gen_range(-0.5, 0.5)];
        result.push(vel);
    }

    result
}

fn create_buffer<T>(
    ctx: &mut Context,
    submit: &mut SubmitGroup,
    data: &[T],
    usages: buffer::BufferUsage,
) -> buffer::BufferHandle {
    let create_info = buffer::DeviceLocalCreateInfo {
        size: std::mem::size_of::<T>() as u64 * (data.len() as u64),
        is_transient: false,
        usage: usages,
    };

    let buffer = ctx
        .buffer_device_local_create(&[create_info])
        .remove(0)
        .unwrap();

    let upload_info = buffer::BufferUploadInfo { offset: 0, data };

    submit.buffer_device_local_upload(ctx, &[(buffer, upload_info)]);

    buffer
}
