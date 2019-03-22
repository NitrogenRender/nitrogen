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
use crate::graph::builder::resource_descriptor::{ImageWriteType, ResourceDescriptor};
use crate::graph::builder::{GraphBuilder, PassType};
use crate::graph::{ComputePassAccessor, PassName};
use core::borrow::Borrow;
use std::collections::{HashMap, HashSet};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub struct ResourceId(pub(crate) usize);

/// Error that can occur during graph compilation.
#[derive(Debug, Clone)]
pub enum CompileError {
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
        /// Name of the moved resource.
        attempted_new_name: ResourceName,
        /// Pass in which the resource is wrongly moved.
        pass: PassId,
        /// Pass in which the resource has been moved before.
        prev_move: PassId,
    },
}

impl CompileError {
    pub(crate) fn to_diagnostic(self, pass_names: &Vec<PassName>) -> String {
        match self {
            CompileError::ResourceRedefined { res, prev, pass } => {
                let prev_name = pass_names[prev.0].clone();
                let pass_name = pass_names[pass.0].clone();

                format!(
                    "Resource \"{}\" was defined in pass \"{}\" but \
                     redefined in pass \"{}\". Shadowing is not permitted.",
                    res, prev_name, pass_name,
                )
            }
            CompileError::ReferencedInvalidResource { res, pass } => {
                let pass_name = pass_names[pass.0].clone();

                format!(
                    "Resource \"{}\" was not defined but used in pass \"{}\".",
                    res, pass_name
                )
            }
            CompileError::InvalidTargetResource { res } => format!(
                "Resource \"{}\" was set as a target resource but is not defined in the graph.",
                res
            ),
            CompileError::ResourceTypeMismatch {
                res,
                pass,
                used_as,
                expected,
            } => {
                let pass_name = pass_names[pass.0].clone();

                format!(
                    "Invalid resource \"{}\" usage in pass \"{}\". Expected {:?} but got {:?}.",
                    res, pass_name, expected, used_as,
                )
            }
            CompileError::ResourceAlreadyMoved {
                res,
                attempted_new_name,
                pass,
                prev_move,
            } => {
                let pass_name = pass_names[pass.0].clone();
                let prev_move_pass = pass_names[prev_move.0].clone();

                format!("Attempted move of resource \"{}\" to \"{}\" in pass \"{}\", but resource was moved before in pass \"{}\".",
                res,
                attempted_new_name,
                pass_name,
                prev_move_pass,)
            }
        }
    }
}

pub(crate) struct CompiledGraph {
    pub(crate) pass_names: Vec<PassName>,

    pub(crate) compute_passes: HashMap<PassId, ComputePassAccessor>,

    pub(crate) contextual_passes: HashSet<PassId>,
    pub(crate) contextual_resources: HashSet<ResourceId>,

    pub(crate) passes_that_render_to_the_backbuffer: HashMap<PassId, Vec<ResourceName>>,

    pub(crate) graph_resources: GraphWithNamesResolved,
    pub(crate) targets: HashSet<ResourceId>,
}

pub(crate) fn compile_graph(
    mut builder: GraphBuilder,
) -> Result<CompiledGraph, (Vec<PassName>, Vec<CompileError>)> {
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
    let targets = builder
        .targets
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
        let mut backbuffer = HashMap::<PassId, Vec<ResourceName>>::new();

        for i in 0..pass_names.len() {
            let id = PassId(i);

            let dep = resolved.pass_dependency(id);

            if dep.context {
                contextual.insert(id);
            }

            if let Some(deps) = dep.backbuffer {
                backbuffer.entry(id).or_default().extend(deps);
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

    if errors.is_empty() {
        Ok(CompiledGraph {
            pass_names,

            contextual_passes,
            contextual_resources,

            passes_that_render_to_the_backbuffer: passes_that_render_to_the_backbufer,

            compute_passes,

            graph_resources: resolved,
            targets,
        })
    } else {
        Err((pass_names, errors))
    }
}
