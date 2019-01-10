/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

extern crate image as img;

use nitrogen_examples_common::{
    self as helper,
    main_loop::{MainLoop, UserData},
};

use nitrogen::{
    self as nit, buffer, graph, graph::GraphicsPassImpl, image, material, sampler, submit_group,
    vertex_attrib as vtx,
};

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

struct Data {
    graph: graph::GraphHandle,

    bufs: Vec<buffer::BufferHandle>,

    img: image::ImageHandle,
    sampler: sampler::SamplerHandle,

    mat: material::MaterialHandle,

    vtx_def: vtx::VertexAttribHandle,
}

impl UserData for Data {
    fn graph(&self) -> Option<graph::GraphHandle> {
        Some(self.graph)
    }

    fn output_image(&self) -> Option<graph::ResourceName> {
        Some("Output".into())
    }

    fn release(self, ctx: &mut nit::Context, submit: &mut submit_group::SubmitGroup) {
        submit.graph_destroy(ctx, &[self.graph]);

        submit.buffer_destroy(ctx, &self.bufs);
        submit.image_destroy(ctx, &[self.img]);
        submit.sampler_destroy(ctx, &[self.sampler]);
        submit.material_destroy(&[self.mat]);

        unsafe {
            submit.wait(ctx);
        }

        ctx.vertex_attribs_destroy(&[self.vtx_def]);
    }
}

fn init(
    _store: &mut graph::Store,
    ctx: &mut nit::Context,
    submit: &mut submit_group::SubmitGroup,
) -> Option<Data> {
    // create image

    let (img, sampler) = unsafe {
        let data = include_bytes!("assets/test.png");

        let image = img::load(std::io::Cursor::new(&data[..]), img::PNG)
            .ok()?
            .to_rgba();

        let dims = image.dimensions();

        helper::resource::image_color(ctx, submit, &*image, dims, image::ImageFormat::RgbaUnorm)?
    };

    // create quad buffers

    let buf_pos =
        unsafe { helper::resource::buffer_device_local_vertex(ctx, submit, &TRIANGLE_POS)? };

    let buf_uv =
        unsafe { helper::resource::buffer_device_local_vertex(ctx, submit, &TRIANGLE_UV)? };

    // vertex attribute description

    let vertex_attrib = {
        let info = vtx::VertexAttribInfo {
            buffer_infos: &[
                // pos
                vtx::VertexAttribBufferInfo {
                    stride: std::mem::size_of::<[f32; 2]>(),
                    index: 0,
                    elements: &[vtx::VertexAttribBufferElementInfo {
                        location: 0,
                        format: nit::gfx::format::Format::Rg32Float,
                        offset: 0,
                    }],
                },
                // uv
                vtx::VertexAttribBufferInfo {
                    stride: std::mem::size_of::<[f32; 2]>(),
                    index: 1,
                    elements: &[vtx::VertexAttribBufferElementInfo {
                        location: 1,
                        format: nitrogen::gfx::format::Format::Rg32Float,
                        offset: 0,
                    }],
                },
            ],
        };

        ctx.vertex_attribs_create(&[info]).remove(0)
    };

    // create material and material instance

    let material = unsafe {
        let create_info = material::MaterialCreateInfo {
            parameters: &[
                (0, material::MaterialParameterType::SampledImage),
                (1, material::MaterialParameterType::Sampler),
            ],
        };

        ctx.material_create(&[create_info]).remove(0)
    }
    .unwrap();

    let mat_instance = unsafe { ctx.material_create_instance(&[material]).remove(0) }.unwrap();

    unsafe {
        ctx.material_write_instance(
            mat_instance,
            &[
                nitrogen::material::InstanceWrite {
                    binding: 0,
                    data: nitrogen::material::InstanceWriteData::Image { image: img },
                },
                nitrogen::material::InstanceWrite {
                    binding: 1,
                    data: nitrogen::material::InstanceWriteData::Sampler { sampler },
                },
            ],
        );
    }

    let graph = setup_graphs(
        ctx,
        Some(vertex_attrib),
        buf_pos,
        buf_uv,
        material,
        mat_instance,
    );

    Some(Data {
        graph,

        bufs: vec![buf_pos, buf_uv],
        img,
        sampler,
        vtx_def: vertex_attrib,
        mat: material,
    })
}

fn main() {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();

    let mut ml = unsafe { MainLoop::new("Multi Target", init) }.unwrap();

    while ml.running() {
        unsafe {
            ml.iterate();
        }
    }

    unsafe {
        ml.release();
    }
}

