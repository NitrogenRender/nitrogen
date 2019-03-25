/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Functionality for the setup-phase of passes.

use crate::graph::ResourceName;

use crate::image;

use self::ResourceReadType as R;
use self::ResourceWriteType as W;

/// Resource types that can be used in graphs and passes.
#[derive(Hash, Debug, Clone, Copy, Eq, PartialEq)]
pub enum ResourceType {
    /// An image resource
    Image,
    /// A buffer resource
    Buffer,
    /// A virtual resource
    Virtual,
}

#[derive(Hash, Debug, Clone)]
pub(crate) enum ResourceCreateInfo {
    Image(ImageInfo),
    Buffer(BufferCreateInfo),
    Virtual,
}

/// Type used to record which resources are used in what ways.
#[derive(Hash, Default)]
pub struct ResourceDescriptor {
    /// Mapping from names to resource create information
    pub(crate) resource_creates: Vec<(ResourceName, ResourceCreateInfo)>,
    /// Mapping from new name to src name
    pub(crate) resource_moves: Vec<(ResourceName, ResourceName)>,

    /// List of resources that will be read from. Also contains read type and binding
    ///
    /// The last binding is for samplers.
    pub(crate) resource_reads: Vec<(ResourceName, ResourceReadType, u8, Option<u8>)>,
    /// List of resources that will be written to. Also contains write type and binding
    pub(crate) resource_writes: Vec<(ResourceName, ResourceWriteType, u8)>,

    /// List of resources that persist executions (backbuffername, localname)
    pub(crate) resource_backbuffer: Vec<(ResourceName, ResourceName)>,
}

impl ResourceDescriptor {
    pub(crate) fn new() -> Self {
        Default::default()
    }

    /// Create a new image resource.
    pub fn image_create<T: Into<ResourceName>>(&mut self, name: T, create_info: ImageCreateInfo) {
        self.resource_creates.push((
            name.into(),
            ResourceCreateInfo::Image(ImageInfo::Create(create_info)),
        ));
    }

    /// Read an image resource from the backbuffer and give it a graph-local name.
    pub fn image_backbuffer_get<BN, LN, F>(
        &mut self,
        backbuffer_name: BN,
        local_name: LN,
        format: F,
    ) where
        BN: Into<ResourceName>,
        LN: Into<ResourceName>,
        F: Into<gfx::format::Format>,
    {
        self.resource_creates.push((
            local_name.into(),
            ResourceCreateInfo::Image(ImageInfo::BackbufferRead {
                name: backbuffer_name.into(),
                format: format.into(),
            }),
        ));
    }

    /// State the dependence on an image resource that will be moved to a new name.
    pub fn image_move<T0: Into<ResourceName>, T1: Into<ResourceName>>(&mut self, from: T0, to: T1) {
        self.resource_moves.push((to.into(), from.into()));
    }

    /// State the dependence on a color image that will be used as a color attachment of the
    /// framebuffer.
    pub fn image_write_color<T: Into<ResourceName>>(&mut self, name: T, binding: u8) {
        self.resource_writes
            .push((name.into(), W::Image(ImageWriteType::Color), binding));
    }

    /// State the dependence on a depth-stencil image used for reading or writing as a framebuffer
    /// attachment.
    pub fn image_write_depth_stencil<T: Into<ResourceName>>(&mut self, name: T) {
        self.resource_writes.push((
            name.into(),
            W::Image(ImageWriteType::DepthStencil),
            u8::max_value(),
        ));
    }

    /// State the dependence on a storage image used for reading or writing.
    pub fn image_write_storage<T: Into<ResourceName>>(&mut self, name: T, binding: u8) {
        self.resource_writes
            .push((name.into(), W::Image(ImageWriteType::Storage), binding));
    }

    /// State the dependence on a color image used for reading.
    pub fn image_read_color<T: Into<ResourceName>>(
        &mut self,
        name: T,
        binding: u8,
        sampler_binding: Option<u8>,
    ) {
        self.resource_reads.push((
            name.into(),
            R::Image(ImageReadType::Color),
            binding,
            sampler_binding,
        ));
    }

    /// State the dependence on a depth-stencil image used for reading as a framebuffer attachment.
    pub fn image_read_depth_stencil<T: Into<ResourceName>>(&mut self, name: T) {
        self.resource_reads.push((
            name.into(),
            R::Image(ImageReadType::DepthStencil),
            u8::max_value(),
            None,
        ));
    }

    /// State the dependence on a storage-image used for reading.
    pub fn image_read_storage<T: Into<ResourceName>>(&mut self, name: T, binding: u8) {
        self.resource_reads
            .push((name.into(), R::Image(ImageReadType::Storage), binding, None));
    }

    /// Create a new buffer resource.
    pub fn buffer_create<T: Into<ResourceName>>(&mut self, name: T, create_info: BufferCreateInfo) {
        self.resource_creates
            .push((name.into(), ResourceCreateInfo::Buffer(create_info)));
    }

    /// State the dependence on a buffer resource that will be moved to a new name.
    pub fn buffer_move<T0: Into<ResourceName>, T1: Into<ResourceName>>(
        &mut self,
        from: T0,
        to: T1,
    ) {
        self.resource_moves.push((to.into(), from.into()));
    }

    /// State the dependence on a storage buffer that will be used for reading or writing.
    pub fn buffer_write_storage<T: Into<ResourceName>>(&mut self, name: T, binding: u8) {
        self.resource_writes
            .push((name.into(), W::Buffer(BufferWriteType::Storage), binding));
    }

