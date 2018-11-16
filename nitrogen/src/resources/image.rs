use back;
use gfx;
use gfxm;

use failure_derive::Fail;

use gfx::image;
use gfx::Device;
use gfxm::Factory;
use gfxm::SmartAllocator;

use std;
use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};

use smallvec::smallvec;
use smallvec::SmallVec;

use util::storage::{Handle, Storage};

use transfer::TransferContext;

use device::DeviceContext;

#[derive(Copy, Clone, Debug)]
pub enum ImageDimension {
    D1 { x: u32 },
    D2 { x: u32, y: u32 },
    D3 { x: u32, y: u32, z: u32 },
}

impl Default for ImageDimension {
    fn default() -> Self {
        ImageDimension::D2 { x: 1, y: 1 }
    }
}

#[derive(PartialOrd, PartialEq, Debug, Clone, Copy)]
pub enum ImageSizeMode {
    ContextRelative { width: f32, height: f32 },
    Absolute { width: u32, height: u32 },
}

impl Hash for ImageSizeMode {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            ImageSizeMode::ContextRelative { .. } => {
                state.write_i8(0);
            },
            ImageSizeMode::Absolute { width, height } => {
                state.write_i8(1);
                state.write_u32(*width);
                state.write_u32(*height);
            }
        }
    }
}

pub type ImageHandle = Handle<Image>;

#[derive(Default, Clone)]
pub struct ImageCreateInfo<T: Into<gfx::image::Usage>> {
    pub dimension: ImageDimension,
    pub num_layers: u16,
    pub num_samples: u8,
    pub num_mipmaps: u8,
    pub format: ImageFormat,
    pub kind: ViewKind,

    pub usage: T,

    pub is_transient: bool,
}

#[derive(Default, Clone, Copy)]
pub struct ImageUsage {
    pub transfer_src: bool,
    pub transfer_dst: bool,
    pub sampling: bool,
    pub color_attachment: bool,
    pub depth_stencil_attachment: bool,
    pub storage_image: bool,
    pub input_attachment: bool,
}

impl From<ImageUsage> for gfx::image::Usage {
    fn from(val: ImageUsage) -> Self {
        use gfx::image::Usage;

        let mut flags = Usage::empty();

        if val.transfer_src {
            flags |= Usage::TRANSFER_SRC;
        }
        if val.transfer_dst {
            flags |= Usage::TRANSFER_DST;
        }

        if val.sampling {
            flags |= Usage::SAMPLED;
        }
        if val.color_attachment {
            flags |= Usage::COLOR_ATTACHMENT;
        }
        if val.depth_stencil_attachment {
            flags |= Usage::DEPTH_STENCIL_ATTACHMENT;
        }
        if val.storage_image {
            flags |= Usage::STORAGE;
        }
        if val.input_attachment {
            flags |= Usage::INPUT_ATTACHMENT;
        }

        flags
    }
}

pub struct ImageUploadInfo<'a> {
    pub data: &'a [u8],
    pub format: ImageFormat,
    pub dimension: ImageDimension,
    pub target_offset: (u32, u32, u32),
}

#[derive(Copy, Clone, Debug, PartialEq, Hash)]
pub enum ImageFormat {
    RUnorm,
    RgUnorm,
    RgbUnorm,
    RgbaUnorm,

    E5b9g9r9Float,
}

impl Default for ImageFormat {
    fn default() -> Self {
        ImageFormat::RgbaUnorm
    }
}

