use util::storage::{Handle, Storage};
use util::CowString;

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;

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

mod execution_path;
use self::execution_path::*;

pub type GraphHandle = Handle<Graph>;

pub type PassName = CowString;
pub type ResourceName = CowString;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub struct ResourceId(pub(crate) usize);

#[derive(Debug)]
struct GraphResourcesResolved {
    name_lookup: BTreeMap<ResourceName, ResourceId>,
    defines: BTreeMap<ResourceId, PassId>,
    infos: BTreeMap<ResourceId, ResourceCreateInfo>,
    copies_from: BTreeMap<ResourceId, ResourceId>,
    moves_from: BTreeMap<ResourceId, ResourceId>,
    moves_to: BTreeMap<ResourceId, ResourceId>,

    reads: BTreeMap<ResourceId, BTreeSet<(PassId, ResourceReadType, u8)>>,
    writes: BTreeMap<ResourceId, BTreeSet<(PassId, ResourceWriteType, u8)>>,
}

#[derive(Debug)]
pub enum GraphCompileError {
    InvalidGraph,
    ResourceRedefined {
        res: ResourceName,
        prev: PassId,
        pass: PassId,
    },
    ReferencedInvalidResource {
        res: ResourceName,
        pass: PassId,
    },
    ResourceTypeMismatch {
        res: ResourceName,
        pass: PassId,
        used_as: ResourceType,
        expected: ResourceType,
    },
}

pub struct Graph {
    passes: Vec<(PassName, PassInfo)>,
    passes_impl: Vec<Box<dyn PassImpl>>,
    pub(crate) output_resources: Vec<ResourceName>,

    resolve_cache: HashMap<u64, (GraphResourcesResolved, usize)>,
    exec_graph_cache: HashMap<(usize, Vec<ResourceName>), ()>,
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
                output_resources: vec![],
                resolve_cache: HashMap::new(),
                exec_graph_cache: HashMap::new(),
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

    /// Compile the graph so it is optimized for execution.
    ///
    /// Compiling the graph is potentially a rather expensive operation.
    /// The "user facing" graph operates with resource *names* and any dependencies are only
    /// implied, not manifested in a datastructure somewhere, so the first thing to do is to
    /// Get all the "unrelated" nodes into a graph structure that has direct or indirect links to
    /// all dependent nodes.
    ///
    /// This representation is then hashed to see if we already did any further work in the past
    /// already and can use a cached graph.
    ///
    ///
    pub fn compile(&mut self, handle: GraphHandle) -> Result<(), Vec<GraphCompileError>> {
        let graph = self
            .storage
            .get_mut(handle)
            .ok_or(vec![GraphCompileError::InvalidGraph])?;

        // TODO can I get rid of the SetUpGraph entirely? :thinking:

        let input = {
            let mut pass_resource_creates = BTreeMap::new();
            let mut pass_resource_copies = BTreeMap::new();
            let mut pass_resource_moves = BTreeMap::new();

            let mut pass_resource_reads = BTreeMap::new();
            let mut pass_resource_writes = BTreeMap::new();

            for (i, pass) in graph.passes_impl.iter_mut().enumerate() {
                let mut builder = GraphBuilder::new();
                pass.setup(&mut builder);

                let id = PassId(i);

                if builder.enabled {
                    pass_resource_creates.insert(id, builder.resource_creates);
                    pass_resource_copies.insert(id, builder.resource_copies);
                    pass_resource_moves.insert(id, builder.resource_moves);

                    pass_resource_reads.insert(id, builder.resource_reads);
                    pass_resource_writes.insert(id, builder.resource_writes);
                }
            }

            GraphInput {
                resource_creates: pass_resource_creates,
                resource_copies: pass_resource_copies,
                resource_moves: pass_resource_moves,

                resource_writes: pass_resource_writes,
                resource_reads: pass_resource_reads,
            }
        };

        // TODO hash the above things and make a lookup table to spare doing the work below.

        let mut errors = vec![];
        let mut read_types = vec![];
        let mut write_types = vec![];

        // "reverse the arrows"

        let input_hash = {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};

            let mut hasher = DefaultHasher::new();

            input.hash(&mut hasher);

            hasher.finish()
        };

        let resolve_cache = &mut graph.resolve_cache;

        let resolved_id = resolve_cache.len();

        let (resolved, resolved_id) = resolve_cache.entry(input_hash).or_insert_with(|| {
            (
                resolve_input_graph(input, &mut read_types, &mut write_types, &mut errors),
                resolved_id,
            )
        });

        // TODO check write and read types match with creation types.

        graph.exec_graph_cache
            .entry((*resolved_id, graph.output_resources.clone()))
            .or_insert_with(|| ());

        if !errors.is_empty() {

            Err(errors)
        } else {
            Ok(())
        }
    }

    pub fn add_output<T: Into<ResourceName>>(&mut self, handle: GraphHandle, image: T) {
        self.storage.get_mut(handle).map(|graph| {
            graph.output_resources.push(image.into());
        });
    }
}

#[derive(Debug, Hash)]
struct GraphInput {
    resource_creates: BTreeMap<PassId, Vec<(ResourceName, ResourceCreateInfo)>>,
    resource_copies: BTreeMap<PassId, Vec<(ResourceName, ResourceName)>>,
    resource_moves: BTreeMap<PassId, Vec<(ResourceName, ResourceName)>>,

