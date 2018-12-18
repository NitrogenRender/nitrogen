/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use nitrogen;

use nitrogen::sampler;

type SamplerId = usize;
type SamplerGeneration = u64;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SamplerHandle(pub SamplerId, pub SamplerGeneration);

impl SamplerHandle {
    pub fn into(self) -> sampler::SamplerHandle {
        unsafe { sampler::SamplerHandle::new(self.0, self.1) }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub enum SamplerFilter {
    Nearest,
    Linear,
}

impl From<SamplerFilter> for sampler::Filter {
    fn from(filter: SamplerFilter) -> Self {
        match filter {
            SamplerFilter::Nearest => sampler::Filter::Nearest,
            SamplerFilter::Linear => sampler::Filter::Linear,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub enum SamplerWrapMode {
    Tile,
    Mirror,
    Clamp,
    Border,
}

impl From<SamplerWrapMode> for sampler::WrapMode {
    fn from(mode: SamplerWrapMode) -> Self {
        match mode {
            SamplerWrapMode::Tile => sampler::WrapMode::Tile,
            SamplerWrapMode::Mirror => sampler::WrapMode::Mirror,
            SamplerWrapMode::Clamp => sampler::WrapMode::Clamp,
            SamplerWrapMode::Border => sampler::WrapMode::Border,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SamplerCreateInfo {
    min_filter: SamplerFilter,
    mag_filter: SamplerFilter,
    mip_filter: SamplerFilter,
    wrap_mode: [SamplerWrapMode; 3],
}

impl From<SamplerCreateInfo> for sampler::SamplerCreateInfo {
    fn from(create: SamplerCreateInfo) -> Self {
        sampler::SamplerCreateInfo {
            min_filter: create.min_filter.into(),
            mag_filter: create.mag_filter.into(),
            mip_filter: create.mip_filter.into(),
            wrap_mode: (
                create.wrap_mode[0].into(),
                create.wrap_mode[1].into(),
                create.wrap_mode[2].into(),
            ),
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn sampler_create(
    _context: *mut nitrogen::Context,
    _create_infos: *const SamplerCreateInfo,
    _handles: *mut SamplerHandle,
    _count: usize,
) {
    /*
    let context = &mut *context;

    let internal_create_infos = slice::from_raw_parts(create_infos, count);
    let handles = slice::from_raw_parts_mut(handles, count);

    let internal_create = internal_create_infos
        .iter()
        .map(|c| {
            let create_info = Into::<sampler::SamplerCreateInfo>::into(*c);
            create_info
        })
        .collect::<SmallVec<[_; 16]>>();

    let sampler_handles = context
        .sampler_storage
        .create(&context.device_ctx, internal_create.as_slice());

    for (i, sampler) in sampler_handles.into_iter().enumerate() {
        handles[i] = SamplerHandle(sampler.id(), sampler.generation());
    }
    */
}

#[no_mangle]
pub unsafe extern "C" fn sampler_destroy(
    _context: *mut nitrogen::Context,
    _samplers: *const SamplerHandle,
    _sampler_count: usize,
) {
    /*
    let context = &mut *context;

    let samplers = slice::from_raw_parts(samplers, sampler_count)
        .iter()
        .map(|s| SamplerHandle::into(s.clone()))
        .collect::<SmallVec<[_; 16]>>();

    // context
    //     .sampler_storage
    //     .destroy(&context.device_ctx, samplers.as_slice());
    */
}
