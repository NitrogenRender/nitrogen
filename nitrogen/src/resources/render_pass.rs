use storage::{Handle, Storage};

use gfx;
use gfx::Device;

use image;

use std::ops::Range;

use smallvec::{SmallVec, smallvec};

use device::DeviceContext;

pub enum BlendMode {
    Alpha,
    Add,
    Mul,
    Sub,
}

pub enum RenderPassError {
    OutOfMemory(gfx::device::OutOfMemory),
}

impl From<gfx::device::OutOfMemory> for RenderPassError {
    fn from(err: gfx::device::OutOfMemory) -> Self {
        RenderPassError::OutOfMemory(err)
    }
}

pub type Result<T> = ::std::result::Result<T, RenderPassError>;

pub struct RenderPass {
    render_pass: ::types::RenderPass,
}

pub type RenderPassHandle = Handle<RenderPass>;

pub struct RenderPassCreateInfo<'a> {
    pub attachments: &'a [gfx::pass::Attachment],
    pub subpasses: &'a [gfx::pass::SubpassDesc<'a>],
    pub dependencies: &'a [gfx::pass::SubpassDependency],
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

    pub fn create(&mut self, device: &DeviceContext, create_infos: &[RenderPassCreateInfo]) -> SmallVec<[Result<RenderPassHandle>; 16]> {

        create_infos.iter()
            .map(|create_info| {

                let pass = device.device.create_render_pass(
                    create_info.attachments,
                    create_info.subpasses,
                    create_info.dependencies,
                );

                match pass {
                    Ok(render_pass) => {
                        let handle = self.storage.insert(RenderPass {
                            render_pass,
                        }).0;

                        Ok(handle)
                    },
                    Err(e) => {
                        Err(e.into())
                    }
                }
            })
            .collect()
    }

    pub fn raw(&self, handle: RenderPassHandle) -> Option<&::types::RenderPass> {
        if self.storage.is_alive(handle) {
            Some(&self.storage[handle].render_pass)
        } else {
            None
        }
    }
}
