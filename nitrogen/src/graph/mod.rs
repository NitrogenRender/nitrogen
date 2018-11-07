use smallvec::SmallVec;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::borrow::Cow;

use gfx;

use storage::{Handle, Storage};

use device::DeviceContext;

use vertex_attrib::{VertexAttribStorage, VertexAttribHandle};

use pipeline::{GraphicsPipelineCreateInfo, PipelineHandle, PipelineStorage};
use render_pass::{RenderPassCreateInfo, RenderPassHandle, RenderPassStorage};
use image::{ImageStorage, ImageHandle}; // can't import ImageCreateInfo because it's shadowed here.
use sampler::{SamplerStorage, SamplerCreateInfo, SamplerHandle};

pub mod builder;
pub use self::builder::*;

pub mod command;
pub use self::command::*;

pub mod compiled;
pub use self::compiled::CompiledGraph;

pub mod constructed;
pub use self::constructed::ConstructedGraph;

use util::CowString;

use types::Framebuffer;

use image;
use sampler;
use pipeline;
use render_pass;

macro_rules! impl_id {
    ($name:ident) => {
        #[derive(Ord, PartialOrd, Eq, PartialEq, Copy, Clone, Hash, Default, Debug)]
        pub struct $name(pub usize);
    };
}

impl_id!(ImageId);
impl_id!(BufferId);
impl_id!(PassId);

pub type GraphHandle = Handle<Graph>;
pub type ConstructedGraphHandle = Handle<ConstructedGraph>;

pub struct GraphStorge {
    pub(crate) graphs: Storage<Graph>,
    pub(crate) compiled_graphs: HashMap<GraphHandle, CompiledGraph>,
    pub(crate) resources: HashMap<GraphHandle, GraphResources>,
}

impl GraphStorge {
    pub fn new() -> Self {
        GraphStorge {
            graphs: Storage::new(),
            compiled_graphs: HashMap::new(),
            resources: HashMap::new(),
        }
    }

    pub fn create(&mut self) -> GraphHandle {
        let graph = Graph {
            ..Default::default()
        };

        let (handle, _) = self.graphs.insert(graph);

        handle
    }

    pub fn destroy(&mut self, graph: GraphHandle) {
        self.graphs.remove(graph);
        self.compiled_graphs.remove(&graph);
    }

    pub fn add_pass(
        &mut self,
        graph: GraphHandle,
        name: CowString,
        info: PassInfo,
        pass_impl: Box<dyn PassImpl>,
    ) -> PassId {
        let graph = &mut self.graphs[graph];

        let id = PassId(graph.passes.len());

        graph.passes.push(info);
        graph.pass_impls.push(pass_impl);

        graph.pass_names.insert(name, id);

        id
    }

    pub fn set_output_image(&mut self, graph: GraphHandle, image_name: CowString) {
        self.graphs[graph].output_image = Some(image_name);
    }

    // TODO This needs to be revisited. The profiler says this function takes quite some resources..
    fn construct(&mut self, graph_handle: GraphHandle) -> ConstructedGraph {
        let graph = &mut self.graphs[graph_handle];

        let mut constructed_graph = ConstructedGraph::new();

        for i in 0..graph.passes.len() {
            let mut builder = builder::GraphBuilder::new();
            let pass_id = PassId(i);

            graph.pass_impls[i].setup(&mut builder);

            if builder.enabled {
                constructed_graph.add_pass(pass_id, builder);
            }
        }

        constructed_graph
    }

    pub fn compile(&mut self, graph_handle: GraphHandle) {
        let constructed_graph = self.construct(graph_handle);

        let mut compiled_graph = CompiledGraph::new(&self.graphs[graph_handle], constructed_graph);

        self.compiled_graphs.insert(graph_handle, compiled_graph);
    }

