/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use nitrogen_examples_common::{
    self as helper,
    main_loop::{MainLoop, UserData},
};

use nitrogen::{self as nit, buffer, graph, image, material, submit_group, vertex_attrib as vtx};

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

const NUM_THINGS: usize = 1_024 * 1_024 * 4;

struct Data2dSquares {
    graph: graph::GraphHandle,

    buf_instance: buffer::BufferHandle,
    buf_velocity: buffer::BufferHandle,
    buf_vertex: buffer::BufferHandle,

    vtx_def: vtx::VertexAttribHandle,

    mat: material::MaterialHandle,
}

impl UserData for Data2dSquares {
    fn iteration(&mut self, store: &mut graph::Store, delta: f64) {
        store.insert(Delta(delta));
    }

    fn graph(&self) -> graph::GraphHandle {
        self.graph
    }

    fn output_image(&self) -> graph::ResourceName {
        "Canvas".into()
    }

    fn release(self, ctx: &mut nit::Context, submit: &mut submit_group::SubmitGroup) {
        submit.graph_destroy(ctx, &[self.graph]);

        submit.buffer_destroy(
            ctx,
            &[self.buf_vertex, self.buf_instance, self.buf_velocity],
        );

        unsafe {
            submit.wait(ctx);

            ctx.material_destroy(&[self.mat]);
        }

        ctx.vertex_attribs_destroy(&[self.vtx_def]);
    }
}

fn init(
    store: &mut graph::Store,
    ctx: &mut nit::Context,
    submit: &mut submit_group::SubmitGroup,
) -> Option<Data2dSquares> {
    // create vertex attribute description

    let vtx_def = {
        let create_info = vtx::VertexAttribInfo {
            buffer_infos: &[vtx::VertexAttribBufferInfo {
                stride: ::std::mem::size_of::<VertexData>(),
                index: 0,
                elements: &[vtx::VertexAttribBufferElementInfo {
                    location: 0,
                    format: nit::gfx::format::Format::Rg32Float,
                    offset: 0,
                }],
            }],
        };
        ctx.vertex_attribs_create(&[create_info]).remove(0)
    };

    // create a bunch of buffers

    let vertex_buffer =
        unsafe { helper::resource::buffer_device_local_vertex(ctx, submit, &VERTEX_DATA[..])? };

    let (instance_buffer, velocity_buffer) = {
        let instance_data = create_instance_data();
        let instance_vel = create_instance_velocity();

        let instance_buffer = unsafe {
            helper::resource::buffer_device_local_create(
                ctx,
                submit,
                &instance_data[..],
                buffer::BufferUsage::UNIFORM | buffer::BufferUsage::STORAGE,
            )?
        };

        let velocity_buffer = unsafe {
            helper::resource::buffer_device_local_storage(ctx, submit, &instance_vel[..])?
        };

        unsafe {
            submit.wait(ctx);
        }

        (instance_buffer, velocity_buffer)
    };

    // create material definition and an instance

    let material = unsafe {
        let create_info = material::MaterialCreateInfo {
            parameters: &[
                // positions
                (0, material::MaterialParameterType::StorageBuffer),
                // velocities
                (1, material::MaterialParameterType::StorageBuffer),
            ],
        };
        ctx.material_create(&[create_info]).remove(0).ok()?
    };

    let instance_material = unsafe { ctx.material_create_instance(&[material]).remove(0).ok()? };

    // write to instance
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

        unsafe {
            ctx.material_write_instance(instance_material, writes);
        }
    }

    let graph = create_graph(ctx, vtx_def, material, instance_material, vertex_buffer);

    // write initial scale to the store
    {
        let scale = 0.25 / 8.0;
        store.insert(Scale(scale));
    }

    store.insert(Delta(0.00001));

    Some(Data2dSquares {
        graph,

        buf_instance: instance_buffer,
        buf_velocity: velocity_buffer,
        buf_vertex: vertex_buffer,

        vtx_def,

        mat: material,
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();

    // boring window stuff
    let mut ml = unsafe { MainLoop::new("2d-squares", init).unwrap() };

    while ml.running() {
        unsafe {
            ml.iterate();
        }
    }

    unsafe {
        ml.release();
    }

    Ok(())
}

fn create_graph(
    ctx: &mut nit::Context,
    vertex_attrib: vtx::VertexAttribHandle,
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

                unsafe {
                    cmd.push_constant::<u32>(0, wide);
                    cmd.push_constant::<u32>(1, NUM_THINGS as u32);

                    let Delta(delta) = store.get::<Delta>().unwrap();

                    cmd.push_constant::<f32>(2, *delta as f32);

                    cmd.bind_material(1, self.mat_instance);

                    cmd.dispatch([wide, batch_size, 1]);
                }
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

                unsafe {
                    cmd.push_constant::<[f32; 4]>(0, [1.0, 1.0, 1.0, 1.0]);

                    let Scale(s) = store.get().unwrap();
                    cmd.push_constant(4, *s);

                    cmd.bind_vertex_buffers(&[(self.buffer, 0)]);
                    cmd.bind_material(1, self.mat_instance);

                    cmd.draw(0..4, 0..things as u32);
                }
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

fn create_instance_data() -> Vec<InstanceData> {
    use rand::{thread_rng, Rng};

    let mut rng = thread_rng();

    let mut result = Vec::with_capacity(NUM_THINGS);

    for _ in 0..NUM_THINGS {
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

fn create_instance_velocity() -> Vec<[f32; 2]> {
    use rand::{thread_rng, Rng};

    let mut rng = thread_rng();

    let mut result = Vec::with_capacity(NUM_THINGS);

    for _ in 0..NUM_THINGS {
        let vel = [rng.gen_range(-0.5, 0.5), rng.gen_range(-0.5, 0.5)];
        result.push(vel);
    }

    result
}
