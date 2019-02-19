/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use super::ResourceName;

use crate::image;

use self::ResourceReadType as R;
use self::ResourceWriteType as W;

#[derive(Hash, Debug, Clone, Copy)]
pub enum ResourceType {
    Image,
    Buffer,
    Virtual,
}

#[derive(Hash, Debug, Clone)]
pub(crate) enum ResourceCreateInfo {
    Image(ImageInfo),
    Buffer(BufferCreateInfo),
    Virtual,
}

#[derive(Hash, Default)]
pub struct GraphBuilder {
    pub(crate) enabled: bool,

    /// Mapping from names to resource create information
    pub(crate) resource_creates: Vec<(ResourceName, ResourceCreateInfo)>,
    /// Mapping from new name to src name
    pub(crate) resource_copies: Vec<(ResourceName, ResourceName)>,
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

impl GraphBuilder {
    pub(crate) fn new() -> Self {
        Default::default()
    }

    // image

    pub fn image_create<T: Into<ResourceName>>(&mut self, name: T, create_info: ImageCreateInfo) {
        self.resource_creates.push((
            name.into(),
            ResourceCreateInfo::Image(ImageInfo::Create(create_info)),
        ));
    }

    pub fn image_backbuffer_create<BN, LN>(
        &mut self,
        backbuffer_name: BN,
        local_name: LN,
        create_info: ImageCreateInfo,
        usage: crate::image::ImageUsage,
    ) where
        BN: Into<ResourceName>,
        LN: Into<ResourceName>,
    {
        let bname = backbuffer_name.into();
        let lname = local_name.into();

        self.resource_creates.push((
            lname.clone(),
            ResourceCreateInfo::Image(ImageInfo::BackbufferCreate(
                bname.clone(),
                create_info,
                usage.into(),
            )),
        ));

        self.resource_backbuffer.push((bname, lname));
    }

    pub fn image_backbuffer_get<BN, LN>(&mut self, backbuffer_name: BN, local_name: LN)
    where
        BN: Into<ResourceName>,
        LN: Into<ResourceName>,
    {
        self.resource_creates.push((
            local_name.into(),
            ResourceCreateInfo::Image(ImageInfo::BackbufferRead(backbuffer_name.into())),
        ));
    }

    pub fn image_copy<T0: Into<ResourceName>, T1: Into<ResourceName>>(&mut self, src: T0, new: T1) {
        self.resource_copies.push((new.into(), src.into()));
    }

    pub fn image_move<T0: Into<ResourceName>, T1: Into<ResourceName>>(&mut self, from: T0, to: T1) {
        self.resource_moves.push((to.into(), from.into()));
    }

    pub fn image_write_color<T: Into<ResourceName>>(&mut self, name: T, binding: u8) {
        self.resource_writes
            .push((name.into(), W::Image(ImageWriteType::Color), binding));
    }

    pub fn image_write_depth_stencil<T: Into<ResourceName>>(&mut self, name: T) {
        self.resource_writes.push((
            name.into(),
            W::Image(ImageWriteType::DepthStencil),
            u8::max_value(),
        ));
    }

    pub fn image_write_storage<T: Into<ResourceName>>(&mut self, name: T, binding: u8) {
        self.resource_writes
            .push((name.into(), W::Image(ImageWriteType::Storage), binding));
    }

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

    pub fn image_read_depth_stencil<T: Into<ResourceName>>(&mut self, name: T) {
        self.resource_reads.push((
            name.into(),
            R::Image(ImageReadType::DepthStencil),
            u8::max_value(),
            None,
        ));
    }

    pub fn image_read_storage<T: Into<ResourceName>>(&mut self, name: T, binding: u8) {
        self.resource_reads
            .push((name.into(), R::Image(ImageReadType::Storage), binding, None));
    }

    // buffer

