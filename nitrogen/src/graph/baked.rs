use std::collections::{HashMap, HashSet};

use super::SetUpGraph;

use super::builder::{ResourceCreateInfo, ResourceReadType, ResourceWriteType};
use super::{PassId, ResourceName};

#[derive(Debug)]
pub enum BakedGraphErrorContent {
    ResourceUndefined { resource: ResourceName },
}

use self::BakedGraphErrorContent::*;

#[derive(Debug)]
pub struct BakedGraphError {
    pub id: Option<PassId>,
    pub content: BakedGraphErrorContent,
}

impl BakedGraphError {
    pub fn new(id: PassId, content: BakedGraphErrorContent) -> Self {
        BakedGraphError {
            id: Some(id),
            content,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub enum ResourceDefinitionType {
    Create,
    Copy,
    Move,
}

#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct ResourceId(pub(crate) usize);

#[derive(Debug)]
pub(crate) struct BakedGraph {
    pub resources: Vec<(ResourceName, ResourceDefinitionType)>,
    /// Create information about the resources
    pub resource_creates: HashMap<ResourceId, ResourceCreateInfo>,
    /// Copied resource -> Original resource
    pub resource_copies_from: HashMap<ResourceId, ResourceId>,
    /// Moved resource -> Original resource
    pub resource_moves_from: HashMap<ResourceId, ResourceId>,

    pub exec_list: Vec<Vec<PassId>>,
}

impl BakedGraph {
    pub fn create(s: SetUpGraph, outputs: &[ResourceName]) -> Result<Self, Vec<BakedGraphError>> {
        let mut errors = vec![];

        if !errors.is_empty() {
            return Err(errors);
        }

        unimplemented!()
    }
}
