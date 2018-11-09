use util::storage::{Handle, Storage};
use util::CowString;

pub mod pass;
pub use self::pass::*;

pub mod builder;
pub use self::builder::*;

pub mod command;
pub use self::command::*;

mod setup;
use self::setup::*;

mod baked;
use self::baked::*;

pub type GraphHandle = Handle<Graph>;

pub type PassName = CowString;
pub type ResourceName = CowString;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub struct ResourceId(pub(crate) usize);

pub struct Graph {
    passes: Vec<(PassName, PassInfo)>,
    passes_impl: Vec<Box<dyn PassImpl>>,
    pub(crate) output_images: Vec<ResourceName>,
    pub(crate) output_buffers: Vec<ResourceName>,
}

pub struct GraphStorage {
    pub(crate) storage: Storage<Graph>,
}

impl GraphStorage {
    pub fn new() -> Self {
        GraphStorage {
            storage: Storage::new(),
        }
    }

    pub fn create(&mut self) -> GraphHandle {
        self.storage
            .insert(Graph {
                passes: vec![],
                passes_impl: vec![],
                output_images: vec![],
                output_buffers: vec![],
            }).0
    }

    pub fn destroy(&mut self, handle: GraphHandle) {
        self.storage.remove(handle);
    }

    pub fn add_pass<T: Into<PassName>>(
        &mut self,
        handle: GraphHandle,
        name: T,
        pass_info: PassInfo,
        pass_impl: Box<dyn PassImpl>,
    ) {
        self.storage.get_mut(handle).map(|graph| {
            graph.passes.push((name.into(), pass_info));
            graph.passes_impl.push(pass_impl);
        });
    }

    pub fn compile(&mut self, handle: GraphHandle) -> Option<()> {
        let graph = self.storage.get_mut(handle)?;

        let set_up_graph = SetUpGraph::new(graph);

        let can_use_cached_baked_graph = false;

        let baked_graph = if can_use_cached_baked_graph {
            unimplemented!()
        } else {
            BakedGraph::new()
        };



        Some(())
    }

    pub fn add_output_image<T: Into<ResourceName>>(&mut self, handle: GraphHandle, image: T) {
        self.storage.get_mut(handle).map(|graph| {
            graph.output_images.push(image.into());
        });
    }

    pub fn add_output_buffer<T: Into<ResourceName>>(&mut self, handle: GraphHandle, buffer: T) {
        self.storage.get_mut(handle).map(|graph| {
            graph.output_buffers.push(buffer.into());
        });
    }
}

pub struct ExecutionContext {
    pub reference_size: (u32, u32),
}