    pub fn buffer_create<T: Into<ResourceName>>(&mut self, name: T, create_info: BufferCreateInfo) {
        self.resource_creates
            .push((name.into(), ResourceCreateInfo::Buffer(create_info)));
    }
    pub fn buffer_copy<T0: Into<ResourceName>, T1: Into<ResourceName>>(
        &mut self,
        src: T0,
        new: T1,
    ) {
        self.resource_copies.push((new.into(), src.into()));
    }
    pub fn buffer_move<T0: Into<ResourceName>, T1: Into<ResourceName>>(
        &mut self,
        from: T0,
        to: T1,
    ) {
        self.resource_moves.push((to.into(), from.into()));
    }

    pub fn buffer_write_storage<T: Into<ResourceName>>(&mut self, name: T, binding: u8) {
        self.resource_writes
            .push((name.into(), W::Buffer(BufferWriteType::Storage), binding));
    }
    pub fn buffer_write_storage_texel<T: Into<ResourceName>>(&mut self, name: T, binding: u8) {
        self.resource_writes.push((
            name.into(),
            W::Buffer(BufferWriteType::StorageTexel),
            binding,
        ));
    }

    pub fn buffer_read_storage<T: Into<ResourceName>>(&mut self, name: T, binding: u8) {
        self.resource_reads.push((
            name.into(),
            R::Buffer(BufferReadType::Storage),
            binding,
            None,
        ));
    }
    pub fn buffer_read_storage_texel<T: Into<ResourceName>>(&mut self, name: T, binding: u8) {
        self.resource_reads.push((
            name.into(),
            R::Buffer(BufferReadType::StorageTexel),
            binding,
            None,
        ));
    }

    // extern

    pub fn virtual_create<T: Into<ResourceName>>(&mut self, name: T) {
        self.resource_creates
            .push((name.into(), ResourceCreateInfo::Virtual));
    }

    pub fn virtual_move<T0: Into<ResourceName>, T1: Into<ResourceName>>(
        &mut self,
        from: T0,
        to: T1,
    ) {
        self.resource_moves.push((to.into(), from.into()));
    }

    pub fn virtual_read<T: Into<ResourceName>>(&mut self, name: T) {
        self.resource_reads.push((name.into(), R::Virtual, 0, None));
    }

    // control flow control

    pub fn enable(&mut self) {
        self.enabled = true;
    }
}

pub type DepthValue = f32;
pub type StencilValue = u32;

#[derive(Debug, Clone, Copy)]
pub enum ImageClearValue {
    Color([f32; 4]),
    DepthStencil(DepthValue, StencilValue),
}

#[derive(Debug, Clone, Hash)]
pub(crate) enum ImageInfo {
    Create(ImageCreateInfo),
    BackbufferRead(ResourceName),
    BackbufferCreate(ResourceName, ImageCreateInfo, gfx::image::Usage),
}

#[derive(Debug, Clone, Hash)]
pub struct ImageCreateInfo {
    pub format: image::ImageFormat,
    pub size_mode: image::ImageSizeMode,
}

#[derive(Debug, Clone, Hash)]
pub struct BufferCreateInfo {
    pub size: u64,
    pub storage: BufferStorageType,
}

#[derive(Debug, Clone, Hash)]
pub enum BufferStorageType {
    HostVisible,
    DeviceLocal,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub enum ResourceReadType {
    Image(ImageReadType),
    Buffer(BufferReadType),
    Virtual,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub enum ImageReadType {
    Color,
    Storage,
    DepthStencil,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub enum BufferReadType {
    Storage,
    StorageTexel,
    Uniform,
    UniformTexel,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub enum ResourceWriteType {
    Image(ImageWriteType),
    Buffer(BufferWriteType),
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub enum ImageWriteType {
    Color,
    DepthStencil,
    Storage,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub enum BufferWriteType {
    Storage,
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

impl From<ResourceCreateInfo> for ResourceType {
    fn from(inf: ResourceCreateInfo) -> Self {
        match inf {
            ResourceCreateInfo::Image(..) => ResourceType::Image,
            ResourceCreateInfo::Buffer(..) => ResourceType::Buffer,
            ResourceCreateInfo::Virtual => ResourceType::Virtual,
        }
    }
}
