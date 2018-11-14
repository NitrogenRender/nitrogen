use std::collections::{BTreeMap, HashMap, HashSet};

use super::constructed::{ConstructedGraph, ExecutionList, ExecutionListBuildError};
use super::{ImageCreateInfo, ImageId, PassId};

use super::Graph;

use util::CowString;

#[derive(Clone, Debug)]
pub(crate) enum ImageCreateType {
    Copy(ImageId),
    Create(ImageCreateInfo),
    Move(ImageId),
}

#[derive(Clone, Debug)]
pub struct CompiledGraph {
    /// List of passes to execute
    pub(crate) execution_list: ExecutionList,

    /// Information about images, such as names and creation info
    pub(crate) images: Vec<(CowString, ImageCreateType)>,
    /// Mapping from names to IDs (which point into `images`)
    pub(crate) image_name_lookup: HashMap<CowString, ImageId>,

    /// Images created in pass
    pub(crate) image_creates: HashMap<PassId, HashSet<ImageId>>,
    /// Images copied in pass
    pub(crate) image_copies: HashMap<PassId, HashSet<ImageId>>,
    /// Images destroyed in pass (does not include moves)
    pub(crate) image_destroys: HashMap<PassId, HashSet<ImageId>>,
    /// Images moved in pass
    pub(crate) image_moves: HashMap<PassId, HashSet<ImageId>>,

    /// Write-binding of image
    pub(crate) image_write_bindings: HashMap<PassId, HashMap<ImageId, u8>>,

    /// Read-bindings of image
    pub(crate) image_read_bindings: HashMap<PassId, HashMap<ImageId, HashSet<u8>>>,

    /// Images in the backbuffer
    pub(crate) image_backbuffers: HashSet<ImageId>,
}

impl CompiledGraph {

