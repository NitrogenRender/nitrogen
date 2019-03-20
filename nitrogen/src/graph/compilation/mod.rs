/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

pub mod input;
pub(crate) use self::input::*;
pub mod resolve;
pub(crate) use self::resolve::*;

use super::{
    PassId, ResourceCreateInfo, ResourceName, ResourceReadType, ResourceType, ResourceWriteType,
};
use crate::graph::builder::{GraphBuilder, PassType};
use crate::graph::builder::resource_descriptor::{ResourceDescriptor, ImageWriteType};
use std::collections::{HashSet, HashMap};
use crate::graph::{PassName, ComputePassAccessor};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub struct ResourceId(pub(crate) usize);

/// Error that can occur during graph compilation.
#[derive(Debug)]
pub enum CompileError {
    /// The graph handle to compile was invalid.
    InvalidGraph,
    /// A resource has been redefined. Shadowing is not allowed.
    ResourceRedefined {
        /// Name of the resource.
        res: ResourceName,
        /// Pass in which the resource has been defined before.
        prev: PassId,
        /// Pass in which the erroneous define is located.
        pass: PassId,
    },
    /// A resource has been named which is not defined anywhere.
    ReferencedInvalidResource {
        /// Name of the invalid resource.
        res: ResourceName,
        /// Pass in which the erroneous resource is referenced.
        pass: PassId,
    },
    /// A resource has been set as a target, but was never defined.
    InvalidTargetResource {
        /// Name of the invalid resource.
        res: ResourceName,
    },
    /// A resource has been used in a way that is not allowed.
    ///
    /// For example, an image resource can be used as a buffer.
    ResourceTypeMismatch {
        /// Name of the resource.
        res: ResourceName,
        /// Pass in which the invalid usage occurs.
        pass: PassId,
        /// Attempted use-kind.
        used_as: ResourceType,
        /// Expected use-kind.
        expected: ResourceType,
    },
    /// A resource has been moved twice.
    ResourceAlreadyMoved {
        /// Name of the resource.
        res: ResourceName,
        /// Pass in which the resource is wrongly moved.
        pass: PassId,
        /// Pass in which the resource has been moved before.
        prev_move: PassId,
    },
}


pub(crate) struct CompiledGraph {
    pub(crate) pass_names: Vec<PassName>,

    pub(crate) compute_passes: HashMap<PassId, ComputePassAccessor>,

    pub(crate) contextual_passes: HashSet<PassId>,
    pub(crate) contextual_resources: HashSet<ResourceId>,

    pub(crate) passes_that_render_to_the_backbuffer: HashSet<PassId>,

    pub(crate) graph_resources: GraphWithNamesResolved,
    pub(crate) targets: HashSet<ResourceId>,
}

pub(crate) fn compile_graph(
    mut builder: GraphBuilder,
) -> Result<CompiledGraph, Vec<CompileError>> {
    let mut errors = vec![];

    let mut input = GraphInput::default();

    let mut pass_names = vec![];
    let mut compute_passes = HashMap::new();

    // collect all pass data in one structure.
    for (name, mut pass) in builder.compute_passes {
        let mut res_desc = ResourceDescriptor::new();

        (pass.describe)(&mut res_desc);

        let pass_num = pass_names.len();
        let id = PassId(pass_num);

        pass_names.push(name);
        compute_passes.insert(id, pass);

        input.add_res_descriptor(id, res_desc, PassType::Compute);
    }

    // replace all resource names with IDs.
    let resolved = resolve_input(input, &mut errors);

    // replace target names with IDs
    let targets = builder.targets
        .iter()
        .filter_map(|res_name| match resolved.name_lookup.get(res_name) {
            None => {
                errors.push(CompileError::InvalidTargetResource {
                     res: res_name.clone(),
                });
                None
            }
            Some(id) => Some(*id),
        })
        .collect();

    // keep track of passes that need their base resources rebuild when the context changes.
    let (contextual_passes, passes_that_render_to_the_backbufer) = {
        let mut contextual = HashSet::new();
        let mut backbuffer = HashSet::new();

        for i in 0..pass_names.len() {
            let id = PassId(i);

            if resolved.is_pass_context_dependent(id) {
                contextual.insert(id);
            }

            for (rid, mode, _) in &resolved.pass_writes[&id] {

                if !resolved.is_backbuffer_resource(*rid) {
                    continue;
                }

                match mode {
                    ResourceWriteType::Image(img_write) => {
                        match img_write {
                            ImageWriteType::Color | ImageWriteType::DepthStencil => {
                                backbuffer.insert(id);
                            },
                            ImageWriteType::Storage => {},
                        }
                    },
                    ResourceWriteType::Buffer(_) => {},
                }
            }
        }

        (contextual, backbuffer)
    };

    // keep track of resources that need to be remade when the context changes.
    let contextual_resources = {
        let mut set = HashSet::new();

        for (id, info) in &resolved.infos {
            if resolved.is_resource_context_dependent(*id) {
                set.insert(*id);
            }
        }

        set
    };

    // replace all target names with IDs
    let compiled_graph = CompiledGraph {
        pass_names,

        contextual_passes,
        contextual_resources,

        passes_that_render_to_the_backbuffer: passes_that_render_to_the_backbufer,

        compute_passes,

        graph_resources: resolved,
        targets,
    };

    if errors.is_empty() {
        Ok(compiled_graph)
    } else {
        Err(errors)
    }
}