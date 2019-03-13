/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Functionalities for describing and implementing render graphs and passes.

use crate::util::storage::{Handle, Storage};
use crate::util::CowString;

use std::collections::HashMap;

use smallvec::SmallVec;

use crate::device::DeviceContext;
use crate::resources::{
    buffer::BufferStorage,
    command_pool::{CommandPoolCompute, CommandPoolGraphics},
    image::ImageStorage,
    material::MaterialStorage,
    pipeline::PipelineStorage,
    render_pass::RenderPassStorage,
    sampler::SamplerStorage,
    semaphore_pool::{SemaphoreList, SemaphorePool},
    vertex_attrib::VertexAttribStorage,
};

pub mod pass;
pub use self::pass::*;

pub mod builder;
pub use self::builder::*;

pub mod command;
pub use self::command::*;

pub(crate) mod compilation;
pub(crate) mod execution;

pub(crate) use self::compilation::*;
pub(crate) use self::execution::*;

pub use self::execution::Backbuffer;
pub use self::execution::GraphExecError;
pub use self::execution::PrepareError;

pub use self::compilation::GraphCompileError;

pub mod store;
pub use self::store::*;

use crate::submit_group::ResourceList;

pub(crate) struct Storages<'a> {
    pub render_pass: &'a mut RenderPassStorage,
    pub pipeline: &'a mut PipelineStorage,
    pub image: &'a mut ImageStorage,
    pub buffer: &'a mut BufferStorage,
    pub vertex_attrib: &'a VertexAttribStorage,
    pub sampler: &'a mut SamplerStorage,
    pub material: &'a mut MaterialStorage,
}

/// Opaque handle to a graph.
pub type GraphHandle = Handle<Graph>;

/// Type used to name passes.
pub type PassName = CowString;

/// Type used to name resources.
pub type ResourceName = CowString;

/// Graphs are "modules" of rendering pipelines.
///
/// A graph should describe a sequence of related rendering-transformations.
///
/// Graphs are conceptually made up of a set of *passes*. Each pass is a single step transformation
/// in the graph.
///
/// A Graph has to be compiled before usage, as compiling does resolution of resource dependencies
/// as well as create the necessary resources up-front.
#[derive(Default)]
pub struct Graph {
    passes: Vec<(PassName, PassInfo)>,
    passes_gfx_impl: HashMap<usize, Box<dyn GraphicsPassImpl>>,
    passes_cmpt_impl: HashMap<usize, Box<dyn ComputePassImpl>>,
    pub(crate) output_resources: Vec<ResourceName>,

    pub(crate) resolve_cache: HashMap<u64, (GraphResourcesResolved, usize)>,
    exec_id_cache: HashMap<(usize, Vec<ResourceName>), usize>,
    exec_graph_cache: HashMap<usize, ExecutionGraph>,

    exec_usages: HashMap<usize, ResourceUsages>,
    pub(crate) exec_base_resources: HashMap<usize, GraphBaseResources>,

    last_exec: Option<usize>,
    pub(crate) last_input: Option<u64>,
}

pub(crate) struct GraphStorage {
    pub(crate) storage: Storage<Graph>,
}

impl GraphStorage {
    pub(crate) fn new() -> Self {
        GraphStorage {
            storage: Storage::new(),
        }
    }

    pub(crate) fn create(&mut self) -> GraphHandle {
        self.storage.insert(Graph::default())
    }

    pub(crate) fn destroy(
        &mut self,
        res_list: &mut ResourceList,
        storages: &mut Storages,
        handle: GraphHandle,
    ) {
        let graph = self.storage.remove(handle);

        if let Some(graph) = graph {
            for (_, res) in graph.exec_base_resources {
                res.release(res_list, storages);
            }
        }
    }

    pub(crate) fn add_graphics_pass<T: Into<PassName>>(
        &mut self,
        handle: GraphHandle,
        name: T,
        pass_info: GraphicsPassInfo,
        pass_impl: Box<dyn GraphicsPassImpl>,
    ) {
        if let Some(graph) = self.storage.get_mut(handle) {
            let id = graph.passes.len();
            graph
                .passes
                .push((name.into(), PassInfo::Graphics(pass_info)));
            graph.passes_gfx_impl.insert(id, pass_impl);
        }
    }

    pub(crate) fn add_compute_pass<T: Into<PassName>>(
        &mut self,
        handle: GraphHandle,
        name: T,
        pass_info: ComputePassInfo,
        pass_impl: Box<dyn ComputePassImpl>,
    ) {
        if let Some(graph) = self.storage.get_mut(handle) {
            let id = graph.passes.len();
            graph
                .passes
                .push((name.into(), PassInfo::Compute(pass_info)));
            graph.passes_cmpt_impl.insert(id, pass_impl);
        }
    }

