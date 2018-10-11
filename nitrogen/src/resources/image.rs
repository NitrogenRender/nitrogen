use back;
use gfx;
use gfxm;

use gfx::image;
use gfx::memory;
use gfx::command;
use gfx::Device;
use gfxm::Factory;
use gfxm::SmartAllocator;

use std;

use slab::Slab;

use super::super::DeviceContext;
use super::super::QueueType;

#[derive(Copy, Clone, Debug)]
pub enum ImageDimension {
    D1 { x: u32 },
    D2 { x: u32, y: u32 },
    D3 { x: u32, y: u32, z: u32 },
}

type ImageGeneration = u64;
type ImageId = usize;

#[derive(Copy, Clone)]
pub struct ImageHandle(pub ImageId, pub ImageGeneration);

pub struct ImageCreateInfo {
    pub dimension: ImageDimension,
    pub num_layers: u16,
    pub num_samples: u8,
    pub num_mipmaps: u8,
    pub format: ImageFormat,
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

type Image = <SmartAllocator<back::Backend> as Factory<back::Backend>>::Image;

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

pub struct ImageStorage {
    dimensions: Vec<ImageDimension>,
    generations: Vec<ImageGeneration>,
    formats: Vec<gfx::format::Format>,
    images: Slab<Image>,

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
            generations: vec![],
            formats: vec![],
            images: Slab::new(),
            command_pool,
        }
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

        let (entry, handle) = {
            let entry = self.images.vacant_entry();
            let key = entry.key();

            let needs_to_grow_storage = self.generations.len() <= key;

            if needs_to_grow_storage {
                self.generations.push(0);
                self.dimensions.push(create_info.dimension);
                self.formats.push(format);

                debug_assert!(self.generations.len() == key + 1);
                debug_assert!(self.dimensions.len() == key + 1);
                debug_assert!(self.formats.len() == key + 1);
            } else {
                self.generations[key] += 1;
                self.dimensions[key] = create_info.dimension;
                self.formats[key] = format;
            }

            let generation = self.generations[key];

            (entry, ImageHandle(key, generation))
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

            let mut alloc = device
                .memory_allocator
                .lock()
                .expect("Memory allocator not accessible");

            alloc.create_image(
                &device.device,
                (gfxm::Type::General, Properties::DEVICE_LOCAL),
                image_kind,
                0,
                format,
                image::Tiling::Optimal,
                image::Usage::TRANSFER_DST | image::Usage::SAMPLED,
                image::ViewCapabilities::empty(),
            )?
        };

        entry.insert(image);

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

        let image_stride = data.data.len() / (image_width * image_height) as usize;

        if !upload_data_fits {
            return Err(ImageError::UploadDataInvalid);
        }

        let conversion_needed = gfx::format::Format::from(data.format) != self.formats[image.0];

        let (buffer_size, row_pitch) = {

            use gfx::adapter::PhysicalDevice;

            let limits: gfx::Limits = device.adapter.physical_device.limits();

            let row_alignment = limits.min_buffer_copy_pitch_alignment as u32;

            // Because low level graphics are low level, we need to take care about buffer
            // alignment here.
            //
            // For example an RGBA8 image with 11 * 11 dims
            // has
            //  - "stride" of 4 (4 components (rgba) with 1 byte size)
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
            // To compute the row pitch we need to know how much padding to add.
            let row_size = image_width * image_stride as u32;

            // If the row size is already aligned, we would do `align - 0`, in which case we would
            // waste space. So we AND cut away the highest number.
            // So we can only range from 0..align-1
            let row_padding = (row_alignment - (row_size % row_alignment)) & (row_alignment - 1);

            let row_pitch = row_size + row_padding;
            let buffer_size = (image_height * row_pitch) as u64;

            (buffer_size, row_pitch)
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
            for row in 0..image_height {

                let input_row_offset = (row * image_width * image_stride as u32) as usize;
                let input_row_end = input_row_offset + image_width as usize * image_stride;

                let row_data = &data.data[input_row_offset .. input_row_end];

                let dest_offset = (row * row_pitch) as usize;
                let dest_end = dest_offset + row_data.len();

                writer[dest_offset .. dest_end].copy_from_slice(row_data);
            }

            device.device.release_mapping_writer(writer);
        }

        let mut cmd_buffer: gfx::command::CommandBuffer<_, _, command::OneShot> = self.command_pool.acquire_command_buffer(false);

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
                    target: self.images[image.0].raw(),
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
                    self.images[image.0].raw(),
                    image::Layout::TransferDstOptimal,
                    &[
                        command::BufferImageCopy {
                            buffer_offset: 0,
                            buffer_width: row_pitch,
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
                        }
                    ]
                );

                let image_barrier = memory::Barrier::Image {
                    states: (image::Access::TRANSFER_WRITE, image::Layout::TransferDstOptimal)
                        ..(image::Access::SHADER_READ, image::Layout::ShaderReadOnlyOptimal),
                    target: self.images[image.0].raw(),
                    range: image::SubresourceRange {
                        aspects: gfx::format::Aspects::COLOR,
                        levels: 0..1,
                        layers: 0..1,
                    },
                };

                cmd_buffer.pipeline_barrier(
                    gfx::pso::PipelineStage::TRANSFER..gfx::pso::PipelineStage::FRAGMENT_SHADER,
                    memory::Dependencies::empty(),
                    &[image_barrier]
                );

                cmd_buffer.finish()
            };
            let submission = gfx::Submission::new().submit(Some(submission));

            {
                device.queue_group().queues[QueueType::ImageStorage as usize].submit(submission, Some(&mut fence));
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
        if !self.is_alive(image) {
            None
        } else {
            Some(self.dimensions[image.0])
        }
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

    pub fn destroy(&mut self, device: &DeviceContext, image: ImageHandle) -> bool {
        if self.is_alive(image) {
            let image = self.images.remove(image.0);
            device.allocator().destroy_image(&device.device, image);
            true
        } else {
            false
        }
    }
}
