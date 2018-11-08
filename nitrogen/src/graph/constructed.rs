use std::collections::{BTreeMap, HashMap, HashSet};

use super::builder::GraphBuilder;
use super::{ImageCreateInfo, ImageId, PassId};

use util::CowString;

#[derive(Debug, Clone)]
pub enum GraphErrorContent {
    ImageRedefined {
        img: CowString,
        defining_pass: PassId,
    },
    ImageMovedTwice {
        src: CowString,
        new_name: CowString,
    },
}

use self::GraphErrorContent::*;

#[derive(Debug, Clone)]
pub struct GraphError {
    pub pass: PassId,
    pub error: GraphErrorContent,
}

impl GraphError {
    fn new(pass: PassId, error: GraphErrorContent) -> Self {
        GraphError { pass, error }
    }
}

#[derive(Debug, Clone)]
pub struct ExecutionList {
    pub passes: Vec<Vec<PassId>>,
}

#[derive(Debug, Clone)]
pub enum ExecutionListBuildError {
    OutputImageInvalid,
}

#[derive(Debug)]
pub struct ConstructedGraph {
    /// Number of nodes in the graph
    pub(crate) num_nodes: usize,

    /// Number of images created (does **NOT** include moved images)
    pub(crate) num_images_created: usize,

    /// Number of named images (includes all created images as well as moves)
    pub(crate) num_image_names: usize,

    // node -> image
    /// Images created by pass - includes moves and copies
    pub(crate) nodes_image_creates: HashMap<PassId, HashSet<CowString>>,

    /// Images copied by pass. The newly created image name is part of `nodes_image_creates`
    pub(crate) nodes_image_copies: HashMap<PassId, HashSet<CowString>>,

    /// Images moved by pass. The new image name is part of `nodes_image_creates`
    pub(crate) nodes_image_move: HashMap<PassId, HashSet<CowString>>,

    /// Images read by pass
    pub(crate) nodes_image_read: HashMap<PassId, HashSet<CowString>>,

    // image -> pass
    /// Pass which creates the image
    pub(crate) image_creates: HashMap<CowString, (PassId, ImageCreateInfo)>,

    /// Passes which copy the image
    pub(crate) image_copies: HashMap<CowString, HashSet<(PassId, CowString)>>,

    /// Pass which moves the image
    pub(crate) image_moves: HashMap<CowString, (PassId, CowString)>,

    /// Passes which read the image
    pub(crate) image_reads: HashMap<CowString, HashSet<PassId>>,

    /// Pass in which the image has been defined (either created, copied or moved)
    pub(crate) image_defines: HashMap<CowString, PassId>,

    // backbuffer
    /// Images in backbuffer
    pub(crate) image_backbuffer: HashSet<CowString>,
}

impl ConstructedGraph {
    pub(crate) fn new() -> Self {
        ConstructedGraph {
            num_nodes: 0,
            num_images_created: 0,
            num_image_names: 0,

            nodes_image_creates: HashMap::new(),
            nodes_image_copies: HashMap::new(),
            nodes_image_move: HashMap::new(),
            nodes_image_read: HashMap::new(),

            image_creates: HashMap::new(),
            image_copies: HashMap::new(),
            image_moves: HashMap::new(),
            image_reads: HashMap::new(),

            image_defines: HashMap::new(),

            image_backbuffer: HashSet::new(),
        }
    }

