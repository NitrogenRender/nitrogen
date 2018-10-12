use nitrogen;

use nitrogen::sampler;

type SamplerId = usize;
type SamplerGeneration = u64;

#[repr(C)]
pub struct SamplerHandle(pub SamplerId, pub SamplerGeneration);

impl From<SamplerHandle> for sampler::SamplerHandle {
    fn from(handle: SamplerHandle) -> Self {
        sampler::SamplerHandle(handle.0, handle.1)
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
    context: *mut nitrogen::Context,
    create_info: SamplerCreateInfo,
    handle: *mut SamplerHandle,
) -> bool {
    let context = &mut *context;

    let internal_create = create_info.into();

    let sampler_handle = context.sampler_storage.create(&context.device_ctx, internal_create);

    *handle = SamplerHandle(sampler_handle.0, sampler_handle.1);

    true
}

#[no_mangle]
pub unsafe extern "C" fn sampler_destroy(
    context: *mut nitrogen::Context,
    sampler: SamplerHandle,
) -> bool {
    let context = &mut *context;
    context.sampler_storage.destroy(&context.device_ctx, sampler.into())
}