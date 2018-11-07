use std::collections::{BTreeMap, HashMap, HashSet};

use super::constructed::ConstructedGraph;
use super::{ImageCreateInfo, ImageId, PassId};

use super::Graph;

use util::CowString;

#[derive(Clone, Debug)]
pub(crate) enum ImageCreateType {
    Copy(ImageId),
    Create(ImageCreateInfo),
}

#[derive(Clone, Debug)]
pub struct CompiledGraph {
    pub(crate) execution_list: Vec<Vec<PassId>>,

    pub(crate) images: Vec<(CowString, ImageCreateType)>,
    pub(crate) image_name_lookup: HashMap<CowString, ImageId>,

    pub(crate) image_creates: HashMap<PassId, HashSet<ImageId>>,
    pub(crate) image_copies: HashMap<PassId, HashSet<ImageId>>,
    pub(crate) image_destroys: HashMap<PassId, HashSet<ImageId>>,
    /// Which pass writes to which image, needed to create render pass attachements/framebuffers.
    pub(crate) image_writes: HashMap<PassId, HashSet<ImageId>>,

    pub(crate) image_backbuffers: HashSet<ImageId>,
}

impl CompiledGraph {
    pub fn new(graph: &Graph, cgraph: ConstructedGraph) -> Self {
        let exec_list = cgraph.execution_list();

        let mut image_creates = HashMap::new();
        let mut image_copies = HashMap::new();

        let mut lookup = HashMap::new();

        let images = {
            let mut images =
                Vec::with_capacity(cgraph.image_copies.len() + cgraph.image_creates.len());

            for (name, info) in cgraph.image_creates {
                let image_id = ImageId(images.len());
                images.push((name.clone(), ImageCreateType::Create(info.1)));

                lookup.insert(name, image_id);

                image_creates
                    .entry(info.0)
                    .or_insert(HashSet::new())
                    .insert(image_id);
            }

            for (name, info) in cgraph.image_copies {
                let image_id = ImageId(images.len());
                images.push((name.clone(), ImageCreateType::Copy(lookup[&info.1])));

                lookup.insert(name, image_id);

                image_copies
                    .entry(info.0)
                    .or_insert(HashSet::new())
                    .insert(image_id);
            }

            images
        };

        let image_backbuffers = cgraph
            .image_backbuffer
            .into_iter()
            .map(|name| lookup[&name])
            .collect::<HashSet<_>>();

        let mut image_destroys = HashMap::with_capacity(images.len());

        {
            let mut last_image_uses = HashMap::with_capacity(images.len());

            for passes in &exec_list {
                for pass in passes {
                    for img_name in &cgraph.nodes_image_use[pass] {
                        last_image_uses.insert(lookup[img_name], *pass);
                    }
                }
            }

            for (img, pass) in last_image_uses {
                if !image_backbuffers.contains(&img) {
                    image_destroys
                        .entry(pass)
                        .or_insert(HashSet::new())
                        .insert(img);
                }
            }
        }

        let mut image_writes = HashMap::new();

        cgraph
            .image_depend_writes
            .iter()
            .for_each(|(name, passes)| {
                let image_id = &lookup[name];
                for pass in passes {
                    image_writes
                        .entry(*pass)
                        .or_insert(HashSet::new())
                        .insert(*image_id);
                }
            });

        CompiledGraph {
            execution_list: exec_list,

            images,
            image_name_lookup: lookup,

            image_creates,
            image_copies,
            image_destroys,

            image_backbuffers,

            image_writes,
        }
    }

    pub fn image_info(&self, image_id: ImageId) -> Option<&ImageCreateInfo> {
        let mut id = image_id.0;

        while let Some(ref entry) = &self.images.get(id) {
            let data = &entry.1;

            match data {
                ImageCreateType::Create(info) => {
                    return Some(info);
                }
                ImageCreateType::Copy(next_id) => {
                    id = next_id.0;
                }
            }
        }

        None
    }
}