    pub fn execute(
        &mut self,
        device: &DeviceContext,
        render_pass_storage: &mut RenderPassStorage,
        pipeline_storage: &mut PipelineStorage,
        vertex_attrib_storage: &VertexAttribStorage,
        image_storage: &mut ImageStorage,
        sampler_storage: &mut SamplerStorage,
        graph_handle: GraphHandle,
        context: &ExecutionContext,
    ) {

        use gfx::Device;



        let graph = &self.graphs[graph_handle];
        let compiled_graph = &self.compiled_graphs[&graph_handle];

        let mut command_pool = {
            let queue_group = device.queue_group();

            device.device.create_command_pool_typed(
                &queue_group,
                gfx::pool::CommandPoolCreateFlags::empty(),
                graph.passes.len(),
            ).unwrap()
        };

        let resources = self.resources
            .entry(graph_handle)
            .or_insert_with(|| {
                let images_len = compiled_graph.images.len();
                let passes_len = graph.passes.len();

                let mut framebuffers = Vec::with_capacity(passes_len);
                for i in 0..passes_len {
                    framebuffers.push(None);
                }

                GraphResources {
                    backbuffer_images: HashSet::with_capacity(compiled_graph.image_backbuffers.len()),
                    pipelines: vec![None; passes_len],
                    passes: vec![None; passes_len],
                    images: vec![None; images_len],
                    framebuffers,
                }
            });

        for group in &compiled_graph.execution_list {
            for pass in group {
                // TODO fearless concurrency!! :ferrisBongo:
                // processing things one by one for now.

                let pass_info = &graph.passes[pass.0];

                // check if pipeline and stuff is set up properly
                {
                    match graph.passes[pass.0] {
                        PassInfo::Graphics { .. } => {
                            if resources.passes[pass.0].is_none() {
                                let render_pass = create_render_pass_graphics(
                                    device,
                                    render_pass_storage,
                                    compiled_graph,
                                    *pass,
                                    pass_info,
                                );

                                resources.passes[pass.0] = render_pass;
                            }

                            if resources.pipelines[pass.0].is_none() && resources.passes[pass.0].is_some() {
                                let graphics_pipeline = create_graphics_pipeline(
                                    device,
                                    render_pass_storage,
                                    resources.passes[pass.0].unwrap(),
                                    vertex_attrib_storage,
                                    pipeline_storage,
                                    compiled_graph,
                                    *pass,
                                    pass_info,
                                );

                                resources.pipelines[pass.0] = graphics_pipeline;
                            }
                        }
                        PassInfo::Compute => {}
                    }
                }

                // create image resources
                {
                    for img in &compiled_graph.image_creates[pass] {

                        if compiled_graph.image_backbuffers.contains(img) {
                            // this will be a backbuffer image, so not a transient resource.
                            if resources.backbuffer_images.contains(img) {
                                continue;
                            }

                            // create the new image here.
                            let image = create_image(
                                device,
                                image_storage,
                                sampler_storage,
                                context,
                                compiled_graph.image_info(*img).unwrap(),
                                false,
                            );

                            if image.is_some() {
                                resources.backbuffer_images.insert(*img);
                            }

                            resources.images[img.0] = image;

                        } else {
                            // transient image.
                            let image = create_image(
                                device,
                                image_storage,
                                sampler_storage,
                                context,
                                compiled_graph.image_info(*img).unwrap(),
                                true,
                            );

                            resources.images[img.0] = image;
                        }

                    }
                }

                // create framebuffers
                {
                    if resources.framebuffers[pass.0].is_none() {

                        use gfx::Device;

                        // to create a framebuffer we need a render pass which the framebuffer
                        // will be compatible with.
                        // Also we need a list of image views for the attachments.
                        // The framebuffer itself has an extent. Here we just take the extent from
                        // the first attachment we find.

                        let render_pass = render_pass_storage.raw(resources.passes[pass.0].unwrap()).unwrap();

                        let (views, dimensions) = {
                            compiled_graph.image_writes[pass]
                                .iter()
                                .map(|x| {
                                    let handle = resources.images[x.0].unwrap();
                                    let image = image_storage.raw(handle.0).unwrap();
                                    (&image.view, image.dimension)
                                })
                                .unzip::<_, _, SmallVec<[_; 16]>, SmallVec<[_; 16]>>()
                        };

                        let extent = {
                            dimensions.as_slice()
                                .iter()
                                .map(|img_dim| {
                                    match img_dim {
                                        image::ImageDimension::D1 { x, } => {
                                            (*x, 1, 1)
                                        }
                                        image::ImageDimension::D2 { x, y, } => {
                                            (*x, *y, 1)
                                        },
                                        image::ImageDimension::D3 { x, y, z, } => {
                                            (*x, *y, *z)
                                        }
                                    }
                                })
                                .map(|(x, y, z)| {
                                    gfx::image::Extent {
                                        width: x,
                                        height: y,
                                        depth: z,
                                    }
                                }).next().unwrap()
                        };

                        let framebuffer = device.device.create_framebuffer(
                            render_pass,
                            views,
                            extent,
                        );

                        resources.framebuffers[pass.0] = framebuffer.ok();
                    }
                }

                // Run!!!
                {
                    let pass_impl = &graph.pass_impls[pass.0];

                    let mut raw_cmd_buf = command_pool.acquire_command_buffer(
                        false,
                    );

                    let pipeline = {
                        let handle = resources.pipelines[pass.0].unwrap();

                        &pipeline_storage.raw_graphics(handle).unwrap().pipeline
                    };
                    let render_pass = {
                        let handle = resources.passes[pass.0].unwrap();

                        render_pass_storage.raw(handle).unwrap()
                    };
                    let framebuffer = {
                        resources.framebuffers[pass.0].as_ref().unwrap()
                    };

                    let viewport = gfx::pso::Viewport {
                        depth: 0.0..1.0,
                        rect: gfx::pso::Rect {
                            x: 0,
                            y: 0,
                            w: context.reference_size.0 as i16,
                            h: context.reference_size.1 as i16,
                        },
                    };

                    raw_cmd_buf.bind_graphics_pipeline(pipeline);

                    raw_cmd_buf.set_viewports(0, &[viewport.clone()]);
                    raw_cmd_buf.set_scissors(0, &[viewport.rect]);

                    {
                        let mut encoder = raw_cmd_buf.begin_render_pass_inline(
                            render_pass,
                            framebuffer,
                            viewport.rect,
                            &[
                                gfx::command::ClearValue::Color(
                                    gfx::command::ClearColor::Float(
                                        [0.0, 0.0, 0.0, 0.0]
                                    )
                                ),
                            ]
                        );

                        let mut command_buffer = command::CommandBuffer::new(encoder);
                        pass_impl.execute(&mut command_buffer);
                    }

                    let submit_buffer = raw_cmd_buf.finish();

                    let mut submit_fence = device.device.create_fence(false).unwrap();

                    {
                        let submission = gfx::Submission::new()
                            .submit(Some(submit_buffer));
                        device.queue_group().queues[0].submit(submission, Some(&mut submit_fence));
                    }

                    device.device.wait_for_fence(&submit_fence, !0);
                    device.device.destroy_fence(submit_fence);
                }

            }

        }

        command_pool.reset();

        device.device.destroy_command_pool(command_pool.into_raw());
    }
}

