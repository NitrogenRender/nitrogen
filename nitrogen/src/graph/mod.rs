/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use util::storage::{Handle, Storage};
use util::CowString;

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;

use smallvec::SmallVec;

use device::DeviceContext;
use resources::{
    buffer::BufferStorage,
    image::ImageStorage,
    material::MaterialStorage,
    pipeline::PipelineStorage,
    render_pass::RenderPassStorage,
    sampler::SamplerStorage,
    semaphore_pool::{SemaphoreList, SemaphorePool},
    vertex_attrib::VertexAttribStorage,
};

use types::CommandPool;

pub mod pass;
pub use self::pass::*;

pub mod builder;
pub use self::builder::*;

pub mod command;
pub use self::command::*;

mod execution;
pub use self::execution::ExecutionResources;
use self::execution::*;
use submit_group::ResourceList;

pub type GraphHandle = Handle<Graph>;

pub type PassName = CowString;
pub type ResourceName = CowString;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub struct ResourceId(pub(crate) usize);

#[derive(Debug)]
pub(crate) struct GraphResourcesResolved {
    pub(crate) name_lookup: BTreeMap<ResourceName, ResourceId>,
    pub(crate) defines: BTreeMap<ResourceId, PassId>,
    pub(crate) infos: BTreeMap<ResourceId, ResourceCreateInfo>,
    pub(crate) copies_from: BTreeMap<ResourceId, ResourceId>,
    pub(crate) moves_from: BTreeMap<ResourceId, ResourceId>,
    pub(crate) moves_to: BTreeMap<ResourceId, ResourceId>,

    pub(crate) reads: BTreeMap<ResourceId, BTreeSet<(PassId, ResourceReadType, u8, Option<u8>)>>,
    pub(crate) writes: BTreeMap<ResourceId, BTreeSet<(PassId, ResourceWriteType, u8)>>,

    /// Resources created by pass - includes copies and moves
    pub(crate) pass_creates: BTreeMap<PassId, BTreeSet<ResourceId>>,
    /// Resources a pass depends on (that are not created by itself)
    pub(crate) pass_ext_depends: BTreeMap<PassId, BTreeSet<ResourceId>>,
    /// Resources that a pass writes to
    pub(crate) pass_writes: BTreeMap<PassId, BTreeSet<(ResourceId, ResourceWriteType, u8)>>,
    /// Resources that a pass reads from
    ///
    /// Last entry is the binding point for samplers
    pub(crate) pass_reads:
        BTreeMap<PassId, BTreeSet<(ResourceId, ResourceReadType, u8, Option<u8>)>>,

    pub(crate) backbuffer: BTreeSet<ResourceId>,
}

impl GraphResourcesResolved {
    pub(crate) fn moved_from(&self, id: ResourceId) -> Option<ResourceId> {
        let mut prev_id = id;

        // Go up the move chain until we reach the end
        while let Some(id) = self.moves_from.get(&prev_id) {
            prev_id = *id;
        }

        // Check if there's a resource
        if self.infos.contains_key(&prev_id) {
            Some(prev_id)
        } else if self.copies_from.contains_key(&prev_id) {
            Some(prev_id)
        } else {
            None
        }
    }

    pub(crate) fn create_info(&self, id: ResourceId) -> Option<(ResourceId, &ResourceCreateInfo)> {
        let mut next_id = self.moved_from(id)?;

        // I'm tired and not sure if this will terminate in all cases. TODO...
        loop {
            if let Some(info) = self.infos.get(&next_id) {
                return Some((next_id, info));
            }

            if let Some(prev) = self.copies_from.get(&next_id) {
                next_id = self.moved_from(*prev)?;
            } else {
                return None;
            }
        }
    }
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
    InvalidOutputResource {
        res: ResourceName,
    },
    ResourceTypeMismatch {
        res: ResourceName,
        pass: PassId,
        used_as: ResourceType,
        expected: ResourceType,
    },
    ResourceAlreadyMoved {
        res: ResourceName,
        pass: PassId,
        prev_move: PassId,
    },
}

pub struct Graph {
    passes: Vec<(PassName, PassInfo)>,
    passes_impl: Vec<Box<dyn PassImpl>>,
    pub(crate) output_resources: Vec<ResourceName>,

    resolve_cache: HashMap<u64, (GraphResourcesResolved, usize)>,
    exec_id_cache: HashMap<(usize, Vec<ResourceName>), usize>,
    exec_graph_cache: HashMap<usize, ExecutionGraph>,