    /// Compile the graph so it is optimized for execution.
    ///
    /// This runs all the `setup` methods on all passes (`GraphicsPassImpl`, `ComputePassImpl`)
    /// and checks if the "same graph version" has been encountered before. If so, the old compiled
    /// representation can be reused.
    ///
    /// If this is a new graph permutation then all the resource names are resolved to IDs and lists
    /// are created that store which pass creates, reads, writes or moves which resource.
    ///
    /// A "execution path" representation is then created from that information, only including
    /// relevant passes for the current permutation.
    ///
    /// With that in place, any gfx resources are created that won't change across execution
    /// (like pipelines and render passes)
    pub(crate) fn compile(
        &mut self,
        store: &mut Store,
        handle: GraphHandle,
    ) -> Result<(), Vec<GraphCompileError>> {
        let graph = self
            .storage
            .get_mut(handle)
            .ok_or_else(|| vec![GraphCompileError::InvalidGraph])?;

        let mut input = GraphInput::default();

        for (i, pass) in graph.passes_gfx_impl.iter_mut() {
            let mut builder = GraphBuilder::new();
            pass.setup(store, &mut builder);

            let id = PassId(*i);

            if builder.enabled {
                input.add_builder(id, builder);
            }
        }

        for (i, pass) in graph.passes_cmpt_impl.iter_mut() {
            let mut builder = GraphBuilder::new();
            pass.setup(store, &mut builder);

            let id = PassId(*i);

            if builder.enabled {
                input.add_builder(id, builder);
            }
        }

        // TODO hash the above things and make a lookup table to spare doing the work below.

        let mut errors = vec![];
        // TODO are those needed in future? Right now they don't do anything except use heap allocs
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

    pub(crate) unsafe fn execute(
        &mut self,
        device: &DeviceContext,
        sem_pool: &SemaphorePool,
        sem_list: &mut SemaphoreList,
        cmd_pool_gfx: &CommandPoolGraphics,
        cmd_pool_cmpt: &CommandPoolCompute,
        res_list: &mut ResourceList,
        storages: &mut Storages,
        store: &Store,
        graph_handle: GraphHandle,
        backbuffer: &mut Backbuffer,
        prev_res: Option<GraphResources>,
        context: &ExecutionContext,
    ) -> Result<GraphResources, GraphExecError> {
        let graph = self
            .storage
            .get_mut(graph_handle)
            .ok_or(GraphExecError::InvalidGraph)?;

        let input_hash = graph.last_input.ok_or(GraphExecError::GraphNotCompiled)?;

        let (resolved, id) = graph
            .resolve_cache
            .get(&input_hash)
            .ok_or(GraphExecError::GraphNotCompiled)?;

        let exec_id = graph
            .exec_id_cache
            .get(&(*id, graph.output_resources.clone()))
            .ok_or(GraphExecError::GraphNotCompiled)?;

        let exec = &graph.exec_graph_cache[exec_id];

        let outputs = graph
            .output_resources
            .as_slice()
            .iter()
            .map(|n| resolved.name_lookup[n])
            .collect::<SmallVec<[_; 16]>>();

        let resources = {
            let (base_resources, usages) = {
                let passes = &graph.passes[..];

                // insert into cache
                graph
                    .exec_base_resources
                    .entry(*exec_id)
                    .or_insert_with(|| {
                        execution::prepare_base(
                            device,
                            &backbuffer.usage,
                            storages,
                            exec,
                            resolved,
                            passes,
                        )
                    });

                // now read base again (some kind of reborrowing, need to investigate...)
                let base = graph
                    .exec_base_resources
                    .get_mut(exec_id)
                    .ok_or(GraphExecError::GraphNotCompiled)?;

                graph.exec_usages.entry(*exec_id).or_insert_with(|| {
                    execution::derive_resource_usage(
                        &backbuffer.usage,
                        exec,
                        resolved,
                        outputs.as_slice(),
                    )
                });

                let usages = &graph.exec_usages[exec_id];

                (base, usages)
            };

            match prev_res {
                None => {
                    // create new completely!
                    let mut res = prepare(
                        usages,
                        backbuffer,
                        base_resources,
                        device,
                        storages,
                        exec,
                        resolved,
                        graph.passes.as_slice(),
                        context.clone(),
                    )?;

                    // add the resolved outputs
                    res.outputs = outputs;
                    res.exec_version = *exec_id;
                    res
                }
                Some(res) => {
                    if Some(res.exec_version) == graph.last_exec && &res.exec_context == context {
                        // same old resources, we can keep them!

                        res
                    } else {
                        // recreate resources
                        res.release(res_list, storages);

                        let mut res = prepare(
                            usages,
                            backbuffer,
                            base_resources,
                            device,
                            storages,
                            exec,
                            resolved,
                            graph.passes.as_slice(),
                            context.clone(),
                        )?;

                        // add the resolved outputs
                        res.outputs = outputs;
                        res.exec_version = *exec_id;
                        res
                    }
                }
            }
        };

        execution::execute(
            device,
            sem_pool,
            sem_list,
            cmd_pool_gfx,
            cmd_pool_cmpt,
            storages,
            store,
            exec,
            resolved,
            graph,
            &graph.exec_base_resources[exec_id],
            &resources,
            context,
        );

        Ok(resources)
    }

    pub(crate) fn add_output<T: Into<ResourceName>>(&mut self, handle: GraphHandle, image: T) {
        if let Some(graph) = self.storage.get_mut(handle) {
            graph.output_resources.push(image.into());
        }
    }
}

/// Reference data used during graph executions.
#[derive(Debug, Clone, Ord, PartialOrd, PartialEq, Eq)]
pub struct ExecutionContext {
    /// Reference size of the current execution.
    ///
    /// These values are used as the reference when [`ImageSizeMode::ContextRelative`] is used.
    ///
    /// [`ImageSizeMode::ContextRelative`]: ../resources/image/enum.ImageSizeMode.html#variant.ContextRelative
    pub reference_size: (u32, u32),
}
