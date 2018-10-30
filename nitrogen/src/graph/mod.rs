use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use storage::{Handle, Storage};

pub mod builder;
pub use self::builder::*;

pub mod command;
pub use self::command::*;

use image;
use pipeline;
use render_pass;

macro_rules! impl_id {
    ($name:ident) => {
        #[derive(Ord, PartialOrd, Eq, PartialEq, Copy, Clone, Default)]
        pub struct $name(pub usize);
    };
}

impl_id!(ImageId);
impl_id!(BufferId);
impl_id!(PassId);
impl_id!(VertexDescId);

pub type GraphHandle = Handle<Graph>;

pub struct GraphStorge {
    graphs: Storage<Graph>,
}

impl GraphStorge {
    pub fn new() -> Self {
        GraphStorge { graphs: Storage::new() }
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
    }

    pub fn add_pass(
        &mut self,
        graph: GraphHandle,
        name: &str,
        info: PassInfo,
        pass_impl: Box<dyn PassImpl>,
    ) -> PassId {
        let graph = &mut self.graphs[graph];

        let id = PassId(graph.passes.len());

        graph.passes.push(info);
        graph.pass_impls.push(pass_impl);

        graph.pass_names.insert(name.into(), id);

        id
    }

    pub fn add_output_image(
        &mut self,
        graph: GraphHandle,
        image_name: &str,
    ) -> bool {
        false
    }

    pub fn construct(&mut self, graph: GraphHandle) {
        let graph = &mut self.graphs[graph];

        for i in 0..graph.passes.len() {
            let mut builder = builder::GraphBuilder {};

            graph.pass_impls[i].setup(&mut builder);
        }
    }
}

#[derive(Default)]
pub struct Graph {
    pass_names: BTreeMap<String, PassId>,
    pass_impls: Vec<Box<dyn PassImpl>>,
    passes: Vec<PassInfo>,
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
    pub vertex: Cow<'static, str>,
    pub fragment: Option<Cow<'static, str>>,
}

pub struct Kernel {
    pub kernel: (),
}

struct ImageDesc {
    name: Cow<'static, str>,
    info: ImageCreateInfo,
}

pub struct ImageCreateInfo {
    pub format: image::ImageFormat,
    pub size_mode: image::ImageSizeMode,
}

#[derive(Default)]
struct BufferDesc {
    name: Cow<'static, str>,
}

#[derive(Default)]
struct VertexDesc {}

#[derive(Default)]
struct Backbuffers {
    pub images: BTreeSet<ImageId>,
    pub buffers: BTreeSet<BufferId>,
}