fn setup_graphs(
    ctx: &mut nitrogen::Context,
    vertex_attrib: Option<nitrogen::vertex_attrib::VertexAttribHandle>,
    buffer_pos: nitrogen::buffer::BufferHandle,
    buffer_uv: nitrogen::buffer::BufferHandle,
    material: nitrogen::material::MaterialHandle,
    material_instance: nitrogen::material::MaterialInstanceHandle,
) -> graph::GraphHandle {
    let graph = ctx.graph_create();

    fn image_create_info() -> graph::ImageCreateInfo {
        graph::ImageCreateInfo {
            format: image::ImageFormat::RgbaUnorm,
            size_mode: image::ImageSizeMode::ContextRelative {
                width: 1.0,
                height: 1.0,
            },
        }
    }

    fn image_create_info_r() -> graph::ImageCreateInfo {
        graph::ImageCreateInfo {
            format: image::ImageFormat::RUnorm,
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
                    "/multi-target/split.hlsl.vert.spirv"
                ))),
                entry: "VertexMain".into(),
            },
            fragment: Some(nitrogen::graph::ShaderInfo {
                content: Cow::Borrowed(include_bytes!(concat!(
                    env!("OUT_DIR"),
                    "/multi-target/split.hlsl.frag.spirv"
                ))),
                entry: "FragmentMain".into(),
            }),
            geometry: None,
        };

        let (pass_impl, info) = create_test_pass(
            shaders,
            |builder| {
                builder.image_create("Red", image_create_info_r());
                builder.image_create("Green", image_create_info_r());
                builder.image_create("Blue", image_create_info_r());

                builder.image_write_color("Red", 0);
                builder.image_write_color("Green", 1);
                builder.image_write_color("Blue", 2);

                builder.enable();
            },
            move |cmd| unsafe {
                cmd.bind_vertex_buffers(&[(buffer_pos, 0), (buffer_uv, 0)]);

                cmd.bind_material(0, material_instance);

                cmd.draw(0..4, 0..1);
            },
            vertex_attrib.clone(),
            vec![(0, material)],
            3,
        );

        ctx.graph_add_graphics_pass(graph, "Split", info, pass_impl);
    }

    {
        let shaders = nitrogen::graph::Shaders {
            vertex: nitrogen::graph::ShaderInfo {
                content: Cow::Borrowed(include_bytes!(concat!(
                    env!("OUT_DIR"),
                    "/multi-target/read.hlsl.vert.spirv"
                ))),
                entry: "VertexMain".into(),
            },
            fragment: Some(nitrogen::graph::ShaderInfo {
                content: Cow::Borrowed(include_bytes!(concat!(
                    env!("OUT_DIR"),
                    "/multi-target/read.hlsl.frag.spirv"
                ))),
                entry: "FragmentMain".into(),
            }),
            geometry: None,
        };

        let (pass_impl, info) = create_test_pass(
            shaders,
            |builder| {
                builder.image_create("Output", image_create_info());

                builder.image_write_color("Output", 0);

                builder.image_read_color("Red", 0, 1);
                builder.image_read_color("Green", 2, 3);
                builder.image_read_color("Blue", 4, 5);

                builder.enable();
            },
            move |cmd| unsafe {
                cmd.bind_vertex_buffers(&[(buffer_pos, 0), (buffer_uv, 0)]);

                cmd.draw(0..4, 0..4);
            },
            vertex_attrib,
            vec![],
            1,
        );

        ctx.graph_add_graphics_pass(graph, "Read", info, pass_impl);
    }

    ctx.graph_add_output(graph, "Output");

    graph
}

fn create_test_pass<FSetUp, FExec>(
    shaders: nitrogen::graph::Shaders,
    setup: FSetUp,
    execute: FExec,
    vert: Option<nitrogen::vertex_attrib::VertexAttribHandle>,
    materials: Vec<(usize, nitrogen::material::MaterialHandle)>,
    num_attachments: usize,
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
        blend_modes: vec![nitrogen::graph::BlendMode::Alpha; num_attachments],
        materials,
        push_constants: vec![],
    };

    struct TestPass<FSetUp, FExec> {
        setup: FSetUp,
        exec: FExec,

        num_attachments: usize,
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
                    .begin_render_pass(
                        std::iter::repeat(graph::ImageClearValue::Color([0.0, 0.0, 0.0, 1.0]))
                            .take(self.num_attachments),
                    )
                    .unwrap()
            };
            (self.exec)(&mut cmd);
        }
    }

    let pass = TestPass {
        setup,
        exec: execute,
        num_attachments,
    };

    (pass, pass_info)
}
