/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::storage::{Handle, Storage};

use gfx::Device;

use crate::device::DeviceContext;
use crate::submit_group::ResourceList;

#[derive(Clone, From, Display, Debug)]
pub enum RenderPassError {
    #[display(fmt = "Out of memory")]
    OutOfMemory(gfx::device::OutOfMemory),
}

pub struct RenderPass {
    render_pass: crate::types::RenderPass,
}

pub type RenderPassHandle = Handle<RenderPass>;

pub struct RenderPassCreateInfo<'a> {
    pub attachments: &'a [gfx::pass::Attachment],
    pub subpasses: &'a [gfx::pass::SubpassDesc<'a>],
    pub dependencies: &'a [gfx::pass::SubpassDependency],
}

pub(crate) struct RenderPassStorage {
    storage: Storage<RenderPass>,
}

impl RenderPassStorage {
    pub(crate) fn new() -> Self {
        RenderPassStorage {
            storage: Storage::new(),
        }
    }

    pub(crate) unsafe fn create(
        &mut self,
        device: &DeviceContext,
        create_info: RenderPassCreateInfo,
    ) -> Result<RenderPassHandle, RenderPassError> {
        let pass = device.device.create_render_pass(
            create_info.attachments,
            create_info.subpasses,
            create_info.dependencies,
        );

        match pass {
            Ok(render_pass) => {
                let handle = self.storage.insert(RenderPass { render_pass });

                Ok(handle)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub(crate) fn raw(&self, handle: RenderPassHandle) -> Option<&crate::types::RenderPass> {
        if self.storage.is_alive(handle) {
            Some(&self.storage[handle].render_pass)
        } else {
            None
        }
    }

    pub(crate) fn destroy<P>(&mut self, res_list: &mut ResourceList, handles: P)
    where
        P: IntoIterator,
        P::Item: std::borrow::Borrow<RenderPassHandle>,
    {
        use std::borrow::Borrow;

        for handle in handles.into_iter() {
            let handle = *handle.borrow();
            if let Some(render_pass) = self.storage.remove(handle) {
                res_list.queue_render_pass(render_pass.render_pass);
            }
        }
    }
}
