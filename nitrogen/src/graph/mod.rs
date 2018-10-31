use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use storage::{Handle, Storage};

pub mod builder;
pub use self::builder::*;

pub mod command;
pub use self::command::*;

use util::CowString;

use image;
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
    constructed_graphs: HashMap<GraphHandle, ConstructedGraph>,
}

impl GraphStorge {
    pub fn new() -> Self {
        GraphStorge {
            graphs: Storage::new(),
            constructed_graphs: HashMap::new(),
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
        self.constructed_graphs.remove(&graph);
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

    pub fn add_output_image(&mut self, graph: GraphHandle, image_name: CowString) -> bool {
        false
    }

    pub fn construct(&mut self, graph_handle: GraphHandle) {
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

        println!("{:?}", constructed_graph.execution_list());

        self.constructed_graphs
            .insert(graph_handle, constructed_graph);
    }
}

use std::collections::BinaryHeap;

#[derive(Debug)]
pub struct ConstructedGraph {
    num_nodes: usize,

    root_nodes: HashSet<PassId>,
    nodes_image_creates: HashMap<PassId, HashSet<CowString>>,
    nodes_image_copies: HashMap<PassId, HashSet<CowString>>,

    image_creates: HashMap<CowString, (PassId, ImageCreateInfo)>,
    image_copies: HashMap<CowString, (PassId, CowString)>,

    image_depend_reads: HashMap<CowString, HashSet<PassId>>,
    image_depend_writes: HashMap<CowString, HashSet<PassId>>,
}

impl ConstructedGraph {
    fn new() -> Self {
        ConstructedGraph {
            num_nodes: 0,

            root_nodes: HashSet::new(),
            nodes_image_creates: HashMap::new(),
            nodes_image_copies: HashMap::new(),

            image_creates: HashMap::new(),
            image_copies: HashMap::new(),
            image_depend_reads: HashMap::new(),
            image_depend_writes: HashMap::new(),
        }
    }

    fn add_pass(&mut self, pass_id: PassId, builder: GraphBuilder) {
        self.num_nodes += 1;

        if builder.images_read.len() == 0 && builder.images_write.len() == 0 {
            self.root_nodes.insert(pass_id);
        }

        let image_creates = builder
            .images_create
            .iter()
            .map(|(name, _)| name.clone())
            .collect::<HashSet<_>>();

        self.nodes_image_creates.insert(pass_id, image_creates);

        let image_copies = builder
            .images_copy
            .iter()
            .map(|(new, src)| {
                new.clone()
            })
            .collect::<HashSet<_>>();

        self.nodes_image_copies.insert(pass_id, image_copies);

        for (name, info) in builder.images_create {
            self.image_creates.insert(name, (pass_id, info));
        }

        for (new, src) in builder.images_copy {
            self.image_copies.insert(new, (pass_id, src));
        }

        for name in builder.images_read {
            self.image_depend_reads
                .entry(name)
                .or_insert(HashSet::new())
                .insert(pass_id);
        }

        for name in builder.images_write {
            self.image_depend_writes
                .entry(name)
                .or_insert(HashSet::new())
                .insert(pass_id);
        }
    }

    // WARNING
    // THIS DOES NOT YET DO CYCLE DETECTION
    // YOU HAVE BEEN WARNED.
    fn execution_list(&self) -> Vec<Vec<PassId>> {
        let mut order = Vec::with_capacity(self.num_nodes);

        let mut nodes = HashSet::with_capacity(self.num_nodes);
        let mut nodes_tmp: HashSet<PassId> = HashSet::with_capacity(self.num_nodes);
        let mut images = HashSet::with_capacity(self.num_nodes);

        let mut node_last_position = HashMap::new();

        // start with root nodes
        for node in &self.root_nodes {
            nodes.insert(*node);
        }

        // flatten graph with duplicates
        while nodes.len() > 0 {
            for node in &nodes {
                node_last_position.insert(*node, order.len());
                order.push(*node);
            }

            for node in &nodes {
                let imgs = &self.nodes_image_creates[node];
                for img in imgs {
                    images.insert(img.clone());
                }

                let imgs = &self.nodes_image_copies[node];
                for img in imgs {
                    images.insert(img.clone());
                }
            }

            nodes.clear();

            for img in &images {
                if let Some(ns) = self.image_depend_reads.get(img) {

                    for n in ns {
                        nodes.insert(*n);
                    }
                }
                if let Some(ns) = self.image_depend_writes.get(img) {

                    for n in ns {
                        nodes.insert(*n);
                    }
                }
            }

            images.clear();
        }

        // deduplicate the list
        order
            .iter()
            .enumerate()
            .filter_map(|(idx, n)| {
                if node_last_position[n] > idx {
                    None
                } else {
                    Some(vec![*n])
                }
            })
            .collect()
    }
}

#[derive(Default)]
pub struct Graph {
    pass_names: BTreeMap<CowString, PassId>,
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
    pub vertex: CowString,
    pub fragment: Option<CowString>,
}

pub struct Kernel {
    pub kernel: (),
}

#[derive(Debug)]
pub(crate) struct ImageDesc {
    name: CowString,
    info: ImageCreateInfo,
}

#[derive(Debug)]
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