    /// State the dependence on a storage-texel buffer that will be used for reading or writing.
    pub fn buffer_write_storage_texel<T: Into<ResourceName>>(&mut self, name: T, binding: u8) {
        self.resource_writes.push((
            name.into(),
            W::Buffer(BufferWriteType::StorageTexel),
            binding,
        ));
    }

    /// State the dependence on a storage buffer that will be used for reading.
    pub fn buffer_read_storage<T: Into<ResourceName>>(&mut self, name: T, binding: u8) {
        self.resource_reads.push((
            name.into(),
            R::Buffer(BufferReadType::Storage),
            binding,
            None,
        ));
    }

    /// State the dependence on a storage-texel buffer that will be used for reading.
    pub fn buffer_read_storage_texel<T: Into<ResourceName>>(&mut self, name: T, binding: u8) {
        self.resource_reads.push((
            name.into(),
            R::Buffer(BufferReadType::StorageTexel),
            binding,
            None,
        ));
    }

    /// Create a new "virtual resource". Virtual resources do not contain any data, nor do they
    /// have a runtime representation.
    /// They are only used to explicitly state a dependence relationship between passes.
    ///
    /// This is needed when a pass modifies a graph-untracked resource.
    pub fn virtual_create<T: Into<ResourceName>>(&mut self, name: T) {
        self.resource_creates
            .push((name.into(), ResourceCreateInfo::Virtual));
    }

    /// State the dependence on a "virtual" resource that will be moved to a new name.
    pub fn virtual_move<T0: Into<ResourceName>, T1: Into<ResourceName>>(
        &mut self,
        from: T0,
        to: T1,
    ) {
        self.resource_moves.push((to.into(), from.into()));
    }

    /// State the dependence on a "virtual" resource.
    pub fn virtual_read<T: Into<ResourceName>>(&mut self, name: T) {
        self.resource_reads.push((name.into(), R::Virtual, 0, None));
    }
}

/// Value type used for depth data.
pub type DepthValue = f32;
/// Value type used for stencil data.
pub type StencilValue = u32;

/// Values an image can be cleared with.
#[derive(Debug, Clone, Copy)]
pub enum ImageClearValue {
    /// Float values used for color/storage images.
    Color([f32; 4]),

    /// Depth and stencil values for depth/+stencil images.
    DepthStencil(DepthValue, StencilValue),
}

#[derive(Debug, Clone, Hash)]
pub(crate) enum ImageInfo {
    Create(ImageCreateInfo),
    BackbufferRead {
        name: ResourceName,
        format: gfx::format::Format,
    },
}

/// Information needed to create an image resource
#[derive(Debug, Clone, Hash)]
pub struct ImageCreateInfo {
    /// Image format used.
    pub format: image::ImageFormat,
    /// Size mode used to determine the dimensions of the image.
    pub size_mode: image::ImageSizeMode,
}

/// Information needed to create a buffer resource.
#[derive(Debug, Clone, Hash)]
pub struct BufferCreateInfo {
    /// Size of the buffer in bytes.
    pub size: u64,
    /// Storage type of the buffer memory.
    pub storage: BufferStorageType,
}

/// Types of memory that a buffer can be backed by.
#[derive(Debug, Clone, Hash)]
pub enum BufferStorageType {
    /// Memory visible to the CPU - slower to access but easier to update.
    HostVisible,

    /// Memory local to the device - faster to access but can't be updated directly.
    DeviceLocal,
}

/// Ways a resource can be used with read-access.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub enum ResourceReadType {
    Image(ImageReadType),
    Buffer(BufferReadType),
    Virtual,
}

/// Ways an image can be used with read-access.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub enum ImageReadType {
    Color,
    Storage,
    DepthStencil,
}

/// Ways a buffer can be used with read-access.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub enum BufferReadType {
    Storage,
    StorageTexel,
    Uniform,
    UniformTexel,
}

/// Ways a resource can be used with write-access.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub enum ResourceWriteType {
    Image(ImageWriteType),
    Buffer(BufferWriteType),
}

/// Ways an image can be used when writing to it.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub enum ImageWriteType {
    /// A color attachment of a Framebuffer
    Color,

    /// A depth-stencil attachment of a Framebuffer
    DepthStencil,

    /// A storage descriptor used for reading or writing.
    Storage,
}

/// Ways a buffer can be used when writing to it
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub enum BufferWriteType {
    /// A buffer used for reading or writing.
    Storage,

    /// A buffer used for reading or writing backed by a texture.
    StorageTexel,
}

impl From<ResourceWriteType> for ResourceType {
    fn from(ty: ResourceWriteType) -> Self {
        match ty {
            ResourceWriteType::Image(..) => ResourceType::Image,
            ResourceWriteType::Buffer(..) => ResourceType::Buffer,
        }
    }
}

impl From<ResourceReadType> for ResourceType {
    fn from(ty: ResourceReadType) -> Self {
        match ty {
            ResourceReadType::Image(..) => ResourceType::Image,
            ResourceReadType::Buffer(..) => ResourceType::Buffer,
            ResourceReadType::Virtual => ResourceType::Virtual,
        }
    }
}

impl From<&ResourceCreateInfo> for ResourceType {
    fn from(inf: &ResourceCreateInfo) -> Self {
        match inf {
            ResourceCreateInfo::Image(..) => ResourceType::Image,
            ResourceCreateInfo::Buffer(..) => ResourceType::Buffer,
            ResourceCreateInfo::Virtual => ResourceType::Virtual,
        }
    }
}
