use super::ResourceName;

use image;

use self::ResourceReadType as R;
use self::ResourceWriteType as W;

#[derive(Hash, Debug, Clone, Copy)]
pub enum ResourceType {
    Image,
    Buffer,
}

#[derive(Hash, Debug, Clone)]
pub(crate) enum ResourceCreateInfo {
    Image(ImageCreateInfo),
    Buffer(BufferCreateInfo),
}

#[derive(Hash, Default)]
pub struct GraphBuilder {
    pub(crate) enabled: bool,

    // image data
    /// Mapping from names to resource create information
    pub(crate) resource_creates: Vec<(ResourceName, ResourceCreateInfo)>,
    /// Mapping from new name to src name
    pub(crate) resource_copies: Vec<(ResourceName, ResourceName)>,
    /// Mapping from new name to src name
    pub(crate) resource_moves: Vec<(ResourceName, ResourceName)>,

    /// List of resources that will be read from. Also contains read type and binding
    pub(crate) resource_reads: Vec<(ResourceName, ResourceReadType, u8)>,
    /// List of resources that will be written to. Also contains write type and binding
    pub(crate) resource_writes: Vec<(ResourceName, ResourceWriteType, u8)>,

    /// List of external resources that will be read
    pub(crate) resource_ext_reads: Vec<(ResourceReadType, u8)>,

    /// List of resources that persist executions
    pub(crate) resource_backbuffer: Vec<ResourceName>,
}

impl GraphBuilder {
    pub(crate) fn new() -> Self {
        Default::default()
    }

    // image

    pub fn image_create<T: Into<ResourceName>>(&mut self, name: T, create_info: ImageCreateInfo) {
        self.resource_creates
            .push((name.into(), ResourceCreateInfo::Image(create_info)));
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
    pub fn image_write_depth_stencil<T: Into<ResourceName>>(&mut self, name: T, binding: u8) {
        self.resource_writes
            .push((name.into(), W::Image(ImageWriteType::DepthStencil), binding));
    }
    pub fn image_write_storage<T: Into<ResourceName>>(&mut self, name: T, binding: u8) {
        self.resource_writes
            .push((name.into(), W::Image(ImageWriteType::Storage), binding));
    }

    pub fn image_read_color<T: Into<ResourceName>>(&mut self, name: T, binding: u8) {
        self.resource_reads
            .push((name.into(), R::Image(ImageReadType::Color), binding));
    }
    pub fn image_read_storage<T: Into<ResourceName>>(&mut self, name: T, binding: u8) {
        self.resource_reads
            .push((name.into(), R::Image(ImageReadType::Storage), binding));
    }


    pub fn image_ext_read_color(&mut self, binding: u8) {
        self.resource_ext_reads.push((ResourceReadType::Image(ImageReadType::Color), binding));
    }

    pub fn image_ext_read_storage(&mut self, binding: u8) {
        self.resource_ext_reads.push((ResourceReadType::Image(ImageReadType::Storage), binding));
    }


    pub fn image_backbuffer<T: Into<ResourceName>>(&mut self, name: T) {
        self.resource_backbuffer.push(name.into());
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
        self.resource_reads
            .push((name.into(), R::Buffer(BufferReadType::Storage), binding));
    }
    pub fn buffer_read_storage_texel<T: Into<ResourceName>>(&mut self, name: T, binding: u8) {
        self.resource_reads.push((
            name.into(),
            R::Buffer(BufferReadType::StorageTexel),
            binding,
        ));
    }


    pub fn buffer_ext_read_storage(&mut self, binding: u8) {
        self.resource_ext_reads.push((ResourceReadType::Buffer(BufferReadType::Storage), binding));
    }

    pub fn buffer_ext_read_uniform(&mut self, binding: u8) {
        self.resource_ext_reads.push((ResourceReadType::Buffer(BufferReadType::Uniform), binding));
    }


    pub fn buffer_backbuffer<T: Into<ResourceName>>(&mut self, name: T) {
        self.resource_backbuffer.push(name.into());
    }

    // control flow control

    pub fn enable(&mut self) {
        self.enabled = true;
    }
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
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub enum ImageReadType {
    Color,
    Storage,
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
        }
    }
}

impl From<ResourceCreateInfo> for ResourceType {
    fn from(inf: ResourceCreateInfo) -> Self {
        match inf {
            ResourceCreateInfo::Image(..) => ResourceType::Image,
            ResourceCreateInfo::Buffer(..) => ResourceType::Buffer,
        }
    }
}
