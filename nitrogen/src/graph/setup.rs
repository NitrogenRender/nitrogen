use std::collections::{BTreeMap, BTreeSet};

use super::builder::{ResourceCreateInfo, ResourceReadType, ResourceWriteType};
use super::Graph;
use super::PassId;
use super::ResourceId;
use super::ResourceName;

#[derive(Debug)]
pub enum GraphSetUpErrorContent {
    ResourceRedefined {
        resource: ResourceName,
        prev_define: PassId,
    },
}

use self::GraphSetUpErrorContent::*;

#[derive(Debug)]
pub struct GraphSetUpError {
    pub id: PassId,
    pub content: GraphSetUpErrorContent,
}

impl GraphSetUpError {
    pub fn new(pass_id: PassId, content: GraphSetUpErrorContent) -> Self {
        GraphSetUpError {
            id: pass_id,
            content,
        }
    }
}

/// Structure that contains the textual representation of the passes and their dependencies
#[derive(Debug, Hash)]
pub(crate) struct SetUpGraph {
    pub resource_definitions: BTreeMap<ResourceName, PassId>,
    pub resource_backbuffer: BTreeSet<ResourceName>,

    pub node_resource_creates: BTreeMap<PassId, BTreeMap<ResourceName, ResourceCreateInfo>>,

    // new name -> old name
    pub node_resource_copies: BTreeMap<PassId, BTreeMap<ResourceName, ResourceName>>,

    // new name -> old name
    pub node_resource_moves: BTreeMap<PassId, BTreeMap<ResourceName, ResourceName>>,

    pub node_resource_writes: BTreeMap<PassId, BTreeMap<ResourceName, (ResourceWriteType, u8)>>,
    pub node_resource_reads: BTreeMap<PassId, BTreeMap<ResourceName, (ResourceReadType, u8)>>,

    // Resources used: contains copies, moves, reads and writes
    pub node_resource_use: BTreeMap<PassId, BTreeSet<ResourceName>>,

    pub node_resource_depends: BTreeMap<PassId, BTreeSet<ResourceName>>,
}

impl SetUpGraph {
    pub fn create(graph: &mut Graph) -> Result<Self, Vec<GraphSetUpError>> {
        let mut errors = vec![];

        let mut resource_definitions = BTreeMap::new();
        let mut resource_backbuffer = BTreeSet::new();
        let mut node_resource_creates = BTreeMap::new();
        let mut node_resource_copies = BTreeMap::new();
        let mut node_resource_moves = BTreeMap::new();
        let mut node_resource_writes = BTreeMap::new();
        let mut node_resource_reads = BTreeMap::new();
        let mut node_resource_use = BTreeMap::new();
        let mut node_resource_depends = BTreeMap::new();

        // let mut created_resources = HashMap::new();

        for (i, pass_impl) in graph.passes_impl.iter_mut().enumerate() {
            let pass_id = PassId(i);

            let mut builder = super::GraphBuilder::new();
            pass_impl.setup(&mut builder);

            if !builder.enabled {
                continue;
            }

            let uses = node_resource_use.entry(pass_id).or_insert(BTreeSet::new());

            let depends = node_resource_depends
                .entry(pass_id)
                .or_insert(BTreeSet::new());

            {
                let creates = node_resource_creates
                    .entry(pass_id)
                    .or_insert(BTreeMap::new());

                for (name, info) in builder.resource_creates {
                    if resource_definitions.contains_key(&name) {
                        errors.push(GraphSetUpError::new(
                            pass_id,
                            ResourceRedefined {
                                resource: name.clone(),
                                prev_define: resource_definitions[&name],
                            },
                        ));
                        continue;
                    }

                    creates.insert(name.clone(), info);
                    resource_definitions.insert(name, pass_id);
                }
            }

            {
                let copies = node_resource_copies
                    .entry(pass_id)
                    .or_insert(BTreeMap::new());

                for (new_name, old_name) in builder.resource_copies {
                    if resource_definitions.contains_key(&new_name) {
                        errors.push(GraphSetUpError::new(
                            pass_id,
                            ResourceRedefined {
                                resource: new_name.clone(),
                                prev_define: resource_definitions[&new_name],
                            },
                        ));
                        continue;
                    }

                    uses.insert(old_name.clone());
                    depends.insert(old_name.clone());
                    copies.insert(new_name.clone(), old_name);
                    resource_definitions.insert(new_name, pass_id);
                }
            }

            {
                let moves = node_resource_moves
                    .entry(pass_id)
                    .or_insert(BTreeMap::new());

                for (new_name, old_name) in builder.resource_moves {
                    if resource_definitions.contains_key(&new_name) {
                        errors.push(GraphSetUpError::new(
                            pass_id,
                            ResourceRedefined {
                                resource: new_name.clone(),
                                prev_define: resource_definitions[&new_name],
                            },
                        ));
                        continue;
                    }

                    uses.insert(old_name.clone());
                    depends.insert(old_name.clone());
                    moves.insert(new_name.clone(), old_name);
                    resource_definitions.insert(new_name, pass_id);
                }
            }

            {
                let write = node_resource_writes
                    .entry(pass_id)
                    .or_insert(BTreeMap::new());

                for (name, ty, binding) in builder.resource_writes {
                    if !resource_definitions
                        .get(&name)
                        .map(|pass| *pass == pass_id)
                        .unwrap_or(false)
                    {
                        depends.insert(name.clone());
                    }

                    uses.insert(name.clone());
                    write.insert(name, (ty, binding));
                }
            }

            {
                let read = node_resource_reads
                    .entry(pass_id)
                    .or_insert(BTreeMap::new());

                for (name, ty, binding) in builder.resource_reads {
                    if !resource_definitions
                        .get(&name)
                        .map(|pass| *pass == pass_id)
                        .unwrap_or(false)
                    {
                        depends.insert(name.clone());
                    }

                    uses.insert(name.clone());
                    read.insert(name, (ty, binding));
                }
            }

            {
                for name in builder.resource_backbuffer {
                    resource_backbuffer.insert(name);
                }
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        Ok(SetUpGraph {
            resource_definitions,
            resource_backbuffer,

            node_resource_creates,
            node_resource_copies,
            node_resource_moves,
            node_resource_writes,
            node_resource_reads,
            node_resource_use,
            node_resource_depends,
        })
    }
}
