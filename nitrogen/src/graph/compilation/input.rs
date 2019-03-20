/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use std::collections::BTreeMap;

use super::*;
use crate::graph::ResourceDescriptor;
use crate::graph::{ResourceCreateInfo, ResourceReadType, ResourceWriteType};
use crate::graph::builder::PassType;

// the Option<u8> represents a possible sampler binding
pub(crate) type ResourceRead = (ResourceName, ResourceReadType, u8, Option<u8>);

#[derive(Debug, Hash, Default)]
pub(crate) struct GraphInput {

    pub(crate) pass_types: BTreeMap<PassId, PassType>,

    pub(crate) resource_creates: BTreeMap<PassId, Vec<(ResourceName, ResourceCreateInfo)>>,
    pub(crate) resource_moves: BTreeMap<PassId, Vec<(ResourceName, ResourceName)>>,

    pub(crate) resource_reads: BTreeMap<PassId, Vec<ResourceRead>>,
    pub(crate) resource_writes: BTreeMap<PassId, Vec<(ResourceName, ResourceWriteType, u8)>>,

    // (backbuffer name, local name)
    pub(crate) resource_backbuffer: BTreeMap<PassId, Vec<(ResourceName, ResourceName)>>,
}

impl GraphInput {
    pub(crate) fn add_res_descriptor(&mut self, id: PassId, res: ResourceDescriptor, pass_type: PassType) {
        self.pass_types.insert(id, pass_type);

        self.resource_creates.insert(id, res.resource_creates);
        self.resource_moves.insert(id, res.resource_moves);

        self.resource_reads.insert(id, res.resource_reads);
        self.resource_writes.insert(id, res.resource_writes);

        self.resource_backbuffer
            .insert(id, res.resource_backbuffer);
    }
}
