/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use std::collections::{BTreeMap, BTreeSet};

use super::*;

use super::GraphInput;

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

    pub(crate) backbuffer: BTreeMap<ResourceName, ResourceId>,
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

pub(crate) fn resolve_input_graph(
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

    for (pass, ress) in &input.resource_moves {
        let creates = pass_creates.entry(*pass).or_insert(BTreeSet::new());
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

    let mut resource_backbuffer = BTreeMap::new();

    for (pass, creates) in input.resource_backbuffer {
        for (bname, lname) in creates {
            if let Some(id) = resource_name_lookup.get(&lname) {
                resource_backbuffer.insert(bname, *id);
            } else {
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
