/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use std::collections::BTreeMap;

use super::*;
use crate::graph::GraphBuilder;
use crate::graph::{ResourceCreateInfo, ResourceReadType, ResourceWriteType};

// the Option<u8> represents a possible sampler binding
pub(crate) type ResourceRead = (ResourceName, ResourceReadType, u8, Option<u8>);

#[derive(Debug, Hash, Default)]
pub(crate) struct GraphInput {
    pub(crate) resource_creates: BTreeMap<PassId, Vec<(ResourceName, ResourceCreateInfo)>>,
    pub(crate) resource_copies: BTreeMap<PassId, Vec<(ResourceName, ResourceName)>>,
    pub(crate) resource_moves: BTreeMap<PassId, Vec<(ResourceName, ResourceName)>>,

    pub(crate) resource_reads: BTreeMap<PassId, Vec<ResourceRead>>,
    pub(crate) resource_writes: BTreeMap<PassId, Vec<(ResourceName, ResourceWriteType, u8)>>,

    // (backbuffer name, local name)
    pub(crate) resource_backbuffer: BTreeMap<PassId, Vec<(ResourceName, ResourceName)>>,
}

impl GraphInput {
    pub(crate) fn add_builder(&mut self, id: PassId, builder: GraphBuilder) {
        self.resource_creates.insert(id, builder.resource_creates);
        self.resource_copies.insert(id, builder.resource_copies);
        self.resource_moves.insert(id, builder.resource_moves);

        self.resource_reads.insert(id, builder.resource_reads);
        self.resource_writes.insert(id, builder.resource_writes);

        self.resource_backbuffer
            .insert(id, builder.resource_backbuffer);
    }
}