    pub(crate) fn add_pass(
        &mut self,
        pass_id: PassId,
        builder: GraphBuilder,
        errors: &mut Vec<GraphError>,
    ) {
        self.num_nodes += 1;

        self.image_backbuffer
            .extend(builder.backbuffer_images.into_iter());

        for (name, info) in builder.images_create {
            if self.image_defines.contains_key(&name) {
                errors.push(GraphError::new(
                    pass_id,
                    ImageRedefined {
                        img: name.clone(),
                        defining_pass: self.image_defines[&name],
                    },
                ));
                continue;
            }

            self.image_creates.insert(name.clone(), (pass_id, info));

            self.image_defines.insert(name.clone(), pass_id);

            // We don't need to check for redefined images here since that would've been
            // already catched in the branch above.
            self.nodes_image_creates
                .entry(pass_id)
                .or_insert(HashSet::new())
                .insert(name.clone());

            self.num_images_created += 1;
            self.num_image_names += 1;
        }

        for (src, new) in builder.images_move {
            if self.image_moves.contains_key(&src) {
                errors.push(GraphError::new(
                    pass_id,
                    ImageMovedTwice {
                        src: src.clone(),
                        new_name: new.clone(),
                    },
                ));
                continue;
            }

            if self.image_defines.contains_key(&new) {
                errors.push(GraphError::new(
                    pass_id,
                    ImageRedefined {
                        img: new.clone(),
                        defining_pass: self.image_defines[&new],
                    },
                ));
                continue;
            }

            self.image_moves.insert(src.clone(), (pass_id, new.clone()));

            self.image_defines.insert(new.clone(), pass_id);

            self.nodes_image_move
                .entry(pass_id)
                .or_insert(HashSet::new())
                .insert(src.clone());

            self.nodes_image_creates
                .entry(pass_id)
                .or_insert(HashSet::new())
                .insert(new.clone());

            self.num_image_names += 1;
        }

        for (src, new) in builder.images_copy {
            if self.image_defines.contains_key(&new) {
                errors.push(GraphError::new(
                    pass_id,
                    ImageRedefined {
                        img: new.clone(),
                        defining_pass: self.image_defines[&new],
                    },
                ));
                continue;
            }

            self.image_copies
                .entry(src.clone())
                .or_insert(HashSet::new())
                .insert((pass_id, new.clone()));

            self.nodes_image_copies
                .entry(pass_id)
                .or_insert(HashSet::new())
                .insert(new.clone());

            self.nodes_image_creates
                .entry(pass_id)
                .or_insert(HashSet::new())
                .insert(new.clone());

            self.num_images_created += 1;
            self.num_image_names += 1;
        }

        for img in builder.images_read {
            self.image_reads
                .entry(img.clone())
                .or_insert(HashSet::new())
                .insert(pass_id);

            self.nodes_image_read
                .entry(pass_id)
                .or_insert(HashSet::new())
                .insert(img.clone());
        }
    }

    /// Create an execution list that will lead up to the pass that creates the
    /// `output_image`.
    pub(crate) fn execution_list(
        &self,
        output_image: &CowString,
    ) -> Result<ExecutionList, ExecutionListBuildError> {
        use self::ExecutionListBuildError::*;

        // In order to create the list we walk the dependency *backwards*.
        // If we know that we will only output image P then it's enough
        // to execute the pass that creates P.
        // Since that pass might have dependencies this procedure can
        // be repeated until we reached root nodes.

        let mut list: Vec<HashSet<PassId>> = Vec::new();

        let last_pass = *self
            .image_defines
            .get(output_image)
            .ok_or(OutputImageInvalid)?;

        // The nodes that the needed image resources are created in.
        let mut required_nodes = HashSet::new();

        required_nodes.insert(last_pass);

        while required_nodes.len() > 0 {
            let batch = required_nodes.clone();
            required_nodes.clear();

            for node in &batch {

                // find the new dependencies of each node.
                let image_deps = {
                    let mut deps = HashSet::new();

                    if let Some(set) = self.nodes_image_read.get(&node) {
                        deps.extend(set);
                    }
                    if let Some(set) = self.nodes_image_move.get(&node) {
                        deps.extend(set);
                    }
                    if let Some(set) = self.nodes_image_copies.get(&node) {
                        deps.extend(set);
                    }

                    deps
                };

                for image in image_deps {
                    required_nodes.insert(self.image_defines[image]);
                }
            }

            list.push(batch.into_iter().collect());
        }

        list.reverse();

        // deduplicate the sub-lists
        let list = {
            // When building the list, every dependency of a node will in the next batch,
            // but some other node in the same (or later) batch can also depend on the same
            // dependency.
            //
            // The list could look like this:
            // [[0, 1], [2, 0], [3]]
            //   => "3 depends on 0 and 2, but 2 depends on 1 and 0"
            //
            // So in this example you can see that the 0 in the middle doesn't need to b there.
            // In fact, every node that was enountered once does not need to be in the list at a
            // later point.
            //
            // Here we use a HashSet to keep track of all previously encountered nodes and then
            // remove all duplicates.
            let mut known_nodes = HashSet::new();

            list.into_iter()
                .map(|batch| {
                    let deduped = batch
                        .into_iter()
                        .filter(|x| !known_nodes.contains(x))
                        .collect::<Vec<PassId>>();

                    for pass in &deduped {
                        known_nodes.insert(*pass);
                    }

                    deduped
                }).collect()
        };

        Ok(ExecutionList { passes: list })
    }
}
