use storage::{Handle, Storage};

use gfx;
use gfx::Device;

use device::DeviceContext;

pub enum BlendMode {
    Alpha,
    Add,
    Mul,
    Sub,
}

pub struct RenderPass;

pub struct RenderPassCreateAttachment {}

pub struct RenderPassCreateInfo<'a> {
    pub attachments: &'a [RenderPassCreateAttachment],
    pub subpasses: &'a [()],
}

pub struct RenderPassStorage {
    storage: Storage<RenderPass>,
}

impl RenderPassStorage {
    pub fn new() -> Self {
        RenderPassStorage {
            storage: Storage::new(),
        }
    }

    pub fn create(&mut self, device: &DeviceContext, create_infos: &[RenderPassCreateInfo]) {
        device.device.create_render_pass(&[], &[], &[]);
    }
}