fn create_render_pass_graphics(
    device: &DeviceContext,
    render_pass_storage: &mut RenderPassStorage,
    graph: &CompiledGraph,
    pass_id: PassId,
    pass_info: &PassInfo,
) -> Option<RenderPassHandle> {
    let attachments = {

        let empty_hash_set = HashSet::new();
        let writes = {
            if let Some(set) = graph.image_writes.get(&pass_id) {
                set
            } else {
                &empty_hash_set
            }
        };

        let write_images = writes.iter().map(|image| {
            let info = graph.image_info(*image).unwrap();

            let is_creating_image = match &graph.images[image.0].1 {
                compiled::ImageCreateType::Copy(..) => false,
                _ => true,
            };

            (info, is_creating_image)
        });

        write_images
            .clone()
            .map(|(info, is_creating_image)| {
                let load_op = if is_creating_image {
                    gfx::pass::AttachmentLoadOp::Clear
                } else {
                    gfx::pass::AttachmentLoadOp::Load
                };

                gfx::pass::Attachment {
                    format: Some(info.format.into()),
                    samples: 0,
                    ops: gfx::pass::AttachmentOps {
                        load: load_op,
                        store: gfx::pass::AttachmentStoreOp::Store,
                    },
                    // TODO stencil stuff
                    stencil_ops: gfx::pass::AttachmentOps {
                        load: gfx::pass::AttachmentLoadOp::DontCare,
                        store: gfx::pass::AttachmentStoreOp::DontCare,
                    },
                    layouts: gfx::image::Layout::Undefined..gfx::image::Layout::General,
                }
            }).collect::<SmallVec<[_; 16]>>()
    };

    let color_attachments = attachments
        .as_slice()
        .iter()
        .enumerate()
        .map(|(i, attachment)| {
            (i, gfx::image::Layout::General)
        })
        .collect::<SmallVec<[_; 16]>>();

    let subpass = {
        gfx::pass::SubpassDesc {
            colors: color_attachments.as_slice(),
            depth_stencil: None,
            inputs: &[],
            resolves: &[],
            preserves: &[]
        }
    };

    let dependencies = {
        gfx::pass::SubpassDependency {
            passes: gfx::pass::SubpassRef::External .. gfx::pass::SubpassRef::Pass(0),
            stages: gfx::pso::PipelineStage::COLOR_ATTACHMENT_OUTPUT
                ..gfx::pso::PipelineStage::COLOR_ATTACHMENT_OUTPUT,
            accesses: gfx::image::Access::empty()
                ..(gfx::image::Access::COLOR_ATTACHMENT_READ
                | gfx::image::Access::COLOR_ATTACHMENT_WRITE),
        }
    };

    let create_info = RenderPassCreateInfo {
        attachments: attachments.as_slice(),
        subpasses: &[subpass],
        dependencies: &[dependencies],
    };

    render_pass_storage
        .create(device, &[create_info])
        .remove(0)
        .ok()
}

