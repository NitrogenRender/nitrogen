/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use gfx::image;
use gfx::Device;

use std;
use std::borrow::Borrow;
use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};

use crate::util::allocator::{
    Allocator, AllocatorError, BufferRequest, Image as AllocImage, ImageRequest,
};
use crate::util::storage::{Handle, Storage};
use crate::util::transfer;

use crate::device::DeviceContext;
use crate::resources::command_pool::CommandPoolTransfer;
use crate::resources::semaphore_pool::SemaphoreList;
use crate::resources::semaphore_pool::SemaphorePool;
use crate::submit_group::ResourceList;

pub use gfx::format::{Component, Swizzle};

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

impl ImageDimension {
    pub fn as_triple(&self, fill: u32) -> (u32, u32, u32) {
        use self::ImageDimension::*;
        match self {
            D1 { x } => (*x, fill, fill),
            D2 { x, y } => (*x, *y, fill),
            D3 { x, y, z } => (*x, *y, *z),
        }
    }
}

#[derive(PartialOrd, PartialEq, Debug, Clone, Copy)]
pub enum ImageSizeMode {
    ContextRelative { width: f32, height: f32 },
    Absolute { width: u32, height: u32 },
}

impl ImageSizeMode {
    pub fn absolute(&self, reference: (u32, u32)) -> (u32, u32) {
        match self {
            ImageSizeMode::ContextRelative { width, height } => (
                (*width as f64 * reference.0 as f64) as u32,
                (*height as f64 * reference.1 as f64) as u32,
            ),
            ImageSizeMode::Absolute { width, height } => (*width, *height),
        }
    }
}

impl Hash for ImageSizeMode {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            ImageSizeMode::ContextRelative { .. } => {
                state.write_i8(0);
            }
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
    pub swizzle: Swizzle,
    pub kind: ViewKind,

    pub usage: T,

    pub is_transient: bool,
}

#[derive(Default, Debug, Clone, Copy, Hash)]
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

    Rgba32Float,

    E5b9g9r9Float,

    D32Float,
    D32FloatS8Uint,
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

            ImageFormat::Rgba32Float => Format::Rgba32Float,

            ImageFormat::E5b9g9r9Float => Format::E5b9g9r9Ufloat,

            ImageFormat::D32Float => Format::D32Float,
            ImageFormat::D32FloatS8Uint => Format::D32FloatS8Uint,
        }
    }
}

impl ImageFormat {
    pub fn is_depth(self) -> bool {
        match self {
            ImageFormat::D32FloatS8Uint => true,
            ImageFormat::D32Float => true,
            _ => false,
        }
    }

    pub fn is_stencil(self) -> bool {
        match self {
            ImageFormat::D32FloatS8Uint => true,
            _ => false,
        }
    }

