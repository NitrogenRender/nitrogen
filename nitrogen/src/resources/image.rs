use back;
use gfx;
use gfxm;

use gfx::command;
use gfx::image;
use gfx::memory;
use gfx::Device;
use gfxm::Factory;
use gfxm::SmartAllocator;

use std;

use util::storage;
use util::storage::{Handle, Storage};

use device::DeviceContext;
use device::QueueType;

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

pub type ImageHandle = Handle<(Image, ImageView)>;

#[derive(Default)]
pub struct ImageCreateInfo {
    pub dimension: ImageDimension,
    pub num_layers: u16,
    pub num_samples: u8,
    pub num_mipmaps: u8,
    pub format: ImageFormat,
    pub kind: ViewKind,

    pub used_as_transfer_src: bool,
    pub used_as_transfer_dst: bool,
    pub used_for_sampling: bool,
    pub used_as_color_attachment: bool,
    pub used_as_depth_stencil_attachment: bool,
    pub used_as_storage_image: bool,
    pub used_as_input_attachment: bool,
}

pub struct ImageUploadInfo<'a> {
    pub data: &'a [u8],
    pub format: ImageFormat,
    pub dimension: ImageDimension,
    pub target_offset: (u32, u32, u32),
}

#[derive(Copy, Clone, Debug, PartialEq)]
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

type Image = <SmartAllocator<back::Backend> as Factory<back::Backend>>::Image;
type ImageView = <back::Backend as gfx::Backend>::ImageView;

#[derive(Debug, Fail)]
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
    dimensions: Vec<ImageDimension>,
    formats: Vec<gfx::format::Format>,

    storage: Storage<(Image, ImageView)>,

    command_pool: gfx::CommandPool<back::Backend, gfx::General>,
}

pub type Result<T> = std::result::Result<T, ImageError>;

impl ImageStorage {
    pub fn new(device: &DeviceContext) -> Self {
        let command_pool = {
            let queue_group = device.queue_group();

            device.device.create_command_pool_typed(
                &queue_group,
                gfx::pool::CommandPoolCreateFlags::empty(),
                16,
            )
        };

        ImageStorage {
            dimensions: vec![],
            formats: vec![],
            storage: Storage::new(),
            command_pool,
        }
    }

    pub fn release(self, device: &DeviceContext) {
        device
            .device
            .destroy_command_pool(self.command_pool.into_raw());
    }

    pub fn create(
        &mut self,
        device: &DeviceContext,
        create_info: ImageCreateInfo,
    ) -> Result<ImageHandle> {
        use gfx::format::Format;

        let format = create_info.format.into();

        // some formats are not supported on most GPUs, for example most 24 bit ones.
        // TODO: this should not use hardcoded values but values from the device info maybe?
        let format = match format {
            Format::Rgb8Unorm => Format::Rgba8Unorm,
            format => format,
        };

        let image = {
            let image_kind = match create_info.dimension {
                ImageDimension::D1 { x } => image::Kind::D1(x, create_info.num_layers),
                ImageDimension::D2 { x, y } => {
                    image::Kind::D2(x, y, create_info.num_layers, create_info.num_samples)
                }
                ImageDimension::D3 { x, y, z } => image::Kind::D3(x, y, z),
            };

            use gfx::memory::Properties;

            let usage_flags = {
                use gfx::image::Usage;

                let mut flags = Usage::empty();

                if create_info.used_as_transfer_src {
                    flags |= Usage::TRANSFER_SRC;
                }
                if create_info.used_as_transfer_dst {
                    flags |= Usage::TRANSFER_DST;
                }

                if create_info.used_for_sampling {
                    flags |= Usage::SAMPLED;
                }
                if create_info.used_as_color_attachment {
                    flags |= Usage::COLOR_ATTACHMENT;
                }
                if create_info.used_as_depth_stencil_attachment {
                    flags |= Usage::DEPTH_STENCIL_ATTACHMENT;
                }
                if create_info.used_as_storage_image {
                    flags |= Usage::STORAGE;
                }
                if create_info.used_as_input_attachment {
                    flags |= Usage::INPUT_ATTACHMENT;
                }

                flags
            };

            device.allocator().create_image(
                &device.device,
                (gfxm::Type::General, Properties::DEVICE_LOCAL),
                image_kind,
                1,
                format,
                image::Tiling::Optimal,
                usage_flags,
                image::ViewCapabilities::empty(),
            )?
        };

        let image_view = {
            device.device.create_image_view(
                image.raw(),
                create_info.kind.into(),
                format,
                gfx::format::Swizzle::NO,
                image::SubresourceRange {
                    aspects: gfx::format::Aspects::COLOR,
                    layers: 0..1,
                    levels: 0..1,
                },
            )?
        };

        let (handle, op) = self.storage.insert((image, image_view));

        match op {
            storage::InsertOp::Grow => {
                self.formats.push(format);
                self.dimensions.push(create_info.dimension);
            }
            storage::InsertOp::Inplace => {
                self.formats[handle.id()] = format;
                self.dimensions[handle.id()] = create_info.dimension;
            }
        }

        Ok(handle)
    }

