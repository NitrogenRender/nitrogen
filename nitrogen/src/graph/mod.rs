/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Functionalities for describing and implementing render graphs and passes.

use crate::util::storage::{Handle, Storage};
use crate::util::CowString;

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

use crate::resources::shader::ShaderStorage;
use crate::submit_group::{QueueSyncRefs, ResourceList};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

pub(crate) struct Storages<'a> {
    pub shader: &'a RefCell<ShaderStorage>,
    pub render_pass: &'a RefCell<RenderPassStorage>,
    pub pipeline: &'a RefCell<PipelineStorage>,
    pub image: &'a RefCell<ImageStorage>,
    pub buffer: &'a RefCell<BufferStorage>,
    pub vertex_attrib: &'a RefCell<VertexAttribStorage>,
    pub sampler: &'a RefCell<SamplerStorage>,
    pub material: &'a RefCell<MaterialStorage>,
}

/// Opaque handle to a graph.
pub type GraphHandle = Handle<Graph>;

/// Type used to name passes.
pub type PassName = CowString;

/// Type used to name resources.
pub type ResourceName = CowString;

pub(crate) struct ComputePassAccessor {
    pub(crate) describe: Box<dyn Fn(&mut ResourceDescriptor)>,
    pub(crate) execute: Box<dyn Fn(&Store, RawComputeDispatcher) -> Result<(), GraphExecError>>,
}

#[derive(Clone, Debug, From)]
pub enum GraphError {
    CompilationErrors(Vec<String>),
    PrepareError(PrepareError),
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
    pub(crate) res_usage: ResourceUsages,

    pub(crate) pass_resources: PassResources,
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

    pub(crate) unsafe fn create(
        &mut self,
        device: &DeviceContext,
        storages: &mut Storages,
        builder: GraphBuilder,
    ) -> Result<GraphHandle, GraphError> {
        let compiled = compile_graph(builder).map_err(|(names, errors)| {
            let errors = errors.into_iter().map(|err| err.to_diagnostic(&names));
            GraphError::CompilationErrors(errors.collect())
        })?;

        let exec_graph = ExecutionGraph::new(&compiled);

        let pass_resources = {
            let mut res = PassResources::default();

            for pass_id in 0..compiled.pass_names.len() {
                let id = PassId(pass_id);

                let mat = execution::create_pass_material(
                    device,
                    &mut *storages.material.borrow_mut(),
                    &compiled.graph_resources,
                    id,
                )?;

                if let Some(mat) = mat {
                    res.pass_material.insert(id, mat);
                }
            }

            res
        };

        let res_usage = derive_resource_usage(&exec_graph, &compiled);

        let graph = Graph {
            compiled_graph: compiled,
            exec_graph,
            res_usage,

            pass_resources,
        };

        Ok(self.storage.insert(graph))
    }

    pub(crate) fn destroy(
        &mut self,
        res_list: &mut ResourceList,
        storages: &mut Storages,
        handle: GraphHandle,
    ) {
        let graph = self.storage.remove(handle);

        if let Some(graph) = graph {
            graph.pass_resources.release(res_list, storages);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) unsafe fn execute<'a>(
        &'a mut self,
        device: &'a DeviceContext,
        sync: &mut QueueSyncRefs,
        storages: &'a mut Storages<'a>,
        (pool_gfx, pool_cmpt): (&CommandPoolGraphics, &CommandPoolCompute),
        store: &Store,
        graph_handle: GraphHandle,
        res: &mut GraphResources,
        backbuffer: &mut Backbuffer,
        context: &ExecutionContext,
    ) -> Result<(), GraphExecError> {
        let graph = self
            .storage
            .get_mut(graph_handle)
            .ok_or(GraphExecError::InvalidGraph)?;

        let compiled = &graph.compiled_graph;

        // execution context size changed, some resources might need to be recreated
        // TODO!!!

        match res.exec_context.clone() {
            None => {
                // create new resources from scratch
                let mut resources = GraphResources::default();
                resources.exec_context = Some(context.clone());
                // TODO actually create resources lol

                // remove whatever is there.
                let old_res = std::mem::replace(res, resources);
                old_res.release(sync.res_list, storages);
            }
            Some(ref exec) if exec == context => {
                // do nothing?
            }
            _ => {
                // resources do exist but all resources that are contextual have to be
                // recreated.
            }
        }

        if res.exec_context.as_ref() != Some(context) {
            // make new resources.
        }

        // TODO check if current backbuffer is compatible.

        execution::execute(
            device,
            sync,
            (pool_gfx, pool_cmpt),
            storages,
            store,
            graph,
            res,
            context,
        )

        /*
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