    pub fn create(graph: &Graph, cgraph: ConstructedGraph, output: &CowString) -> Result<Self, ()> {
        // Create the execution list from which we can compute lifetimes.
        let exec_list = cgraph.execution_list(output).map_err(|_| ())?;

        let mut image_creates = HashMap::new();
        let mut image_copies: HashMap<PassId, _> = HashMap::new();
        let mut image_moves = HashMap::new();
        let mut image_destroys = HashMap::new();

        // Default initialize the maps
        {
            for batch in &exec_list.passes {
                for pass in batch {
                    image_creates.insert(*pass, HashSet::new());
                    image_copies.insert(*pass, HashSet::new());
                    image_moves.insert(*pass, HashSet::new());
                    image_destroys.insert(*pass, HashSet::new());
                }
            }
        }


        let mut lookup = HashMap::new();

        // Go through creates, copies and moves to map names to IDs
        let images = {

            let mut images = Vec::with_capacity(
                cgraph.image_copies.len() + cgraph.image_creates.len() + cgraph.image_moves.len(),
            );

            for batch in &exec_list.passes {
                for pass in batch {

                    if let Some(set) = cgraph.nodes_image_direct_create.get(pass) {
                        for img in set {

                            if lookup.contains_key(img) {
                                continue;
                            }

                            let info = &cgraph.image_creates[img].1;

                            let image_id = ImageId(images.len());
                            images.push((img.clone(), ImageCreateType::Create(info.clone())));
                            lookup.insert(img.clone(), image_id);
                            image_creates.get_mut(pass).map(|set| set.insert(image_id));
                        }
                    }

                    if let Some(set) = cgraph.nodes_image_copies.get(pass) {
                        for img in &cgraph.nodes_image_copies[pass] {

                            for (pass_that_copies, copy) in &cgraph.image_copies[img] {
                                if pass != pass_that_copies {
                                    continue;
                                }

                                if lookup.contains_key(copy) {
                                    continue;
                                }

                                let src_id = lookup[img];

                                let image_id = ImageId(images.len());
                                images.push((copy.clone(), ImageCreateType::Copy(src_id)));
                                lookup.insert(copy.clone(), image_id);
                                image_copies.get_mut(pass).map(|set| set.insert(image_id));
                            }
                        }
                    }

                    if let Some(set) = cgraph.nodes_image_move.get(pass) {
                        for img in &cgraph.nodes_image_move[pass] {
                            let (pass_that_moves, new) = &cgraph.image_moves[img];

                                if pass != pass_that_moves {
                                    continue;
                                }

                                if lookup.contains_key(new) {
                                    continue;
                                }

                                let src_id = lookup[img];

                                let image_id = ImageId(images.len());
                                images.push((new.clone(), ImageCreateType::Move(src_id)));
                                lookup.insert(new.clone(), image_id);
                                image_moves.get_mut(pass).map(|set| set.insert(image_id));
                        }
                    }
                }


            }

            images
        };

        // Resolve image backbuffer names
        let image_backbuffers = {
            let backbuffer = cgraph
                .image_backbuffer
                .into_iter()
                .filter_map(|name| lookup.get(&name).map(|i| *i))
                .collect::<HashSet<_>>();

            let mut final_backbuffer = HashSet::new();

            for img in backbuffer {
                final_backbuffer.insert(img);

                final_backbuffer.insert(resolve_image_id(&images, img).unwrap());
            }

            final_backbuffer
        };


        // Find the last time an image is used. That information can be used to
        // create the list of images to be deleted after the last pass that used it.
        {
            let mut last_image_uses = HashMap::with_capacity(images.len());

            for passes in &exec_list.passes {
                for pass in passes {

                    if let Some(set) = cgraph.nodes_image_direct_create.get(pass) {
                        for img_name in set {
                            last_image_uses.insert(lookup[img_name], *pass);
                        }
                    }

                    if let Some(set) = cgraph.nodes_image_read.get(pass) {
                        for (img_name, _bindings) in set {
                            last_image_uses.insert(lookup[img_name], *pass);
                        }
                    }

                    if let Some(set) = cgraph.nodes_image_write.get(pass) {
                        for (img_name, _binding) in set {
                            last_image_uses.insert(lookup[img_name], *pass);
                        }
                    }

                    if let Some(set) = cgraph.nodes_image_copies.get(pass) {
                        for img_name in set {
                            last_image_uses.insert(lookup[img_name], *pass);
                        }
                    }

                    if let Some(set) = cgraph.nodes_image_move.get(pass) {
                        for img_name in set {
                            last_image_uses.insert(lookup[img_name], *pass);
                        }
                    }
                }
            }

            for (img, pass) in last_image_uses {
                // don't destroy images that will be moved or are in the backbuffer
                // let is_in_backbuffer = real_backbuffers.clone()
                //     .find(|id| *id == img).is_some();
                let is_in_backbuffer = image_backbuffers.contains(&img);

                if is_in_backbuffer {
                    continue;
                }

                let image_gets_moved = cgraph.image_moves.contains_key(&images[img.0].0);

                if image_gets_moved {
                    continue;
                }

                println!("destroy image {} in pass {}", img.0, pass.0);
                image_destroys
                    .entry(pass)
                    .or_insert(HashSet::new())
                    .insert(img);
            }
        }

        Ok(CompiledGraph {
            execution_list: exec_list,

            images,
            image_name_lookup: lookup,

            image_creates,
            image_copies,
            image_destroys,

            image_backbuffers,

            image_moves,
        })
    }

    /// Find out which images `pass` is writing to
    pub fn image_writes<'a>(&'a self, pass: PassId) -> impl Iterator<Item = ImageId> + Clone + 'a {

        let creates = self.image_creates[&pass].iter().cloned();
        let copies = self.image_copies[&pass].iter().cloned();
        let moves = self.image_moves[&pass].iter().cloned();

        creates.chain(copies).chain(moves)
    }

    pub fn resolve_image_id(&self, image_id: ImageId) -> Option<ImageId> {
        // NOT recursion. Just not a member function
        resolve_image_id(&self.images, image_id)
    }

    pub fn image_info(&self, image_id: ImageId) -> Option<&ImageCreateInfo> {
        self.resolve_image_id(image_id).map(|id| {
            match &self.images[id.0].1 {
                ImageCreateType::Create(info) => info,
                _ => unreachable!(),
            }
        })
    }
}

fn resolve_image_id(images: &Vec<(CowString, ImageCreateType)>, image: ImageId) -> Option<ImageId> {
    let mut id = image.0;

    while let Some(ref entry) = &images.get(id) {
        let data = &entry.1;

        match data {
            ImageCreateType::Create(_) => {
                return Some(ImageId(id));
            }
            ImageCreateType::Copy(next_id) => {
                id = next_id.0;
            }
            ImageCreateType::Move(next_id) => {
                id = next_id.0;
            }
        }
    }
    None
}