impl From<ImageFormat> for gfx::format::Format {
    fn from(format: ImageFormat) -> Self {
        use gfx::format::Format;
        match format {
            ImageFormat::RUnorm => Format::R8Unorm,
            ImageFormat::RgUnorm => Format::Rg8Unorm,
            ImageFormat::RgbUnorm => Format::Rgb8Unorm,
            ImageFormat::RgbaUnorm => Format::Rgba8Unorm,

            ImageFormat::E5b9g9r9Float => Format::E5b9g9r9Ufloat,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub enum ViewKind {
    D1,
    D1Array,
    D2,
    D2Array,
    D3,
    Cube,
    CubeArray,
}

impl Default for ViewKind {
    fn default() -> Self {
        ViewKind::D2
    }
}

impl From<ViewKind> for gfx::image::ViewKind {
    fn from(kind: ViewKind) -> Self {
        use gfx::image::ViewKind as vk;
        match kind {
            ViewKind::D1 => vk::D1,
            ViewKind::D1Array => vk::D1Array,
            ViewKind::D2 => vk::D2,
            ViewKind::D2Array => vk::D2Array,
            ViewKind::D3 => vk::D3,
            ViewKind::Cube => vk::Cube,
            ViewKind::CubeArray => vk::CubeArray,
        }
    }
}

pub type ImageType = <SmartAllocator<back::Backend> as Factory<back::Backend>>::Image;
type ImageView = <back::Backend as gfx::Backend>::ImageView;

pub struct Image {
    pub image: ImageType,
    pub view: ImageView,
    pub dimension: ImageDimension,
    pub format: gfx::format::Format,
}

#[derive(Debug, Fail, Clone)]
pub enum ImageError {
    #[fail(display = "The specified image handle was invalid")]
    HandleInvalid,

    #[fail(display = "The data provided for uploading was not valid")]
    UploadDataInvalid,

    #[fail(display = "Failed to allocate image")]
    CantCreate(#[cause] gfxm::FactoryError),

    #[fail(display = "Failed to map memory")]
    MappingError(#[cause] gfx::mapping::Error),

    #[fail(display = "Image View could not be created")]
    ViewError(#[cause] gfx::image::ViewError),

    #[fail(display = "Image can not be used a transfer destination")]
    CantWriteToImage,
}

impl From<gfxm::FactoryError> for ImageError {
    fn from(err: gfxm::FactoryError) -> Self {
        ImageError::CantCreate(err)
    }
}
impl From<gfx::mapping::Error> for ImageError {
    fn from(err: gfx::mapping::Error) -> Self {
        ImageError::MappingError(err)
    }
}

impl From<gfx::image::ViewError> for ImageError {
    fn from(err: gfx::image::ViewError) -> Self {
        ImageError::ViewError(err)
    }
}

pub struct ImageStorage {
    // TODO handle host visible images??
    transfer_dst: BTreeSet<usize>,

    storage: Storage<Image>,
}

pub type Result<T> = std::result::Result<T, ImageError>;

impl ImageStorage {
    pub fn new() -> Self {
        ImageStorage {
            transfer_dst: BTreeSet::new(),
            storage: Storage::new(),
        }
    }

    pub fn release(self, device: &DeviceContext) {
        let mut alloc = device.allocator();

        for (_, image) in self.storage.into_iter() {
            alloc.destroy_image(&device.device, image.image);
            device.device.destroy_image_view(image.view);
        }
    }

    pub fn create<T: Into<gfx::image::Usage> + Clone>(
        &mut self,
        device: &DeviceContext,
        create_infos: &[ImageCreateInfo<T>],
    ) -> SmallVec<[Result<ImageHandle>; 16]> {
        use gfx::format::Format;

        let mut result = SmallVec::with_capacity(create_infos.len());

        let mut allocator = device.allocator();

        for create_info in create_infos {
            let format = create_info.format.into();

            // some formats are not supported on most GPUs, for example most 24 bit ones.
            // TODO: this should not use hardcoded values but values from the device info maybe?
            let format = match format {
                Format::Rgb8Unorm => Format::Rgba8Unorm,
                format => format,
            };

            let (image, usage) = {
                let image_kind = match create_info.dimension {
                    ImageDimension::D1 { x } => image::Kind::D1(x, create_info.num_layers),
                    ImageDimension::D2 { x, y } => {
                        image::Kind::D2(x, y, create_info.num_layers, create_info.num_samples)
                    }
                    ImageDimension::D3 { x, y, z } => image::Kind::D3(x, y, z),
                };

                use gfx::memory::Properties;

                let usage_flags = create_info.usage.clone().into();

                let alloc_type = if create_info.is_transient {
                    gfxm::Type::ShortLived
                } else {
                    gfxm::Type::General
                };

                let image = allocator.create_image(
                    &device.device,
                    (alloc_type, Properties::DEVICE_LOCAL),
                    image_kind,
                    1,
                    format,
                    image::Tiling::Optimal,
                    usage_flags,
                    image::ViewCapabilities::empty(),
                );

                match image {
                    Err(e) => {
                        result.push(Err(e.into()));
                        continue;
                    }
                    Ok(i) => (i, usage_flags),
                }
            };

            let image_view = {
                match device.device.create_image_view(
                    image.raw(),
                    create_info.kind.into(),
                    format,
                    gfx::format::Swizzle::NO,
                    image::SubresourceRange {
                        aspects: gfx::format::Aspects::COLOR,
                        layers: 0..1,
                        levels: 0..1,
                    },
                ) {
                    Err(e) => {
                        result.push(Err(e.into()));
                        continue;
                    }
                    Ok(iv) => iv,
                }
            };

            let img_store = Image {
                image,
                format,
                dimension: create_info.dimension,
                view: image_view,
            };

            let (handle, _) = self.storage.insert(img_store);

            if usage.contains(gfx::image::Usage::TRANSFER_DST) {
                self.transfer_dst.insert(handle.id());
            }

            result.push(Ok(handle));
        }

        result
    }

    pub fn upload_data(
        &self,
        device: &DeviceContext,
        transfer: &mut TransferContext,
        images: &[(ImageHandle, ImageUploadInfo)],
    ) -> SmallVec<[Result<()>; 16]> {
        let mut results = smallvec![Ok(()); images.len()];

        let mut data: SmallVec<[_; 16]> = images.iter().enumerate().collect();
        data.as_mut_slice()
            .sort_by_key(|(_, (handle, _))| handle.id());

        // categorize images
        let (transferable, other) = {
            let mut transferable = SmallVec::<[_; 16]>::new();
            let mut other = SmallVec::<[_; 16]>::new();

            for (idx, (handle, data)) in data {
                let handle = *handle;
                if !self.storage.is_alive(handle) {
                    results[idx] = Err(ImageError::HandleInvalid);
                    continue;
                }

                if self.transfer_dst.contains(&handle.id()) {
                    transferable.push((idx, handle, data));
                } else {
                    other.push((idx, handle, data));
                }
            }

            (transferable, other)
        };

        // Can't upload to those..
        for (idx, _, _) in other {
            results[idx] = Err(ImageError::CantWriteToImage);
        }

        use gfx::memory::Properties;
        use gfx::PhysicalDevice;

        let limits: gfx::Limits = device.adapter.physical_device.limits();

        let mut allocator = device.allocator();

        let staging_data = transferable
            .as_slice()
            .iter()
            .filter_map(|(idx, handle, data)| {
                let idx = *idx;

                let image = &self.storage[*handle];
                let dimensions = image.dimension;

                let upload_data_fits = {
                    use self::ImageDimension as I;
                    match (dimensions, data.dimension) {
                        (I::D1 { x: dx }, I::D1 { x: sx }) => (sx + data.target_offset.0) <= dx,
                        (I::D2 { x: dx, y: dy }, I::D2 { x: sx, y: sy }) => {
                            (sx + data.target_offset.0) <= dx && (sy + data.target_offset.1) <= dy
                        }
                        (
                            I::D3 {
                                x: dx,
                                y: dy,
                                z: dz,
                            },
                            I::D3 {
                                x: sx,
                                y: sy,
                                z: sz,
                            },
                        ) => {
                            (sx + data.target_offset.0) <= dx
                                && (sy + data.target_offset.1) <= dy
                                && (sz + data.target_offset.2) <= dz
                        }
                        _ => false,
                    }
                };

                if !upload_data_fits {
                    results[idx] = Err(ImageError::UploadDataInvalid);
                    return None;
                }

                let (upload_width, upload_height) = match data.dimension {
                    ImageDimension::D1 { x } => (x, 1),
                    ImageDimension::D2 { x, y } => (x, y),
                    ImageDimension::D3 { .. } => {
                        // TODO support 3D data?
                        results[idx] = Err(ImageError::UploadDataInvalid);
                        return None;
                    }
                };

                let upload_nums = {
                    let row_align = limits.min_buffer_copy_pitch_alignment as u32;
                    image_copy_buffer_size(row_align, &data, (upload_width, upload_height))
                };
                let (upload_size, _row_pitch, texel_size) = upload_nums;

                debug_assert!(
                    upload_size >= upload_width as u64 * upload_height as u64 * texel_size as u64
                );

                let staging_buffer = match allocator.create_buffer(
                    &device.device,
                    (
                        gfxm::Type::ShortLived,
                        Properties::CPU_VISIBLE | Properties::COHERENT,
                    ),
                    upload_size,
                    gfx::buffer::Usage::TRANSFER_SRC | gfx::buffer::Usage::TRANSFER_DST,
                ) {
                    Err(e) => {
                        results[idx] = Err(e.into());
                        return None;
                    }
                    Ok(buffer) => buffer,
                };

                Some((
                    idx,
                    image,
                    data,
                    staging_buffer,
                    upload_nums,
                    (upload_width, upload_height),
                ))
            }).collect::<SmallVec<[_; 16]>>();

        {
            let upload_data = staging_data
                .as_slice()
                .iter()
                .filter_map(|(idx, image, data, staging, upload_nums, upload_dims)| {
                    let (_upload_size, row_pitch, texel_size) = *upload_nums;

                    let (width, height) = *upload_dims;

                    // write to staging buffer
                    {
                        use gfxm::Block;

                        let range = staging.range();

                        let mut writer = match device
                            .device
                            .acquire_mapping_writer(staging.memory(), range)
                        {
                            Err(e) => {
                                results[*idx] = Err(e.into());
                                return None;
                            }
                            Ok(x) => x,
                        };

                        // Alignment strikes back again! We do copy all the rows, but the row length in the
                        // staging buffer might be bigger than in the upload data, so we need to construct
                        // a slice for each row instead of just copying *everything*
                        for y in 0..height as usize {
                            let src_start = y * (width as usize) * texel_size;
                            let src_end = (y + 1) * (width as usize) * texel_size;

                            let row = &data.data[src_start..src_end];

                            let dst_start = y * row_pitch as usize;
                            let dst_end = dst_start + row.len();

                            writer[dst_start..dst_end].copy_from_slice(row);
                        }

                        device.device.release_mapping_writer(writer);
                    }

                    // create image upload data

                    use transfer::BufferImageTransfer;

                    let transfer_data = BufferImageTransfer {
                        src: staging,
                        dst: &image.image,
                        subresource_range: gfx::image::SubresourceRange {
                            aspects: gfx::format::Aspects::COLOR,
                            levels: 0..1,
                            layers: 0..1,
                        },
                        copy_information: gfx::command::BufferImageCopy {
                            buffer_offset: 0,
                            buffer_width: row_pitch / (texel_size as u32),
                            buffer_height: height,
                            image_layers: gfx::image::SubresourceLayers {
                                aspects: gfx::format::Aspects::COLOR,
                                level: 0,
                                layers: 0..1,
                            },
                            image_offset: image::Offset {
                                x: data.target_offset.0 as i32,
                                y: data.target_offset.1 as i32,
                                z: data.target_offset.2 as i32,
                            },
                            image_extent: image::Extent {
                                width,
                                height,
                                depth: 1,
                            },
                        },
                    };

                    Some(transfer_data)
                }).collect::<SmallVec<[_; 16]>>();

            transfer.copy_buffers_to_images(device, upload_data.as_slice());
        }

        staging_data
            .into_iter()
            .for_each(|(_, _, _, staging_buffer, _, _)| {
                allocator.destroy_buffer(&device.device, staging_buffer);
            });

        results
    }

    pub fn raw(&self, image: ImageHandle) -> Option<&Image> {
        if self.storage.is_alive(image) {
            Some(&self.storage[image])
        } else {
            None
        }
    }

    pub fn destroy(&mut self, device: &DeviceContext, handles: &[ImageHandle]) {
        let mut allocator = device.allocator();

        for handle in handles {
            match self.storage.remove(*handle) {
                Some(image) => {
                    allocator.destroy_image(&device.device, image.image);
                    device.device.destroy_image_view(image.view);

                    if self.transfer_dst.contains(&handle.id()) {
                        self.transfer_dst.remove(&handle.id());
                    }
                }
                None => {}
            }
        }
    }
}

/// Compute the total size in bytes and the row stride
/// for a buffer that should be used to copy data into an image.
fn image_copy_buffer_size(
    row_align: u32,
    upload_info: &ImageUploadInfo,
    (width, height): (u32, u32),
) -> (u64, u32, usize) {
    let texel_size = upload_info.data.len() / (width * height) as usize;

    // Because low level graphics are low level, we need to take care about buffer
    // alignment here.
    //
    // For example an RGBA8 image with 11 * 11 dims
    // has
    //  - "texel_size" of 4 (4 components (rgba) with 1 byte size)
    //  - "width" of 11
    //  - "height" of 10
    //
    // If we want to make a buffer used for copying the image, the "row size" is important
    // since graphics APIs like to have a certain *alignment* for copying the data.
    //
    // Let's assume the "row alignment" is 8, that means each row size has to be divisible
    // by 8. In the RGBA8 example, each row has a size of `width * stride = 44`, which is
    // not divisible evenly by 8, so we need to add some padding.
    // In this case the padding we add needs to be 4 bytes, so we get to a row width of 48.
    //
    // Generally this padding is there because it seems like GPUs like it when
    // `offset_of(x, y + 1) = offset_of(x, y) + n * alignment`
    // (I strongly assume that that's because of SIMD operations)
    //

    // This mask says how many bits from the right need to be 0
    let row_alignment_mask = row_align as u32 - 1;

    // We add the alignment mask, then cut away everything stuff on the right so it's all 0s
    let row_pitch = (width * texel_size as u32 + row_alignment_mask) & !row_alignment_mask;

    let buffer_size = (height * row_pitch) as u64;

    (buffer_size, row_pitch, texel_size)
}