    pub fn upload_data(
        &mut self,
        device: &DeviceContext,
        image: ImageHandle,
        data: ImageUploadInfo,
    ) -> Result<()> {
        use gfxm::Factory;

        let image_dimensions = self.dimension(image).ok_or(ImageError::HandleInvalid)?;

        let upload_data_fits = {
            use self::ImageDimension as I;
            match (image_dimensions, data.dimension) {
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

        let (image_width, image_height) = match data.dimension {
            ImageDimension::D1 { x } => (x, 1),
            ImageDimension::D2 { x, y } => (x, y),
            ImageDimension::D3 { .. } => unimplemented!("Setting of 3D texture data"),
        };

        if !upload_data_fits {
            return Err(ImageError::UploadDataInvalid);
        }

        let conversion_needed = gfx::format::Format::from(data.format) != self.formats[image.0];

        let (buffer_size, row_pitch, texel_size) = {
            use gfx::PhysicalDevice;

            let limits = device.adapter.physical_device.limits();
            let row_align = limits.min_buffer_copy_pitch_alignment as u32;
            image_copy_buffer_size(row_align, &data, (image_width, image_height))
        };

        let staging_buffer = {
            use gfx::memory::Properties;

            let mut alloc = device.allocator();

            alloc.create_buffer(
                &device.device,
                (
                    gfxm::Type::ShortLived,
                    Properties::CPU_VISIBLE | Properties::COHERENT,
                ),
                buffer_size,
                gfx::buffer::Usage::TRANSFER_SRC | gfx::buffer::Usage::TRANSFER_DST,
            )?
        };

        {
            use gfxm::Block;

            let mut writer = device
                .device
                .acquire_mapping_writer(staging_buffer.memory(), 0..buffer_size)?;

            // Alignment strikes back again! We do copy all the rows, but the row length in the
            // staging buffer might be bigger than in the upload data, so we need to construct
            // a slice for each row instead of just copying *everything*
            for y in 0..image_height as usize {
                let row = &data.data[y * (image_width as usize) * texel_size .. (y + 1) * (image_width as usize) * texel_size];
                let dest_base = y * row_pitch as usize;
                writer[dest_base .. dest_base + row.len()].copy_from_slice(row);
            }

            device.device.release_mapping_writer(writer);
        }

        let mut cmd_buffer: gfx::command::CommandBuffer<_, _, command::OneShot> =
            self.command_pool.acquire_command_buffer(false);

        let mut fence = device.device.create_fence(false);

        if conversion_needed {
            // compute pass that does conversion. whey.
            println!("Conversion! Wheeeey...");
        } else {
            let submission = {
                // TODO cubemaps? Arrays?? DEEEEPTH?!?!?
                let image_barrier = memory::Barrier::Image {
                    states: (image::Access::empty(), image::Layout::Undefined)
                        ..(
                            image::Access::TRANSFER_WRITE,
                            image::Layout::TransferDstOptimal,
                        ),
                    target: self.storage[image].0.raw(),
                    range: image::SubresourceRange {
                        aspects: gfx::format::Aspects::COLOR,
                        levels: 0..1,
                        layers: 0..1,
                    },
                };

                cmd_buffer.pipeline_barrier(
                    gfx::pso::PipelineStage::TOP_OF_PIPE..gfx::pso::PipelineStage::TRANSFER,
                    memory::Dependencies::empty(),
                    &[image_barrier],
                );

                cmd_buffer.copy_buffer_to_image(
                    staging_buffer.raw(),
                    self.storage[image].0.raw(),
                    image::Layout::TransferDstOptimal,
                    &[command::BufferImageCopy {
                        buffer_offset: 0,
                        buffer_width: row_pitch / (texel_size as u32),
                        buffer_height: image_height,
                        image_layers: image::SubresourceLayers {
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
                            width: image_width,
                            height: image_height,
                            depth: 1,
                        },
                    }],
                );

                let image_barrier = memory::Barrier::Image {
                    states: (
                        image::Access::TRANSFER_WRITE,
                        image::Layout::TransferDstOptimal,
                    )
                        ..(
                            image::Access::SHADER_READ,
                            image::Layout::ShaderReadOnlyOptimal,
                        ),
                    target: self.storage[image].0.raw(),
                    range: image::SubresourceRange {
                        aspects: gfx::format::Aspects::COLOR,
                        levels: 0..1,
                        layers: 0..1,
                    },
                };

                cmd_buffer.pipeline_barrier(
                    gfx::pso::PipelineStage::TRANSFER..gfx::pso::PipelineStage::FRAGMENT_SHADER,
                    memory::Dependencies::empty(),
                    &[image_barrier],
                );

                cmd_buffer.finish()
            };
            let submission = gfx::Submission::new().submit(Some(submission));

            {
                let mut queue_group = device.queue_group();
                queue_group.queues[QueueType::ImageStorage as usize]
                    .submit(submission, Some(&mut fence));
            }
            device.device.wait_for_fence(&fence, !0);
        };

        device.device.destroy_fence(fence);

        device
            .allocator()
            .destroy_buffer(&device.device, staging_buffer);

        Ok(())
    }

    pub fn dimension(&self, image: ImageHandle) -> Option<ImageDimension> {
        if !self.storage.is_alive(image) {
            None
        } else {
            Some(self.dimensions[image.id()])
        }
    }

    pub fn raw(&self, image: ImageHandle) -> Option<&(Image, ImageView)> {
        if self.storage.is_alive(image) {
            Some(&self.storage[image])
        } else {
            None
        }
    }

    pub fn destroy(&mut self, device: &DeviceContext, handle: ImageHandle) -> bool {
        match self.storage.remove(handle) {
            None => false,
            Some((image, view)) => {
                device.device.destroy_image_view(view);
                device.allocator().destroy_image(&device.device, image);
                true
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

    let row_alignment_mask = row_align as u32 - 1;
    let row_pitch = (width * texel_size as u32 + row_alignment_mask) & !row_alignment_mask;
    let buffer_size = (height * row_pitch) as u64;

    (buffer_size, row_pitch, texel_size)
}
