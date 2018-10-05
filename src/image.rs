use gfx;
use back;

use gfx::Device;
use gfx::image;

use std::sync::Arc;

use slab::Slab;

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub enum ImageDimension {
    D1 { x: u32 },
    D2 { x: u32, y: u32 },
    D3 { x: u32, y: u32, z: u32 },
}

type ImageGeneration = u64;
type ImageId = usize;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct ImageHandle(ImageId, ImageGeneration);

#[repr(C)]
pub struct ImageCreateInfo {
    pub dimension: ImageDimension,
    pub num_layers: u16,
    pub num_samples: u8,
    pub num_mipmaps: u8,
}

pub struct ImageStorage {
    dimensions: Vec<ImageDimension>,
    generations: Vec<ImageGeneration>,

    images: Slab<()>,

    device: Arc<back::Device>,
}


impl ImageStorage {
    pub fn new(device: Arc<back::Device>) -> Self {
        ImageStorage {
            dimensions: vec![],
            generations: vec![],
            images: Slab::new(),
            device,
        }
    }


    pub fn create(&mut self, create_info: ImageCreateInfo) -> ImageHandle {

        let handle = {

            let entry = self.images.vacant_entry();
            let key = entry.key();

            entry.insert(());

            let needs_to_grow_storage = self.generations.len() <= key;

            if needs_to_grow_storage {
                self.generations.push(0);
                self.dimensions.push(create_info.dimension);

                debug_assert!(self.generations.len() == self.dimensions.len());
            } else {
                self.generations[key] += 1;
                self.dimensions[key] = create_info.dimension;
            }

            let generation = self.generations[key];

            ImageHandle(key, generation)
        };

        let texture = {

            let image_kind = match create_info.dimension {
                ImageDimension::D1 { x } => image::Kind::D1(x, create_info.num_layers),
                ImageDimension::D2 { x, y } => image::Kind::D2( x, y, create_info.num_layers, create_info.num_samples),
                ImageDimension::D3 { x, y, z } => image::Kind::D3(x, y, z)
            };

            let format = gfx::format::Format::Rgba8Srgb;

            let unbound_image = self
                .device
                .create_image(
                    image_kind,
                    create_info.num_mipmaps,
                    format,
                    image::Tiling::Optimal,
                    image::Usage::TRANSFER_DST | image::Usage::SAMPLED,
                    image::ViewCapabilities::empty()
                ).unwrap();

            let requirements = self.device.get_image_requirements(&unbound_image);

            // unimplemented!()
        };

        handle

    }

    pub fn is_alive(&self, image: ImageHandle) -> bool {
        let fits_inside_storage = self.generations.len() > image.0;

        if fits_inside_storage {
            let is_generation_same = self.generations[image.0] == image.1;
            is_generation_same
        } else {
            false
        }
    }

    pub fn destroy(&mut self, image: ImageHandle) {
        if self.is_alive(image) {
            self.images.remove(image.0);
        }
    }
}