fn create_graphics_pipeline(
    device: &DeviceContext,
    render_pass_storage: &RenderPassStorage,
    render_pass: RenderPassHandle,
    vertex_attrib_storage: &VertexAttribStorage,
    pipeline_storage: &mut PipelineStorage,
    graph: &CompiledGraph,
    pass_id: PassId,
    pass_info: &PassInfo,
) -> Option<PipelineHandle> {

    let (primitive, shaders, vertex_attribs,) = match pass_info {
        PassInfo::Graphics { primitive, shaders, vertex_attrib, .. } => {
            (*primitive, shaders, vertex_attrib,)
        },
        _ => {
            return None;
        }
    };

    let create_info = GraphicsPipelineCreateInfo {
        vertex_attribs: vertex_attribs.clone(),
        primitive,
        shader_vertex: pipeline::ShaderInfo {
            content: &shaders.vertex.content,
            entry: &shaders.vertex.entry,
        },
        shader_fragment: if shaders.fragment.is_some() {
            Some(pipeline::ShaderInfo {
                content: &shaders.fragment.as_ref().unwrap().content,
                entry: &shaders.fragment.as_ref().unwrap().entry,
            })
        } else {
            None
        },
        // TODO also add support for geometry shaders in graph
        shader_geometry: None
    };

    let handle = pipeline_storage.create_graphics_pipelines(
        device,
        render_pass_storage,
        vertex_attrib_storage,
        render_pass,
        &[create_info]
    ).remove(0).ok();

    handle
}

