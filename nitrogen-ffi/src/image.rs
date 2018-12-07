use nitrogen;

use smallvec::SmallVec;

use std::slice;

type ImageId = usize;
type ImageGeneration = u64;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct ImageHandle(pub ImageId, pub ImageGeneration);

impl ImageHandle {
    pub fn into(self) -> nitrogen::image::ImageHandle {
        nitrogen::image::ImageHandle::new(self.0, self.1)
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct ImageCreateInfo {
    pub dimension: ImageDimension,
    pub num_layers: u16,
    pub num_samples: u8,
    pub num_mipmaps: u8,
    pub format: ImageFormat,
    pub kind: ImageViewKind,

    pub used_as_transfer_src: bool,
    pub used_as_transfer_dst: bool,
    pub used_for_sampling: bool,
    pub used_as_color_attachment: bool,
    pub used_as_depth_stencil_attachment: bool,
    pub used_as_storage_image: bool,
    pub used_as_input_attachment: bool,
    pub is_transient: bool,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub enum ImageViewKind {
    D1,
    D1Array,
    D2,
    D2Array,
    D3,
    Cube,
    CubeArray,
}

impl From<ImageViewKind> for nitrogen::image::ViewKind {
    fn from(kind: ImageViewKind) -> Self {
        use nitrogen::image::ViewKind as vk;
        match kind {
            ImageViewKind::D1 => vk::D1,
            ImageViewKind::D1Array => vk::D1Array,
            ImageViewKind::D2 => vk::D2,
            ImageViewKind::D2Array => vk::D2Array,
            ImageViewKind::D3 => vk::D3,
            ImageViewKind::Cube => vk::Cube,
            ImageViewKind::CubeArray => vk::CubeArray,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub enum ImageFormat {
    RUnorm,
    RgUnorm,
    RgbUnorm,
    RgbaUnorm,

    E5b9g9r9Float,
}

impl From<ImageFormat> for nitrogen::image::ImageFormat {
    fn from(format: ImageFormat) -> Self {
        use nitrogen::image::ImageFormat as ni;
        match format {
            ImageFormat::RUnorm => ni::RUnorm,
            ImageFormat::RgUnorm => ni::RgUnorm,
            ImageFormat::RgbUnorm => ni::RgbUnorm,
            ImageFormat::RgbaUnorm => ni::RgbaUnorm,

            ImageFormat::E5b9g9r9Float => ni::E5b9g9r9Float,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub enum ImageDimension {
    D1 { x: u32 },
    D2 { x: u32, y: u32 },
    D3 { x: u32, y: u32, z: u32 },
}

impl From<ImageDimension> for nitrogen::image::ImageDimension {
    fn from(dim: ImageDimension) -> Self {
        match dim {
            ImageDimension::D1 { x } => nitrogen::image::ImageDimension::D1 { x },
            ImageDimension::D2 { x, y } => nitrogen::image::ImageDimension::D2 { x, y },
            ImageDimension::D3 { x, y, z } => nitrogen::image::ImageDimension::D3 { x, y, z },
        }
    }
}

impl From<nitrogen::image::ImageDimension> for ImageDimension {
    fn from(dim: nitrogen::image::ImageDimension) -> Self {
        match dim {
            nitrogen::image::ImageDimension::D1 { x } => ImageDimension::D1 { x },
            nitrogen::image::ImageDimension::D2 { x, y } => ImageDimension::D2 { x, y },
            nitrogen::image::ImageDimension::D3 { x, y, z } => ImageDimension::D3 { x, y, z },
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct ImageUploadInfo {
    pub data: *const u8,
    pub data_len: u64,
    pub format: ImageFormat,
    pub dimension: ImageDimension,
    pub target_offset: [u32; 3],
}

#[no_mangle]
pub unsafe extern "C" fn image_create(
    context: *mut nitrogen::Context,
    create_infos: *const ImageCreateInfo,
    handles: *mut ImageHandle,
    successes: *mut bool,
    count: usize,
) {
    /*
    let context = &mut *context;

    let create_infos = slice::from_raw_parts(create_infos, count);

    let internal_create_infos = (0..count)
        .into_iter()
        .map(|i| {
            let create_info = &create_infos[i];

            nitrogen::image::ImageCreateInfo {
                dimension: create_info.dimension.into(),
                format: create_info.format.into(),
                num_mipmaps: create_info.num_mipmaps,
                num_samples: create_info.num_samples,
                num_layers: create_info.num_layers,
                kind: create_info.kind.into(),

                usage: nitrogen::image::ImageUsage {
                    transfer_src: create_info.used_as_transfer_src,
                    transfer_dst: create_info.used_as_transfer_dst,
                    sampling: create_info.used_for_sampling,
                    color_attachment: create_info.used_as_color_attachment,
                    depth_stencil_attachment: create_info.used_as_depth_stencil_attachment,
                    storage_image: create_info.used_as_storage_image,
                    input_attachment: create_info.used_as_input_attachment,
                },

                is_transient: create_info.is_transient,
            }
        })
        .collect::<SmallVec<[_; 16]>>();

    let results = context
        .image_storage
        .create(&context.device_ctx, &internal_create_infos);

    let handles = slice::from_raw_parts_mut(handles, count);
    let successes = slice::from_raw_parts_mut(successes, count);

    for (i, result) in results.into_iter().enumerate() {
        match result {
            Ok(t) => {
                handles[i] = ImageHandle(t.id(), t.generation());
                successes[i] = true;
            }
            Err(_) => {
                successes[i] = false;
            }
        }
    }
    */
}

#[no_mangle]
pub unsafe extern "C" fn image_upload_data(
    context: *mut nitrogen::Context,
    images: *const ImageHandle,
    data: *const ImageUploadInfo,
    successes: *mut bool,
    count: usize,
) {
    /*
    let context = &mut *context;

    let images = slice::from_raw_parts(images, count);
    let data = slice::from_raw_parts(data, count);
    let successes = slice::from_raw_parts_mut(successes, count);

    let upload_infos = images
        .iter()
        .zip(data)
        .map(|(handle, data)| {
            let handle = handle.clone().into();
            let upload_info = nitrogen::image::ImageUploadInfo {
                data: slice::from_raw_parts(data.data, data.data_len as usize),
                format: data.format.into(),
                dimension: data.dimension.into(),
                target_offset: (
                    data.target_offset[0],
                    data.target_offset[1],
                    data.target_offset[2],
                ),
            };

            (handle, upload_info)
        })
        .collect::<SmallVec<[_; 16]>>();
    /*

    let results = context.image_storage.upload_data(
        &context.device_ctx,
        &mut context.transfer,
        upload_infos.as_slice(),
    );

    for (i, result) in results.into_iter().enumerate() {
        successes[i] = result.is_ok();
    }
    */

    */
}

#[no_mangle]
pub unsafe extern "C" fn image_destroy(
    context: *mut nitrogen::Context,
    images: *const ImageHandle,
    images_count: usize,
) {
    use std;

    let context = &mut *context;

    let images = std::slice::from_raw_parts(images, images_count)
        .iter()
        .map(|image| (*image).into())
        .collect::<Vec<_>>();

    // context.image_storage.destroy(&context.device_ctx, &images);
}
