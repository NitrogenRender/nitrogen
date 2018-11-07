use std::collections::{BTreeMap, HashMap, HashSet};

use super::builder::GraphBuilder;
use super::{ImageCreateInfo, ImageId, PassId};

use util::CowString;

#[derive(Debug)]
pub struct ConstructedGraph {
    pub(crate) num_nodes: usize,

    pub(crate) root_nodes: HashSet<PassId>,
    pub(crate) nodes_image_creates: HashMap<PassId, HashSet<CowString>>,
    pub(crate) nodes_image_copies: HashMap<PassId, HashSet<CowString>>,
    pub(crate) nodes_image_use: HashMap<PassId, HashSet<CowString>>,

    pub(crate) image_creates: BTreeMap<CowString, (PassId, ImageCreateInfo)>,
    pub(crate) image_copies: BTreeMap<CowString, (PassId, CowString)>,

    pub(crate) image_depend_reads: HashMap<CowString, HashSet<PassId>>,
    pub(crate) image_depend_writes: HashMap<CowString, HashSet<PassId>>,

    pub(crate) image_backbuffer: HashSet<CowString>,
}

impl ConstructedGraph {
    pub(crate) fn new() -> Self {
        ConstructedGraph {
            num_nodes: 0,

            root_nodes: HashSet::new(),
            nodes_image_creates: HashMap::new(),
            nodes_image_copies: HashMap::new(),
            nodes_image_use: HashMap::new(),

            image_creates: BTreeMap::new(),
            image_copies: BTreeMap::new(),
            image_depend_reads: HashMap::new(),
            image_depend_writes: HashMap::new(),

            image_backbuffer: HashSet::new(),
        }
    }

    pub(crate) fn add_pass(&mut self, pass_id: PassId, builder: GraphBuilder) {
        self.num_nodes += 1;

        if builder.images_read.len() == 0 && builder.images_write.len() == 0 {
            self.root_nodes.insert(pass_id);
        }

        self.image_backbuffer
            .extend(builder.backbuffer_images.into_iter());

        let image_creates = builder
            .images_create
            .iter()
            .map(|(name, _)| name.clone())
            .collect::<HashSet<_>>();

        self.nodes_image_creates.insert(pass_id, image_creates);

        let image_copies = builder
            .images_copy
            .iter()
            .map(|(new, src)| new.clone())
            .collect::<HashSet<_>>();

        self.nodes_image_copies.insert(pass_id, image_copies);

        for (name, info) in builder.images_create {
            self.image_creates.insert(name.clone(), (pass_id, info));

            self.nodes_image_use
                .entry(pass_id)
                .or_insert(HashSet::new())
                .insert(name);
        }

        for (new, src) in builder.images_copy {
            self.image_copies.insert(new.clone(), (pass_id, src));

            self.nodes_image_use
                .entry(pass_id)
                .or_insert(HashSet::new())
                .insert(new);
        }

        for name in builder.images_read {
            self.image_depend_reads
                .entry(name.clone())
                .or_insert(HashSet::new())
                .insert(pass_id);

            self.nodes_image_use
                .entry(pass_id)
                .or_insert(HashSet::new())
                .insert(name);
        }

        for name in builder.images_write {
            self.image_depend_writes
                .entry(name.clone())
                .or_insert(HashSet::new())
                .insert(pass_id);

            self.nodes_image_use
                .entry(pass_id)
                .or_insert(HashSet::new())
                .insert(name);
        }
    }

    // WARNING
    // THIS DOES NOT YET DO CYCLE DETECTION
    // YOU HAVE BEEN WARNED.
    pub(crate) fn execution_list(&self) -> Vec<Vec<PassId>> {
        let mut order = Vec::with_capacity(self.num_nodes);

        let mut nodes = HashSet::with_capacity(self.num_nodes);
        let mut nodes_tmp: HashSet<PassId> = HashSet::with_capacity(self.num_nodes);
        let mut images = HashSet::with_capacity(self.num_nodes);

        let mut node_last_position = HashMap::new();

        // start with root nodes
        for node in &self.root_nodes {
            nodes.insert(*node);
        }

        // flatten graph with duplicates
        while nodes.len() > 0 {
            for node in &nodes {
                node_last_position.insert(*node, order.len());
                order.push(*node);
            }

            for node in &nodes {
                let imgs = &self.nodes_image_creates[node];
                for img in imgs {
                    images.insert(img.clone());
                }

                let imgs = &self.nodes_image_copies[node];
                for img in imgs {
                    images.insert(img.clone());
                }
            }

            nodes.clear();

            for img in &images {
                if let Some(ns) = self.image_depend_reads.get(img) {
                    for n in ns {
                        nodes.insert(*n);
                    }
                }
                if let Some(ns) = self.image_depend_writes.get(img) {
                    for n in ns {
                        nodes.insert(*n);
                    }
                }
            }

            images.clear();
        }

        // deduplicate the list
        order
            .iter()
            .enumerate()
            .filter_map(|(idx, n)| {
                if node_last_position[n] > idx {
                    None
                } else {
                    Some(vec![*n])
                }
            }).collect()
    }
}