fn create_image(
    device: &DeviceContext,
    image_storage: &mut ImageStorage,
    sampler_storage: &mut SamplerStorage,
    exec_context: &ExecutionContext,
    create_info: &ImageCreateInfo,
    is_transient: bool,
) -> Option<(ImageHandle, SamplerHandle)> {

    let dims = {

        let (width, height) = match create_info.size_mode {
            image::ImageSizeMode::Absolute { width, height } => {
                (width, height)
            },
            image::ImageSizeMode::ContextRelative { width, height } => {
                (
                    (width * exec_context.reference_size.0 as f32) as u32,
                    (height * exec_context.reference_size.1 as f32) as u32,
                )
            }
        };

        image::ImageDimension::D2 { x: width, y: height }
    };

    let internal_create_info = image::ImageCreateInfo {
        dimension: dims,
        num_layers: 1,
        num_samples: 1,
        num_mipmaps: 1,
        format: create_info.format,
        kind: image::ViewKind::D2,
        used_as_transfer_src: true,
        used_as_transfer_dst: true,
        used_for_sampling: true,
        used_as_color_attachment: true,
        used_as_depth_stencil_attachment: false,
        used_as_storage_image: false,
        used_as_input_attachment: false,

        is_transient,
    };

    let img = image_storage.create(
        device,
        &[internal_create_info]
    ).remove(0).ok()?;

    let sampler_create_info = SamplerCreateInfo {
        wrap_mode: (
            sampler::WrapMode::Clamp,
            sampler::WrapMode::Clamp,
            sampler::WrapMode::Clamp,
            ),
        mag_filter: sampler::Filter::Linear,
        min_filter: sampler::Filter::Linear,
        mip_filter: sampler::Filter::Linear,
    };

    let sampler = sampler_storage.create(
        device,
        &[sampler_create_info]
    ).remove(0);

    Some((img, sampler))
}

#[derive(Default)]
pub struct Graph {
    pass_names: BTreeMap<CowString, PassId>,
    pass_impls: Vec<Box<dyn PassImpl>>,
    passes: Vec<PassInfo>,
    pub(crate) output_image: Option<CowString>,
}

pub struct GraphResources {
    pub(crate) backbuffer_images: HashSet<ImageId>,
    pub(crate) pipelines: Vec<Option<PipelineHandle>>,
    pub(crate) passes: Vec<Option<RenderPassHandle>>,
    pub(crate) images: Vec<Option<(image::ImageHandle, sampler::SamplerHandle)>>,
    pub(crate) framebuffers: Vec<Option<Framebuffer>>,
}

pub struct ExecutionContext {
    pub reference_size: (u32, u32),
}

pub trait PassImpl {
    fn setup(&mut self, builder: &mut GraphBuilder);
    fn execute(&self, command_buffer: &mut CommandBuffer);
}

struct InstanceData {}

pub enum PassInfo {
    Compute,
    Graphics {
        vertex_attrib: Option<VertexAttribHandle>,
        shaders: Shaders,
        primitive: pipeline::Primitive,
        blend_mode: render_pass::BlendMode,
    },
}

pub struct Shaders {
    pub vertex: ShaderInfo,
    pub fragment: Option<ShaderInfo>,
}

pub struct ShaderInfo {
    pub content: Cow<'static, [u8]>,
    pub entry: CowString,
}

pub struct Kernel {
    pub kernel: (),
}

#[derive(Debug, Clone)]
pub struct ImageCreateInfo {
    pub format: image::ImageFormat,
    pub size_mode: image::ImageSizeMode,
}

#[derive(Default)]
struct BufferDesc {
    name: CowString,
}

#[derive(Default)]
struct VertexDesc {}

#[derive(Default)]
struct Backbuffers {
    pub images: BTreeSet<ImageId>,
    pub buffers: BTreeSet<BufferId>,
}
