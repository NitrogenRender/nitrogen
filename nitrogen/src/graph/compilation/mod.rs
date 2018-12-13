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

#[derive(Copy, Clone, Debug, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub struct ResourceId(pub(crate) usize);

#[derive(Debug)]
pub enum GraphCompileError {
    InvalidGraph,
    ResourceRedefined {
        res: ResourceName,
        prev: PassId,
        pass: PassId,
    },
    ReferencedInvalidResource {
        res: ResourceName,
        pass: PassId,
    },
    InvalidOutputResource {
        res: ResourceName,
    },
    ResourceTypeMismatch {
        res: ResourceName,
        pass: PassId,
        used_as: ResourceType,
        expected: ResourceType,
    },
    ResourceAlreadyMoved {
        res: ResourceName,
        pass: PassId,
        prev_move: PassId,
    },
}
