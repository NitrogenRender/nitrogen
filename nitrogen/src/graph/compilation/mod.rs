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

/// Error that can occur during graph compilation.
#[derive(Debug)]
pub enum GraphCompileError {
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
    /// A resource has been set as an output, but was never defined.
    InvalidOutputResource {
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
