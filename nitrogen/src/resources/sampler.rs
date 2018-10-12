use gfx;
use back;

use slab::Slab;

use gfx::image;
use gfx::Device;

use super::DeviceContext;


type Sampler = <back::Backend as gfx::Backend>::Sampler;

pub enum Filter {
    Nearest,
    Linear,
}

impl From<Filter> for image::Filter {
    fn from(filter: Filter) -> Self {
        match filter {
            Filter::Nearest => image::Filter::Nearest,
            Filter::Linear => image::Filter::Linear,
        }
    }
}

pub enum WrapMode {
    Tile,
    Mirror,
    Clamp,
    Border,
}

impl From<WrapMode> for image::WrapMode {
    fn from(mode: WrapMode) -> Self {
        match mode {
            WrapMode::Tile => image::WrapMode::Tile,
            WrapMode::Mirror => image::WrapMode::Mirror,
            WrapMode::Clamp => image::WrapMode::Clamp,
            WrapMode::Border => image::WrapMode::Border,
        }
    }
}


pub struct SamplerCreateInfo {
    pub min_filter: Filter,
    pub mag_filter: Filter,
    pub mip_filter: Filter,
    pub wrap_mode: (WrapMode, WrapMode, WrapMode),
}

impl From<SamplerCreateInfo> for image::SamplerInfo {
    fn from(create: SamplerCreateInfo) -> Self {
        image::SamplerInfo {
            min_filter: create.min_filter.into(),
            mag_filter: create.mag_filter.into(),
            mip_filter: create.mip_filter.into(),
            wrap_mode: (
                create.wrap_mode.0.into(),
                create.wrap_mode.1.into(),
                create.wrap_mode.2.into(),
            ),
            lod_bias: 0.0.into(),
            lod_range: (0.0.into())..(1.0.into()),
            comparison: None,
            border: image::PackedColor(0x0),
            anisotropic: image::Anisotropic::Off,
        }
    }
}



pub type SamplerGeneration = u64;
pub type SamplerId = usize;

#[derive(Copy, Clone)]
pub struct SamplerHandle(pub SamplerId, pub SamplerGeneration);

pub struct SamplerStorage {
    generations: Vec<SamplerGeneration>,
    samplers: Slab<Sampler>,
}

impl SamplerStorage {
    pub fn new() -> Self {
        Self {
            generations: vec![],
            samplers: Slab::new(),
        }
    }

    pub fn create(
        &mut self,
        device: &DeviceContext,
        create_info: SamplerCreateInfo,
    ) -> SamplerHandle {

        let (entry, handle) = {

            let entry = self.samplers.vacant_entry();
            let key = entry.key();

            let needs_to_grow_storage = self.generations.len() <= key;

            if needs_to_grow_storage {
                self.generations.push(0);
            } else {
                self.generations[key] += 1;
            }

            let generation = self.generations[key];

            (entry, SamplerHandle(key, generation))
        };

        let sampler = {
            device.device.create_sampler(create_info.into())
        };

        entry.insert(sampler);

        println!("Created sampler {}", handle.0);

        handle
    }

    pub fn is_alive(&self, sampler: SamplerHandle) -> bool {
        let fits_inside_storage = self.generations.len() > sampler.0;

        if fits_inside_storage {
            let is_generation_same = self.generations[sampler.0] == sampler.1;
            is_generation_same
        } else {
            false
        }
    }

    pub fn destroy(&mut self, device: &DeviceContext, handle: SamplerHandle) -> bool {
        if self.is_alive(handle) {
            let sampler = self.samplers.remove(handle.0);
            device.device.destroy_sampler(sampler);

            println!("Destroyed sampler {}", handle.0);

            true
        } else {
            false
        }
    }
}
