/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use std::collections::{BTreeMap, BTreeSet};

use super::*;

use super::GraphInput;
use crate::graph::builder::resource_descriptor::ImageInfo;
use crate::graph::pass::dispatcher::ResourceAccessType;
use crate::graph::PassType;

// the Option<u8> represents a possible sampler binding
pub(crate) type ReadsByResource = (ResourceId, ResourceReadType, u8, Option<u8>);

pub(crate) struct PassDependency {
    pub(crate) context: bool,
    // list of backbuffer names that will be written to.
    pub(crate) backbuffer: Option<Vec<ResourceName>>,
}

#[derive(Debug)]
pub(crate) struct GraphWithNamesResolved {
    pub(crate) name_lookup: BTreeMap<ResourceName, ResourceId>,

    pub(crate) pass_types: BTreeMap<PassId, PassType>,

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

impl GraphWithNamesResolved {
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

    pub(crate) fn resource_access_type(
        &self,
        pass: PassId,
        res: ResourceId,
    ) -> Option<ResourceAccessType> {
        let res_type = self.infos.get(&res)?.into();

        let writes = self.pass_writes.get(&pass)?;
        let reads = self.pass_reads.get(&pass)?;

        let is_written = writes.iter().any(|(id, _, _)| *id == res);

        if is_written {
            return Some(ResourceAccessType::Write(res_type));
        }

        let is_read = reads.iter().any(|(id, _, _, _)| *id == res);

        if is_read {
            return Some(ResourceAccessType::Read(res_type));
        }

        None
    }

    pub(crate) fn is_resource_context_dependent(&self, id: ResourceId) -> bool {
        use crate::image;

        let (_id, info) = if let Some((id, info)) = self.create_info(id) {
            (id, info)
        } else {
            return false;
        };

        match info {
            ResourceCreateInfo::Image(img_info) => match &img_info {
                ImageInfo::BackbufferRead { .. } => false,
                ImageInfo::Create(create) => match create.size_mode {
                    image::ImageSizeMode::ContextRelative { .. } => true,
                    image::ImageSizeMode::Absolute { .. } => false,
                },
            },
            ResourceCreateInfo::Buffer(_buf) => false,
            ResourceCreateInfo::Virtual => false,
        }
    }

    pub(crate) fn pass_dependency(&self, id: PassId) -> PassDependency {
        let mut dependency = PassDependency {
            context: false,
            backbuffer: None,
        };

        match self.pass_types[&id] {
            PassType::Compute => {
                // can never depend on the context
                dependency
            }
            PassType::Graphics => {
                let writes = &self.pass_writes[&id];

                for (id, write_type, _) in writes {
                    let res_id = if let Some(id) = self.moved_from(*id) {
                        id
                    } else {
                        return dependency;
                    };

                    match write_type {
                        ResourceWriteType::Image(img) => match img {
                            ImageWriteType::Color | ImageWriteType::DepthStencil => {
                                let bb = self.is_backbuffer_resource(res_id);
                                let context = self.is_resource_context_dependent(res_id);

                                dependency.context |= context;

                                if let Some(name) = bb {
                                    if dependency.backbuffer.is_none() {
                                        dependency.backbuffer = Some(vec![]);
                                    }
                                    dependency.backbuffer = dependency.backbuffer.map(|mut val| {
                                        val.push(name);
                                        val
                                    });
                                }
                            }
                            ImageWriteType::Storage => {}
                        },
                        ResourceWriteType::Buffer(_) => {}
                    }
                }

                dependency
            }
        }
    }

    pub(crate) fn is_backbuffer_resource(&self, id: ResourceId) -> Option<ResourceName> {
        let info = if let Some((_, info)) = self.create_info(id) {
            info
        } else {
            return None;
        };

        match info {
            ResourceCreateInfo::Image(img_info) => match &img_info {
                ImageInfo::BackbufferRead { name, .. } => Some(name.clone()),
                ImageInfo::Create(_) => None,
            },
            ResourceCreateInfo::Buffer(_buf) => None,
            ResourceCreateInfo::Virtual => None,
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
pub(crate) fn resolve_input(
    input: GraphInput,
    errors: &mut Vec<CompileError>,
) -> GraphWithNamesResolved {
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
                errors.push(CompileError::ResourceRedefined {
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
                errors.push(CompileError::ResourceRedefined {
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
                errors.push(CompileError::ReferencedInvalidResource {
                    pass,
                    res: old_name.clone(),
                });
                continue;
            };
            let new_id = if let Some(id) = resource_name_lookup.get(&new_name) {
                *id
            } else {
                errors.push(CompileError::ReferencedInvalidResource {
                    pass,
                    res: new_name.clone(),
                });
                continue;
            };

            if let Some(prev_res) = resource_moves_to.get(&old_id) {
                if let Some(prev_pass) = resource_defines.get(prev_res) {
                    errors.push(CompileError::ResourceAlreadyMoved {
                        res: old_name.clone(),
                        attempted_new_name: new_name.clone(),
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
                errors.push(CompileError::ReferencedInvalidResource {
                    pass,
                    res: name.clone(),
                });
                continue;
            };

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
                errors.push(CompileError::ReferencedInvalidResource {
                    pass,
                    res: name.clone(),
                });
                continue;
            };

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
                errors.push(CompileError::ReferencedInvalidResource {
                    res: lname.clone(),
                    pass,
                });
                continue;
            }
        }
    }

    GraphWithNamesResolved {
        name_lookup: resource_name_lookup,

        pass_types: input.pass_types,

        defines: resource_defines,
        infos: resource_infos,

        moves_from: resource_moves_from,

        pass_creates,
        pass_ext_depends,
        pass_reads,
        pass_writes,
    }
}
