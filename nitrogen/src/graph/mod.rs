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

use crate::resources::image::ImageHandle;
use crate::resources::shader::ShaderStorage;
use crate::submit_group::{QueueSyncRefs, ResourceList};
use std::cell::RefCell;
use std::collections::BTreeMap;

pub(crate) struct Storages<'a> {
    pub shader: &'a RefCell<ShaderStorage>,
    pub render_pass: &'a RefCell<RenderPassStorage>,
    pub pipeline: &'a RefCell<PipelineStorage>,
    pub image: &'a RefCell<ImageStorage>,
    pub buffer: &'a RefCell<BufferStorage>,
    pub sampler: &'a RefCell<SamplerStorage>,
    pub material: &'a RefCell<MaterialStorage>,
}

/// Opaque handle to a graph.
pub type GraphHandle = Handle<Graph>;

/// Type used to name passes.
pub type PassName = CowString;

/// Type used to name resources.
pub type ResourceName = CowString;

// The ComputePass (and GraphicsPass) traits now have associated types, which means it's not
// possible anymore to use `dyn ComputePass` to store the passes (since they can't be named).
//
// Fortunately the `Config` associated type never "escapes" to the *called* interface.
//
// The solution here is to use "accessor closures" which capture the actual value with the
// associated type and perform further dispatch from there.
pub(crate) struct ComputePassAccessor {
    pub(crate) prepare: Box<dyn Fn(&mut Store)>,
    pub(crate) describe: Box<dyn Fn(&mut ResourceDescriptor)>,
    pub(crate) execute: Box<dyn Fn(&Store, RawComputeDispatcher) -> Result<(), GraphExecError>>,
}

// Same explanation as `ComputePassAccessor`
pub(crate) struct GraphicPassAccessor {
    pub(crate) prepare: Box<dyn Fn(&mut Store)>,
    pub(crate) describe: Box<dyn Fn(&mut ResourceDescriptor)>,
    pub(crate) execute: Box<dyn Fn(&Store, RawGraphicsDispatcher) -> Result<(), GraphExecError>>,
}

/// Errors that can occur when dealing with graph preparation/execution.
#[derive(Clone, Debug, From)]
pub enum GraphError {
    /// A set of graph-compilation errors.
    CompilationErrors(Vec<String>),
    /// Error preparing graph- or pass-resources.
    PrepareError(PrepareError),
}

/// Graphs are "modules" of rendering pipelines.
///
/// A graph should describe a sequence of related rendering-transformations.
///
/// Graphs are conceptually made up of a set of *passes*. Each pass is a single step transformation
/// in the graph.
pub struct Graph {
    pub(crate) _name: GraphName,

    pub(crate) compiled_graph: CompiledGraph,
    pub(crate) exec_graph: ExecutionGraph,
    pub(crate) res_usage: ResourceUsages,

    pub(crate) pass_resources: PassResources,

    // TODO generalize when backbuffers have more resource types than images.
    pub(crate) backbuffer_compat: Option<BTreeMap<ResourceName, ImageHandle>>,
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
        let name = builder.name.clone();

        let compiled = compile_graph(builder).map_err(|(names, errors)| {
            let errors = errors.into_iter().map(|err| err.diagnostic(&names));
            GraphError::CompilationErrors(errors.collect())
        })?;

        let exec_graph = ExecutionGraph::new(&compiled);

        let pass_resources = {
            let mut res = PassResources::default();

            for batch in &exec_graph.pass_execution {
                for pass in &batch.passes {
                    let mat = execution::create_pass_material(
                        device,
                        &mut *storages.material.borrow_mut(),
                        &compiled.graph_resources,
                        *pass,
                    )?;

                    if let Some(mat) = mat {
                        res.pass_material.insert(*pass, mat);
                    }

                    // create base resources

                    if compiled.compute_passes.contains_key(pass) {
                        // nothing to do for compute passes. For now, at least.
                    } else {
                        // graphics
                        prepare_graphics_pass_base(device, storages, &mut res, *pass, &compiled)?;
                    }
                }
            }

            res
        };

        let res_usage = derive_resource_usage(&exec_graph, &compiled);

        let graph = Graph {
            _name: name,

            compiled_graph: compiled,
            exec_graph,
            res_usage,

            pass_resources,

            backbuffer_compat: None,
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
        store: &mut Store,
        graph_handle: GraphHandle,
        res: &mut GraphResources,
        backbuffer: &mut Backbuffer,
        context: &ExecutionContext,
    ) -> Result<(), GraphExecError> {
        let graph = self
            .storage
            .get_mut(graph_handle)
            .ok_or(GraphExecError::InvalidGraph)?;

        // graph resources
        match res.exec_context.clone() {
            None => {
                // create new resources from scratch
                let mut resources = GraphResources::default();
                resources.exec_context = Some(context.clone());

                prepare_resources(
                    device,
                    storages,
                    sync.res_list,
                    graph,
                    &mut resources,
                    backbuffer,
                    ResourcePrepareOptions {
                        create_non_contextual: true,
                        create_contextual: true,
                        create_pass_mat: true,
                    },
                    context,
                )?;

                // create graphics stuff.
                prepare_graphics_passes(
                    device,
                    storages,
                    sync.res_list,
                    &mut resources,
                    backbuffer,
                    graph,
                    GraphicsPassPrepareOptions {
                        create_non_contextual: true,
                        create_contextual: true,
                        create_backbuffer: false,
                    },
                )?;

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
                prepare_resources(
                    device,
                    storages,
                    sync.res_list,
                    graph,
                    res,
                    backbuffer,
                    ResourcePrepareOptions {
                        create_non_contextual: false,
                        create_contextual: true,
                        create_pass_mat: false,
                    },
                    context,
                )?;

                prepare_graphics_passes(
                    device,
                    storages,
                    sync.res_list,
                    res,
                    backbuffer,
                    graph,
                    GraphicsPassPrepareOptions {
                        create_non_contextual: false,
                        create_contextual: true,
                        create_backbuffer: false,
                    },
                )?;

                res.exec_context = Some(context.clone());
            }
        }

        // backbuffer compatibility
        {
            let recreate = if let Some(compat) = &graph.backbuffer_compat {
                !backbuffer.is_compatible(compat)
            } else {
                true
            };

            // create passes and make compat struct.
            if recreate {
                prepare_graphics_passes(
                    device,
                    storages,
                    sync.res_list,
                    res,
                    backbuffer,
                    graph,
                    GraphicsPassPrepareOptions {
                        create_non_contextual: false,
                        create_contextual: false,
                        create_backbuffer: true,
                    },
                )?;

                let new_compat = backbuffer
                    .make_compat(&graph.compiled_graph.passes_that_render_to_the_backbuffer);

                if new_compat.is_none() {
                    return Err(GraphExecError::IncompatibleBackbuffer);
                }

                graph.backbuffer_compat = new_compat;
            }
        }

        execution::execute(
            device,
            sync,
            (pool_gfx, pool_cmpt),
            storages,
            store,
            graph,
            res,
        )
    }

    pub(crate) fn resource_id(
        &self,
        handle: GraphHandle,
        name: impl Into<ResourceName>,
    ) -> Option<ResourceId> {
        let graph = self.storage.get(handle)?;

        let id = graph
            .compiled_graph
            .graph_resources
            .name_lookup
            .get(&name.into())?;

        graph.compiled_graph.graph_resources.moved_from(*id)
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
