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

pub mod builder;
pub use self::builder::*;

pub mod pass;
pub use self::pass::*;

pub(crate) mod compilation;
pub(crate) mod execution;

pub(crate) use self::compilation::*;
pub(crate) use self::execution::*;

pub use self::execution::Backbuffer;
pub use self::execution::GraphExecError;
pub use self::execution::PrepareError;

pub use self::compilation::CompileError;

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

pub(crate) struct ComputePassAccessor {
    pub(crate) describe: Box<dyn Fn(&mut ResourceDescriptor)>,
    pub(crate) execute: Box<dyn Fn(&Store, RawComputeDispatcher)>,
}

/// Graphs are "modules" of rendering pipelines.
///
/// A graph should describe a sequence of related rendering-transformations.
///
/// Graphs are conceptually made up of a set of *passes*. Each pass is a single step transformation
/// in the graph.
pub struct Graph {
    pub(crate) compiled_graph: CompiledGraph,
    pub(crate) exec_graph: ExecutionGraph,
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

    pub(crate) fn create(
        &mut self,
        builder: GraphBuilder,
    ) -> Result<GraphHandle, Vec<CompileError>> {

        let compiled = compile_graph(builder)?;

        let exec_graph = ExecutionGraph::new(&compiled);

        let graph = Graph {
            compiled_graph: compiled,
            exec_graph,
        };

        Ok(self.storage.insert(graph))
    }

    pub(crate) fn destroy(
        &mut self,
        res_list: &mut ResourceList,
        storages: &mut Storages,
        handle: GraphHandle,
    ) {
        /*
        let graph = self.storage.remove(handle);

        if let Some(graph) = graph {
            for (_, res) in graph.exec_base_resources {
                res.release(res_list, storages);
            }
        }
        */
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
        /*
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
        */

        Err(GraphExecError::InvalidGraph)
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
