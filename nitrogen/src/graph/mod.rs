use smallvec::SmallVec;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::borrow::Cow;

use gfx;

use storage::{Handle, Storage};

use device::DeviceContext;

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
impl_id!(VertexDescId);

pub type GraphHandle = Handle<Graph>;
pub type ConstructedGraphHandle = Handle<ConstructedGraph>;

pub struct GraphStorge {
    graphs: Storage<Graph>,
    compiled_graphs: HashMap<GraphHandle, CompiledGraph>,
}

impl GraphStorge {
    pub fn new() -> Self {
        GraphStorge {
            graphs: Storage::new(),
            compiled_graphs: HashMap::new(),
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

    pub fn add_output_image(&mut self, graph: GraphHandle, image_name: CowString) {
        self.graphs[graph].output_images.push(image_name);
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
        &self,
        device: &DeviceContext,
        render_pass_storage: &mut RenderPassStorage,
        pipeline_storage: &mut PipelineStorage,
        image_storage: &mut ImageStorage,
        sampler_storage: &mut SamplerStorage,
        graph_handle: GraphHandle,
        context: &ExecutionContext,
        resources: Option<GraphResources>,
    ) -> GraphResources {
        let graph = &self.graphs[graph_handle];
        let compiled_graph = &self.compiled_graphs[&graph_handle];

        let mut resources = resources.unwrap_or_else(|| {
            let images_len = compiled_graph.images.len();
            let passes_len = graph.passes.len();

            GraphResources {
                backbuffer_images: HashMap::with_capacity(compiled_graph.image_backbuffers.len()),
                pipelines: vec![None; passes_len],
                passes: vec![None; passes_len],
                images_transient: vec![None; images_len],
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
                            if resources.backbuffer_images.contains_key(img) {
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
                        }

                    }
                }
            }
        }

        resources
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
    pipeline_storage: &mut PipelineStorage,
    graph: &CompiledGraph,
    pass_id: PassId,
    pass_info: &PassInfo,
) -> Option<PipelineHandle> {

    let (primitive, shaders,) = match pass_info {
        PassInfo::Graphics { primitive, shaders, .. } => {
            (*primitive, shaders,)
        },
        _ => {
            return None;
        }
    };

    let create_info = GraphicsPipelineCreateInfo {
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
    output_images: Vec<CowString>,
}

pub struct GraphResources {
    pub(crate) backbuffer_images: HashMap<ImageId, image::ImageHandle>,
    pub(crate) pipelines: Vec<Option<PipelineHandle>>,
    pub(crate) passes: Vec<Option<RenderPassHandle>>,
    pub(crate) images_transient: Vec<Option<image::ImageHandle>>,
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
        vertex_desc: Option<VertexDescId>,
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
