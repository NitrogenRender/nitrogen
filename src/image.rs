use back;
use gfx;
use gfxm;

use gfx::image;
use gfx::Device;
use gfxm::Factory;
use gfxm::SmartAllocator;

use std;

use slab::Slab;

use super::DeviceContext;

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
    pub format: ImageFormat,
}

pub struct ImageUploadInfo<'a> {
    pub data: &'a [u8],
    pub format: ImageFormat,
    pub dimension: ImageDimension,
    pub target_offset: (u32, u32, u32),
}

#[repr(C)]
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
        let command_pool = device.device.create_command_pool_typed(
            &device.queue_group,
            gfx::pool::CommandPoolCreateFlags::empty(),
            16,
        );

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

        let image_dimensions = self.get_dimension(image).ok_or(ImageError::HandleInvalid)?;

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

        if !upload_data_fits {
            return Err(ImageError::UploadDataInvalid);
        }

        let size = data.data.len() as u64;

        let staging_buffer = {
            use gfx::memory::Properties;

            let mut alloc = device.allocator();

            alloc.create_buffer(
                &device.device,
                (
                    gfxm::Type::ShortLived,
                    Properties::CPU_VISIBLE | Properties::COHERENT,
                ),
                size,
                gfx::buffer::Usage::TRANSFER_SRC | gfx::buffer::Usage::TRANSFER_DST,
            )?
        };

        {
            use gfxm::Block;

            let mut writer = device
                .device
                .acquire_mapping_writer(staging_buffer.memory(), 0..size)?;
            writer[0..(size as usize)].copy_from_slice(data.data);
            device.device.release_mapping_writer(writer);
        }

        let conversion_needed = gfx::format::Format::from(data.format) != self.formats[image.0];

        if conversion_needed {
            // compute pass that does conversion. whey.

        } else {
            let mut cmd_buffer: gfx::command::CommandBuffer<_, _, gfx::command::OneShot> = self.command_pool.acquire_command_buffer(false);

            let image_barrier = gfx::memory::Barrier::Image {
                states: (gfx::image::Access::empty(), gfx::image::Layout::Undefined)
                    ..(
                        gfx::image::Access::TRANSFER_WRITE,
                        gfx::image::Layout::TransferDstOptimal,
                    ),
                target: self.images[image.0].raw(),
                range: gfx::image::SubresourceRange {
                    aspects: gfx::format::Aspects::COLOR,
                    levels: 0..1,
                    layers: 0..1,
                },
            };

            cmd_buffer.pipeline_barrier(
                gfx::pso::PipelineStage::TOP_OF_PIPE..gfx::pso::PipelineStage::TRANSFER,
                gfx::memory::Dependencies::empty(),
                &[image_barrier],
            );

            cmd_buffer.finish();
        }

        device
            .allocator()
            .destroy_buffer(&device.device, staging_buffer);

        Ok(())
    }

    pub fn get_dimension(&self, image: ImageHandle) -> Option<ImageDimension> {
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