    exec_resources: HashMap<usize, ExecutionGraphResources>,

    last_exec: Option<usize>,
    last_input: Option<u64>,
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
                exec_id_cache: HashMap::new(),
                exec_graph_cache: HashMap::new(),
                exec_resources: HashMap::new(),
                last_exec: None,
                last_input: None,
            })
            .0
    }

    pub fn destroy(
        &mut self,
        device: &DeviceContext,
        render_pass_storage: &mut RenderPassStorage,
        pipeline_storage: &mut PipelineStorage,
        image_storage: &mut ImageStorage,
        buffer_storage: &mut BufferStorage,
        vertex_attrib_storage: &VertexAttribStorage,
        sampler_storage: &mut SamplerStorage,
        material_storage: &MaterialStorage,
        handle: GraphHandle,
    ) {
        let graph = self.storage.remove(handle);

        let mut storages = execution::ExecutionStorages {
            render_pass: render_pass_storage,
            pipeline: pipeline_storage,
            image: image_storage,
            buffer: buffer_storage,
            vertex_attrib: vertex_attrib_storage,
            sampler: sampler_storage,
            material: material_storage,
        };

        if let Some(graph) = graph {
            for (_, res) in graph.exec_resources {
                res.release(device, &mut storages);
            }
        }
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
            let mut pass_resource_backbuffers = Vec::new();

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

                    for res in builder.resource_backbuffer {
                        pass_resource_backbuffers.push((id, res));
                    }
                }
            }

            GraphInput {
                resource_creates: pass_resource_creates,
                resource_copies: pass_resource_copies,
                resource_moves: pass_resource_moves,

                resource_writes: pass_resource_writes,
                resource_reads: pass_resource_reads,

                resource_backbuffer: pass_resource_backbuffers,
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

        graph.last_input = Some(input_hash);

        // TODO check write and read types match with creation types.

        let exec_id = graph.exec_id_cache.len();

        let exec_id = *graph
            .exec_id_cache
            .entry((*resolved_id, graph.output_resources.clone()))
            .or_insert(exec_id);

        let output_slice = graph.output_resources.as_slice();

        for output in output_slice {
            if !resolved.name_lookup.contains_key(output) {
                errors.push(GraphCompileError::InvalidOutputResource {
                    res: output.clone(),
                });
            }
        }

        graph
            .exec_graph_cache
            .entry(exec_id)
            .or_insert_with(|| ExecutionGraph::new(resolved, output_slice));

        graph.last_exec = Some(exec_id);

        if !errors.is_empty() {
            Err(errors)
        } else {
            Ok(())
        }
    }

    pub fn execute(
        &mut self,
        device: &DeviceContext,
        sem_pool: &mut SemaphorePool,
        sem_list: &mut SemaphoreList,
        cmd_pool: &mut CommandPool<gfx::Graphics>,
        res_list: &mut ResourceList,
        render_pass_storage: &mut RenderPassStorage,
        pipeline_storage: &mut PipelineStorage,
        image_storage: &mut ImageStorage,
        buffer_storage: &mut BufferStorage,
        vertex_storage: &VertexAttribStorage,
        sampler_storage: &mut SamplerStorage,
        material_storage: &MaterialStorage,
        graph: GraphHandle,
        context: &ExecutionContext,
    ) -> ExecutionResources {
        // TODO error handling !!!
        // TODO
        // TODO
        let graph = self.storage.get_mut(graph).expect("Invalid graph!");

        let input_hash = graph.last_input.unwrap();

        let (resolved, id) = graph.resolve_cache.get(&input_hash).unwrap();

        let exec_id = graph
            .exec_id_cache
            .get(&(*id, graph.output_resources.clone()))
            .unwrap();

        let exec = &graph.exec_graph_cache[exec_id];

        let mut storages = execution::ExecutionStorages {
            render_pass: render_pass_storage,
            pipeline: pipeline_storage,
            image: image_storage,
            buffer: buffer_storage,
            vertex_attrib: vertex_storage,
            sampler: sampler_storage,
            material: material_storage,
        };

        let outputs = graph
            .output_resources
            .as_slice()
            .iter()
            .map(|n| resolved.name_lookup[n])
            .collect::<SmallVec<[_; 16]>>();

        let resources = {
            let passes = &graph.passes[..];

            graph.exec_resources.entry(*exec_id).or_insert_with(|| {
                execution::prepare(
                    device,
                    &mut storages,
                    exec,
                    resolved,
                    passes,
                    outputs.as_slice(),
                    context,
                )
            });

            // TODO Aaaargh there's no immutable insert_with for HashMap :/
            &graph.exec_resources[exec_id]
        };

        execution::execute(
            device,
            sem_pool,
            sem_list,
            cmd_pool,
            res_list,
            &mut storages,
            exec,
            resolved,
            graph,
            &resources,
            context,
        )
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

    resource_reads: BTreeMap<PassId, Vec<(ResourceName, ResourceReadType, u8, Option<u8>)>>,
    resource_writes: BTreeMap<PassId, Vec<(ResourceName, ResourceWriteType, u8)>>,

    resource_backbuffer: Vec<(PassId, ResourceName)>,
}

