/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use std::borrow::Cow;

use nitrogen::graph::*;
use nitrogen::*;

const NUM_ELEMS: u64 = 32;

fn main() {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();

    let mut ctx = Context::new("compute example", 1);

    let mut submit = ctx.create_submit_group();

    let material = {
        let create_info = material::MaterialCreateInfo {
            parameters: &[(0, material::MaterialParameterType::StorageBuffer)],
        };
        ctx.material_create(&[create_info]).remove(0).unwrap()
    };

    let buffer = {
        let mut buffer_data: [f32; NUM_ELEMS as usize] = unsafe { std::mem::uninitialized() };
        // fill buffer
        {
            for i in 0..NUM_ELEMS {
                buffer_data[i as usize] = i as f32;
            }
        }

        println!("input  {:?}", &buffer_data[..]);

        let create_info = buffer::CpuVisibleCreateInfo {
            size: std::mem::size_of::<f32>() as u64 * NUM_ELEMS,
            is_transient: false,
            usage: buffer::BufferUsage::TRANSFER_SRC
                | buffer::BufferUsage::TRANSFER_DST
                | buffer::BufferUsage::UNIFORM,
        };

        let buffer = ctx
            .buffer_cpu_visible_create(&[create_info])
            .remove(0)
            .unwrap();

        let upload_data = buffer::BufferUploadInfo {
            offset: 0,
            data: &buffer_data[..],
        };

        submit
            .buffer_cpu_visible_upload(&mut ctx, &[(buffer, upload_data)])
            .remove(0)
            .unwrap();

        submit.wait(&mut ctx);

        buffer
    };

    let material_instance = ctx.material_create_instance(&[material]).remove(0).unwrap();

    ctx.material_write_instance(
        material_instance,
        &[material::InstanceWrite {
            binding: 0,
            data: material::InstanceWriteData::Buffer {
                buffer,
                region: None..None,
            },
        }],
    );

    let graph = create_graph(&mut ctx, material_instance);

    let store = Store::new();

    let _res = ctx.graph_compile(graph);

    submit.graph_execute(
        &mut ctx,
        graph,
        &store,
        &ExecutionContext {
            reference_size: (1, 1),
        },
    );

    submit.wait(&mut ctx);

    {
        let mut out: [f32; NUM_ELEMS as usize] = unsafe { std::mem::uninitialized() };

        submit.buffer_cpu_visible_read(&ctx, buffer, &mut out[..]);

        submit.wait(&mut ctx);

        println!("output {:?}", &out[..]);
    }

    submit.buffer_destroy(&mut ctx, &[buffer]);
    submit.graph_destroy(&mut ctx, &[graph]);

    submit.wait(&mut ctx);

    submit.release(&mut ctx);

    ctx.material_destroy(&[material]);
    ctx.release();
}

fn create_graph(
    ctx: &mut Context,
    material_instance: material::MaterialInstanceHandle,
) -> GraphHandle {
    let graph = ctx.graph_create();

    {
        let info = ComputePassInfo {
            shader: ShaderInfo {
                entry: "ComputeMain".into(),
                content: Cow::Borrowed(include_bytes!(concat!(
                    env!("OUT_DIR"),
                    "/compute/add.hlsl.comp.spirv"
                ),)),
            },
            materials: vec![(1, material_instance.0)],
            push_constants: vec![0..1],
        };

        struct Adder {
            mat: material::MaterialInstanceHandle,
        }

        impl ComputePassImpl for Adder {
            fn setup(&mut self, builder: &mut GraphBuilder) {
                builder.extern_create("Test");

                builder.enable();
            }

            fn execute(&self, _: &Store, command_buffer: &mut ComputeCommandBuffer<'_>) {
                command_buffer.bind_material(1, self.mat);
                command_buffer.push_constant(0, 1_f32);

                command_buffer.dispatch([NUM_ELEMS as _, 1, 1]);
            }
        }

        let adder = Adder {
            mat: material_instance,
        };

        ctx.graph_add_compute_pass(graph, "AddFirst", info, adder);
    }

    {
        let info = ComputePassInfo {
            shader: ShaderInfo {
                entry: "ComputeMain".into(),
                content: Cow::Borrowed(include_bytes!(concat!(
                    env!("OUT_DIR"),
                    "/compute/add.hlsl.comp.spirv"
                ),)),
            },
            materials: vec![(1, material_instance.0)],
            push_constants: vec![0..1],
        };

        struct Adder {
            mat: material::MaterialInstanceHandle,
        }

        impl ComputePassImpl for Adder {
            fn setup(&mut self, builder: &mut GraphBuilder) {
                builder.extern_move("Test", "TestFinal");

                builder.enable();
            }

            fn execute(&self, _: &Store, command_buffer: &mut ComputeCommandBuffer<'_>) {
                command_buffer.bind_material(1, self.mat);
                command_buffer.push_constant(0, 10.0_f32);

                command_buffer.dispatch([NUM_ELEMS as _, 1, 1]);
            }
        }

        let adder = Adder {
            mat: material_instance,
        };

        ctx.graph_add_compute_pass(graph, "AddSecond", info, adder);
    }

    ctx.graph_add_output(graph, "TestFinal");

    graph
}