    pub fn is_depth_stencil(self) -> bool {
        self.is_depth() && self.is_stencil()
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

pub(crate) type ImageType = AllocImage;
pub(crate) type ImageView = <back::Backend as gfx::Backend>::ImageView;

pub struct Image {
    pub(crate) image: ImageType,
    pub(crate) aspect: gfx::format::Aspects,
    pub view: ImageView,
    pub dimension: ImageDimension,
    pub format: gfx::format::Format,
}

#[derive(Debug, Display, From, Clone)]
pub enum ImageError {
    #[display(fmt = "The specified image handle was invalid")]
    HandleInvalid,

    #[display(fmt = "The data provided for uploading was not valid")]
    UploadDataInvalid,

    #[display(fmt = "Failed to allocate image")]
    CantCreate(AllocatorError),

    #[display(fmt = "Failed to map memory")]
    MappingError(gfx::mapping::Error),

    #[display(fmt = "Image View could not be created")]
    ViewError(gfx::image::ViewError),

    #[display(fmt = "Image can not be used a transfer destination")]
    CantWriteToImage,
}

impl std::error::Error for ImageError {}

pub(crate) struct ImageStorage {
    // TODO handle host visible images??
    transfer_dst: BTreeSet<usize>,

    storage: Storage<Image>,
}

pub type Result<T> = std::result::Result<T, ImageError>;

impl ImageStorage {
    pub(crate) fn new() -> Self {
        ImageStorage {
            transfer_dst: BTreeSet::new(),
            storage: Storage::new(),
        }
    }

    pub(crate) unsafe fn release(self, device: &DeviceContext) {
        let mut alloc = device.allocator();

        for (_, image) in self.storage {
            alloc.destroy_image(&device.device, image.image);
            device.device.destroy_image_view(image.view);
        }
    }

    pub(crate) unsafe fn create<T: Into<gfx::image::Usage> + Clone>(
        &mut self,
        device: &DeviceContext,
        create_info: ImageCreateInfo<T>,
    ) -> Result<ImageHandle> {
        use gfx::format::Format;

        let mut allocator = device.allocator();

        let format = create_info.format.into();

        // some formats are not supported on most GPUs, for example most 24 bit ones.
        // TODO: this should not use hardcoded values but values from the device info maybe?
        let format = match format {
            Format::Rgb8Unorm => Format::Rgba8Unorm,
            format => format,
        };

        let aspect = {
            let mut aspect = gfx::format::Aspects::empty();

            if format.is_depth() {
                aspect |= gfx::format::Aspects::DEPTH;
            }

            if format.is_stencil() {
                aspect |= gfx::format::Aspects::STENCIL;
            }

            if format.is_color() {
                aspect |= gfx::format::Aspects::COLOR;
            }

            aspect
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

            let req = ImageRequest {
                transient: create_info.is_transient,
                properties: Properties::DEVICE_LOCAL,
                kind: image_kind,
                level: 1,
                format,
                tiling: image::Tiling::Optimal,
                usage: usage_flags,
                view_caps: image::ViewCapabilities::empty(),
            };

            let image = allocator.create_image(&device.device, req)?;

            (image, usage_flags)
        };

        let image_view = device.device.create_image_view(
            image.raw(),
            create_info.kind.into(),
            format,
            create_info.swizzle,
            image::SubresourceRange {
                aspects: aspect,
                layers: 0..1,
                levels: 0..1,
            },
        )?;

        let img_store = Image {
            image,
            format,
            aspect,
            dimension: create_info.dimension,
            view: image_view,
        };

        let handle = self.storage.insert(img_store);

        if usage.contains(gfx::image::Usage::TRANSFER_DST) {
            self.transfer_dst.insert(handle.id());
        }

        Ok(handle)
    }

    pub(crate) unsafe fn upload_data(
        &self,
        device: &DeviceContext,
        sem_pool: &SemaphorePool,
        sem_list: &mut SemaphoreList,
        cmd_pool: &CommandPoolTransfer,
        res_list: &mut ResourceList,
        handle: ImageHandle,
        data: ImageUploadInfo,
    ) -> Result<()> {
        use gfx::memory::Properties;
        use gfx::PhysicalDevice;

        let image = self.storage.get(handle).ok_or(ImageError::HandleInvalid)?;

        if !self.transfer_dst.contains(&handle.id()) {
            return Err(ImageError::CantWriteToImage);
        }

        let limits: gfx::Limits = device.adapter.physical_device.limits();

        let mut allocator = device.allocator();

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
            return Err(ImageError::UploadDataInvalid);
        }

        let (upload_width, upload_height) = match data.dimension {
            ImageDimension::D1 { x } => (x, 1),
            ImageDimension::D2 { x, y } => (x, y),
            ImageDimension::D3 { .. } => {
                // TODO support 3D data?
                return Err(ImageError::UploadDataInvalid);
            }
        };

        let upload_nums = {
            let row_align = limits.min_buffer_copy_pitch_alignment as u32;
            image_copy_buffer_size(row_align, &data, (upload_width, upload_height))
        };
        let (upload_size, row_pitch, texel_size) = upload_nums;

        debug_assert!(
            upload_size >= upload_width as u64 * upload_height as u64 * texel_size as u64
        );

        let buf_req = BufferRequest {
            transient: true,
            persistently_mappable: false,
            properties: Properties::CPU_VISIBLE | Properties::COHERENT,
            usage: gfx::buffer::Usage::TRANSFER_SRC | gfx::buffer::Usage::TRANSFER_DST,
            size: upload_size,
        };

        let staging = allocator.create_buffer(&device.device, buf_req)?;

        // write to staging buffer
        {
            use crate::util::allocator::Block;

            let range = staging.block().range();

            let mut writer = device
                .device
                .acquire_mapping_writer(staging.block().memory(), range)?;

            // Alignment strikes back again! We do copy all the rows, but the row length in the
            // staging buffer might be bigger than in the upload data, so we need to construct
            // a slice for each row instead of just copying *everything*
            for y in 0..upload_height as usize {
                let src_start = y * (upload_width as usize) * texel_size;
                let src_end = (y + 1) * (upload_width as usize) * texel_size;

                let row = &data.data[src_start..src_end];

                let dst_start = y * row_pitch as usize;
                let dst_end = dst_start + row.len();

                writer[dst_start..dst_end].copy_from_slice(row);
            }

            device.device.release_mapping_writer(writer).unwrap();
        }

        // create image upload data

        use crate::transfer::BufferImageTransfer;

        let transfer_data = BufferImageTransfer {
            src: &staging,
            dst: &image.image,
            subresource_range: gfx::image::SubresourceRange {
                aspects: gfx::format::Aspects::COLOR,
                levels: 0..1,
                layers: 0..1,
            },
            copy_information: gfx::command::BufferImageCopy {
                buffer_offset: 0,
                buffer_width: row_pitch / (texel_size as u32),
                buffer_height: upload_height,
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
                    width: upload_width,
                    height: upload_height,
                    depth: 1,
                },
            },
        };

        transfer::copy_buffers_to_images(device, sem_pool, sem_list, cmd_pool, &[transfer_data]);

        res_list.queue_buffer(staging);

        Ok(())
    }

    pub fn raw(&self, image: ImageHandle) -> Option<&Image> {
        if self.storage.is_alive(image) {
            Some(&self.storage[image])
        } else {
            None
        }
    }

    pub fn destroy<I>(&mut self, res_list: &mut ResourceList, handles: I)
    where
        I: IntoIterator,
        I::Item: std::borrow::Borrow<ImageHandle>,
    {
        for handle in handles.into_iter() {
            let handle = *handle.borrow();
            match self.storage.remove(handle) {
                Some(image) => {
                    res_list.queue_image(image.image);
                    res_list.queue_image_view(image.view);

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
