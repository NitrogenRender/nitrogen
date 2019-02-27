/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use std::collections::{BTreeMap, BTreeSet};

use super::*;

use super::GraphInput;

// the Option<u8> represents a possible sampler binding
pub(crate) type ReadsByResource = (ResourceId, ResourceReadType, u8, Option<u8>);

#[derive(Debug)]
pub(crate) struct GraphResourcesResolved {
    pub(crate) name_lookup: BTreeMap<ResourceName, ResourceId>,
    pub(crate) defines: BTreeMap<ResourceId, PassId>,
    pub(crate) infos: BTreeMap<ResourceId, ResourceCreateInfo>,
    pub(crate) moves_from: BTreeMap<ResourceId, ResourceId>,

    /// Resources created by pass - includes copies and moves
    pub(crate) pass_creates: BTreeMap<PassId, BTreeSet<ResourceId>>,
    /// Resources a pass depends on (that are not created by itself)
    pub(crate) pass_ext_depends: BTreeMap<PassId, BTreeSet<ResourceId>>,
    /// Resources that a pass writes to
    pub(crate) pass_writes: BTreeMap<PassId, BTreeSet<(ResourceId, ResourceWriteType, u8)>>,
    /// Resources that a pass reads from
    pub(crate) pass_reads: BTreeMap<PassId, BTreeSet<ReadsByResource>>,
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
        } else {
            None
        }
    }

    pub(crate) fn create_info(&self, id: ResourceId) -> Option<(ResourceId, &ResourceCreateInfo)> {
        let next_id = self.moved_from(id)?;

        if let Some(info) = self.infos.get(&next_id) {
            return Some((next_id, info));
        } else {
            return None;
        }
    }
}

// This function looks way scarier than it is. (really!)
// Its purpose is to go from collections of names to collections of IDs and a lookup table
// to associate names with IDs.
//
// In the first pass, all new "definitions" are associated with IDs.
// In the second pass, all "mentions" of resources by name are replaced by the now-known IDs.
//
// All this happens in two passes since "usage of resource" does not have to "physically" appear
// after the definition.
pub(crate) fn resolve_input_graph(
    input: GraphInput,
    reads: &mut Vec<(ResourceId, ResourceReadType, PassId)>,
    writes: &mut Vec<(ResourceId, ResourceWriteType, PassId)>,
    errors: &mut Vec<GraphCompileError>,
) -> GraphResourcesResolved {
    // TODO check for duplicated binding points everywhere?
    // TODO cycle detection?

    let mut resource_name_lookup = BTreeMap::new();

    let mut resource_defines = BTreeMap::new();
    let mut resource_infos = BTreeMap::new();
    let mut resource_moves_from = BTreeMap::new();
    let mut resource_moves_to = BTreeMap::new();

    let mut pass_creates = BTreeMap::<_, BTreeSet<_>>::new();
    let mut pass_ext_depends = BTreeMap::<_, BTreeSet<_>>::new();
    let mut pass_writes = BTreeMap::<_, BTreeSet<_>>::new();
    let mut pass_reads = BTreeMap::<_, BTreeSet<_>>::new();

    // generate IDs for all "new" resources.

    for (pass, ress) in input.resource_creates {
        let creates = pass_creates.entry(pass).or_default();

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

    for (pass, ress) in &input.resource_moves {
        let creates = pass_creates.entry(*pass).or_default();
        for (new_name, _old_name) in ress {
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

    for (pass, ress) in input.resource_moves {
        let depends = pass_ext_depends.entry(pass).or_default();

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
        let depends = pass_ext_depends.entry(pass).or_default();
        let pass_writes = pass_writes.entry(pass).or_default();

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

            writes.push((id, ty, pass));

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
        let depends = pass_ext_depends.entry(pass).or_default();
        let pass_reads = pass_reads.entry(pass).or_default();

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

            reads.push((id, ty, pass));

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

    for (pass, creates) in input.resource_backbuffer {
        for (_bname, lname) in creates {
            if resource_name_lookup.get(&lname).is_none() {
                errors.push(GraphCompileError::ReferencedInvalidResource {
                    res: lname.clone(),
                    pass,
                });
                continue;
            }
        }
    }

    GraphResourcesResolved {
        name_lookup: resource_name_lookup,

        defines: resource_defines,
        infos: resource_infos,

        moves_from: resource_moves_from,

        pass_creates,
        pass_ext_depends,
        pass_reads,
        pass_writes,
    }
}
