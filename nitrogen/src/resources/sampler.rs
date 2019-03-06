/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Description of sampler objects.

use gfx::image;
use gfx::Device;

use crate::device::DeviceContext;

use crate::util::storage;
use crate::util::storage::Storage;

use crate::submit_group::ResourceList;

/// Samplers are used to determine how texture lookups should be performed.
///
/// The most common use for samplers is to enable "linear filtering" to "unpixelate" images, or
/// "nearest filtering" to have a pixelated look. There are many more options for samplers apart
/// from filter modes.
pub struct Sampler(pub crate::types::Sampler);

use std::borrow::Borrow;

/// Filter mode used when sampling.
#[repr(u8)]
#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub enum Filter {
    /// Use color of the nearest texel.
    Nearest,
    /// Use linear interpolation between the nearest texels.
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

/// Wrap mode used when sampling outside of `[0..1]` range occurs
#[allow(missing_docs)]
#[repr(u8)]
#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash)]
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

/// Description of a sampler object
#[derive(Copy, Clone)]
pub struct SamplerCreateInfo {
    /// Filter mode used for "minifying" samples.
    pub min_filter: Filter,
    /// Fitler mode used for magnifying samplers
    pub mag_filter: Filter,
    /// Filter mode used for mip-map sampling.
    pub mip_filter: Filter,
    /// Wrap modes used when sampling outside of the `[0..1]` range occurs.
    pub wrap_mode: (WrapMode, WrapMode, WrapMode),
    // TODO anisotropy?
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

/// Opaque handle to a sampler object.
pub type SamplerHandle = storage::Handle<Sampler>;

pub(crate) struct SamplerStorage {
    pub storage: Storage<Sampler>,
}

impl SamplerStorage {
    pub(crate) fn new() -> Self {
        Self {
            storage: Storage::new(),
        }
    }

    pub(crate) unsafe fn release(self, device: &DeviceContext) {
        for (_, sampler) in self.storage {
            device.device.destroy_sampler(sampler.0);
        }
    }

    pub(crate) unsafe fn create(
        &mut self,
        device: &DeviceContext,
        create_info: SamplerCreateInfo,
    ) -> SamplerHandle {
        let create_info = create_info.into();

        let sampler = {
            device
                .device
                .create_sampler(create_info)
                .expect("Can't create sampler")
        };

        self.storage.insert(Sampler(sampler))
    }

    pub(crate) fn raw(&self, sampler: SamplerHandle) -> Option<&Sampler> {
        if self.storage.is_alive(sampler) {
            Some(&self.storage[sampler])
        } else {
            None
        }
    }

    pub(crate) fn destroy<S>(&mut self, res_list: &mut ResourceList, handles: S)
    where
        S: IntoIterator,
        S::Item: std::borrow::Borrow<SamplerHandle>,
    {
        for handle in handles.into_iter() {
            let handle = *handle.borrow();

            if let Some(sampler) = self.storage.remove(handle) {
                res_list.queue_sampler(sampler.0);
            }
        }
    }
}