fn resolve_input_graph(
    input: GraphInput,
    reads: &mut Vec<(ResourceId, ResourceReadType, PassId)>,
    writes: &mut Vec<(ResourceId, ResourceWriteType, PassId)>,
    errors: &mut Vec<GraphCompileError>,
) -> GraphResourcesResolved {
    // TODO check for duplicated binding points everywhere?

    let mut resource_name_lookup = BTreeMap::new();

    let mut resource_defines = BTreeMap::new();
    let mut resource_infos = BTreeMap::new();
    let mut resource_copies_from = BTreeMap::new();
    let mut resource_moves_from = BTreeMap::new();
    let mut resource_moves_to = BTreeMap::new();

    let mut resource_reads = BTreeMap::new();
    let mut resource_writes = BTreeMap::new();

    let mut pass_creates = BTreeMap::new();
    let mut pass_ext_depends = BTreeMap::new();
    let mut pass_writes = BTreeMap::new();
    let mut pass_reads = BTreeMap::new();

    // generate IDs for all "new" resources.

    for (pass, ress) in input.resource_creates {
        let creates = pass_creates.entry(pass).or_insert(BTreeSet::new());

        for (name, info) in ress {
            if let Some(id) = resource_name_lookup.get(&name) {
                errors.push(GraphCompileError::ResourceRedefined {
                    pass,
                    res: name.clone(),
                    prev: resource_defines[id],
                });
                continue;
            }

            let id = ResourceId(resource_defines.len());
            resource_defines.insert(id, pass);
            resource_infos.insert(id, info);
            resource_name_lookup.insert(name, id);

            creates.insert(id);
        }
    }

    for (pass, ress) in &input.resource_copies {
        let creates = pass_creates.entry(*pass).or_insert(BTreeSet::new());
        for (new_name, old_name) in ress {
            if let Some(id) = resource_name_lookup.get(new_name) {
                errors.push(GraphCompileError::ResourceRedefined {
                    pass: *pass,
                    res: new_name.clone(),
                    prev: resource_defines[id],
                });
                continue;
            }

            let id = ResourceId(resource_defines.len());

            resource_defines.insert(id, *pass);
            resource_name_lookup.insert(new_name.clone(), id);

            creates.insert(id);
        }
    }

    for (pass, ress) in &input.resource_moves {
        let creates = pass_creates.entry(*pass).or_insert(BTreeSet::new());
        for (new_name, old_name) in ress {
            if let Some(id) = resource_name_lookup.get(new_name) {
                errors.push(GraphCompileError::ResourceRedefined {
                    pass: *pass,
                    res: new_name.clone(),
                    prev: resource_defines[id],
                });
                continue;
            }

            let id = ResourceId(resource_defines.len());

            resource_defines.insert(id, *pass);
            resource_name_lookup.insert(new_name.clone(), id);

            creates.insert(id);
        }
    }

    // "back-reference" old resources

    for (pass, ress) in input.resource_copies {
        let depends = pass_ext_depends.entry(pass).or_insert(BTreeSet::new());

        for (new_name, old_name) in ress {
            let old_id = if let Some(id) = resource_name_lookup.get(&old_name) {
                *id
            } else {
                errors.push(GraphCompileError::ReferencedInvalidResource {
                    pass,
                    res: old_name.clone(),
                });
                continue;
            };
            let new_id = if let Some(id) = resource_name_lookup.get(&new_name) {
                *id
            } else {
                errors.push(GraphCompileError::ReferencedInvalidResource {
                    pass,
                    res: new_name.clone(),
                });
                continue;
            };

            resource_copies_from.insert(new_id, old_id);

            // If the old id was something that is made in another pass it means we depend on
            // another pass
            if !pass_creates
                .get(&pass)
                .map(|s| s.contains(&old_id))
                .unwrap_or(false)
            {
                depends.insert(old_id);
            }
        }
    }

    for (pass, ress) in input.resource_moves {
        let depends = pass_ext_depends.entry(pass).or_insert(BTreeSet::new());

        for (new_name, old_name) in ress {
            let old_id = if let Some(id) = resource_name_lookup.get(&old_name) {
                *id
            } else {
                errors.push(GraphCompileError::ReferencedInvalidResource {
                    pass,
                    res: old_name.clone(),
                });
                continue;
            };
            let new_id = if let Some(id) = resource_name_lookup.get(&new_name) {
                *id
            } else {
                errors.push(GraphCompileError::ReferencedInvalidResource {
                    pass,
                    res: new_name.clone(),
                });
                continue;
            };

            if let Some(prev_res) = resource_moves_to.get(&old_id) {
                if let Some(prev_pass) = resource_defines.get(prev_res) {
                    errors.push(GraphCompileError::ResourceAlreadyMoved {
                        res: old_name.clone(),
                        pass,
                        prev_move: *prev_pass,
                    });
                    continue;
                } else {
                    unreachable!("Moved a VALID resource that was never created??");
                }
            }

            resource_moves_from.insert(new_id, old_id);
            resource_moves_to.insert(old_id, new_id);

            // If the old id was something that is made in another pass it means we depend on
            // another pass
            if !pass_creates
                .get(&pass)
                .map(|s| s.contains(&old_id))
                .unwrap_or(false)
            {
                depends.insert(old_id);
            }
        }
    }

    for (pass, ress) in input.resource_writes {
        let depends = pass_ext_depends.entry(pass).or_insert(BTreeSet::new());
        let pass_writes = pass_writes.entry(pass).or_insert(BTreeSet::new());

        for (name, ty, binding) in ress {
            let id = if let Some(id) = resource_name_lookup.get(&name) {
                *id
            } else {
                errors.push(GraphCompileError::ReferencedInvalidResource {
                    pass,
                    res: name.clone(),
                });
                continue;
            };

            writes.push((id, ty.clone(), pass));

            resource_writes
                .entry(id)
                .or_insert(BTreeSet::new())
                .insert((pass, ty.clone(), binding));

            pass_writes.insert((id, ty, binding));

            // If the id is something that is made in another pass it means we depend on another
            // pass
            if !pass_creates
                .get(&pass)
                .map(|s| s.contains(&id))
                .unwrap_or(false)
            {
                depends.insert(id);
            }
        }
    }

    for (pass, ress) in input.resource_reads {
        let depends = pass_ext_depends.entry(pass).or_insert(BTreeSet::new());
        let pass_reads = pass_reads.entry(pass).or_insert(BTreeSet::new());

        for (name, ty, binding, sampler_binding) in ress {
            let id = if let Some(id) = resource_name_lookup.get(&name) {
                *id
            } else {
                errors.push(GraphCompileError::ReferencedInvalidResource {
                    pass,
                    res: name.clone(),
                });
                continue;
            };

            reads.push((id, ty.clone(), pass));

            resource_reads.entry(id).or_insert(BTreeSet::new()).insert((
                pass,
                ty.clone(),
                binding,
                sampler_binding,
            ));

            pass_reads.insert((id, ty, binding, sampler_binding));

            // If the id is something that is made in another pass it means we depend on another
            // pass
            if !pass_creates
                .get(&pass)
                .map(|s| s.contains(&id))
                .unwrap_or(false)
            {
                depends.insert(id);
            }
        }
    }

    let mut resource_backbuffer = BTreeSet::new();
    for (pass, res) in input.resource_backbuffer {
        if let Some(id) = resource_name_lookup.get(&res) {
            resource_backbuffer.insert(*id);
        } else {
            errors.push(GraphCompileError::ReferencedInvalidResource {
                res: res.clone(),
                pass,
            });
            continue;
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

        backbuffer: resource_backbuffer,

        pass_creates,
        pass_ext_depends,
        pass_reads,
        pass_writes,
    }
}

#[derive(Debug, Clone, Ord, PartialOrd, PartialEq, Eq)]
pub struct ExecutionContext {
    pub reference_size: (u32, u32),
}