    resource_reads: BTreeMap<PassId, Vec<(ResourceName, ResourceReadType, u8)>>,
    resource_writes: BTreeMap<PassId, Vec<(ResourceName, ResourceWriteType, u8)>>,
}

fn resolve_input_graph(
    input: GraphInput,
    reads: &mut Vec<(ResourceId, ResourceReadType, PassId)>,
    writes: &mut Vec<(ResourceId, ResourceWriteType, PassId)>,
    errors: &mut Vec<GraphCompileError>,
) -> GraphResourcesResolved {
    println!("Create resolved graph");

    let mut resource_name_lookup = BTreeMap::new();

    let mut resource_defines = BTreeMap::new();
    let mut resource_infos = BTreeMap::new();
    let mut resource_copies_from = BTreeMap::new();
    let mut resource_moves_from = BTreeMap::new();
    let mut resource_moves_to = BTreeMap::new();

    let mut resource_reads = BTreeMap::new();
    let mut resource_writes = BTreeMap::new();

    // generate IDs for all "new" resources.

    for (pass, ress) in input.resource_creates {
        'res: for (name, info) in ress {
            if let Some(id) = resource_name_lookup.get(&name) {
                errors.push(GraphCompileError::ResourceRedefined {
                    pass,
                    res: name.clone(),
                    prev: resource_defines[id],
                });
                continue 'res;
            }

            let id = ResourceId(resource_defines.len());
            resource_defines.insert(id, pass);
            resource_infos.insert(id, info);
            resource_name_lookup.insert(name, id);
        }
    }

    for (pass, ress) in &input.resource_copies {
        'res: for (new_name, old_name) in ress {
            if let Some(id) = resource_name_lookup.get(new_name) {
                errors.push(GraphCompileError::ResourceRedefined {
                    pass: *pass,
                    res: new_name.clone(),
                    prev: resource_defines[id],
                });
                continue 'res;
            }

            let id = ResourceId(resource_defines.len());

            resource_defines.insert(id, *pass);
            resource_name_lookup.insert(new_name.clone(), id);
        }
    }

    for (pass, ress) in &input.resource_moves {
        'res: for (new_name, old_name) in ress {
            if let Some(id) = resource_name_lookup.get(new_name) {
                errors.push(GraphCompileError::ResourceRedefined {
                    pass: *pass,
                    res: new_name.clone(),
                    prev: resource_defines[id],
                });
                continue 'res;
            }

            let id = ResourceId(resource_defines.len());

            resource_defines.insert(id, *pass);
            resource_name_lookup.insert(new_name.clone(), id);
        }
    }

    // "back-reference" old resources

    for (pass, ress) in input.resource_copies {
        'res: for (new_name, old_name) in ress {
            let old_id = if let Some(id) = resource_name_lookup.get(&old_name) {
                *id
            } else {
                errors.push(GraphCompileError::ReferencedInvalidResource {
                    pass,
                    res: old_name.clone(),
                });
                continue 'res;
            };
            let new_id = if let Some(id) = resource_name_lookup.get(&new_name) {
                *id
            } else {
                errors.push(GraphCompileError::ReferencedInvalidResource {
                    pass,
                    res: new_name.clone(),
                });
                continue 'res;
            };

            resource_copies_from.insert(new_id, old_id);
        }
    }

    for (pass, ress) in input.resource_moves {
        'res: for (new_name, old_name) in ress {
            let old_id = if let Some(id) = resource_name_lookup.get(&old_name) {
                *id
            } else {
                errors.push(GraphCompileError::ReferencedInvalidResource {
                    pass,
                    res: old_name.clone(),
                });
                continue 'res;
            };
            let new_id = if let Some(id) = resource_name_lookup.get(&new_name) {
                *id
            } else {
                errors.push(GraphCompileError::ReferencedInvalidResource {
                    pass,
                    res: new_name.clone(),
                });
                continue 'res;
            };

            resource_moves_from.insert(new_id, old_id);
            resource_moves_to.insert(old_id, new_id);
        }
    }

    for (pass, ress) in input.resource_writes {
        'res: for (name, ty, binding) in ress {
            let id = if let Some(id) = resource_name_lookup.get(&name) {
                *id
            } else {
                errors.push(GraphCompileError::ReferencedInvalidResource {
                    pass,
                    res: name.clone(),
                });
                continue 'res;
            };

            writes.push((id, ty.clone(), pass));

            resource_writes
                .entry(id)
                .or_insert(BTreeSet::new())
                .insert((pass, ty, binding));
        }
    }

    for (pass, ress) in input.resource_reads {
        'res: for (name, ty, binding) in ress {
            let id = if let Some(id) = resource_name_lookup.get(&name) {
                *id
            } else {
                errors.push(GraphCompileError::ReferencedInvalidResource {
                    pass,
                    res: name.clone(),
                });
                continue 'res;
            };

            reads.push((id, ty.clone(), pass));

            resource_reads
                .entry(id)
                .or_insert(BTreeSet::new())
                .insert((pass, ty, binding));
        }
    }

    GraphResourcesResolved {
        name_lookup: resource_name_lookup,

        defines: resource_defines,
        infos: resource_infos,

        copies_from: resource_copies_from,
        moves_from: resource_moves_from,
        moves_to: resource_moves_to,

        reads: resource_reads,
        writes: resource_writes,
    }
}

pub struct ExecutionContext {
    pub reference_size: (u32, u32),
}
