/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::storage::{Handle, Storage};

use gfx::Device;

use smallvec::SmallVec;

use crate::device::DeviceContext;
use crate::submit_group::ResourceList;

#[derive(Clone, Copy, Debug)]
pub enum BlendMode {
    Alpha,
    Add,
    Mul,
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

    pub(crate) fn create(
        &mut self,
        device: &DeviceContext,
        create_infos: &[RenderPassCreateInfo],
    ) -> SmallVec<[Result<RenderPassHandle>; 16]> {
        create_infos
            .iter()
            .map(|create_info| {
                let pass = device.device.create_render_pass(
                    create_info.attachments,
                    create_info.subpasses,
                    create_info.dependencies,
                );

                match pass {
                    Ok(render_pass) => {
                        let handle = self.storage.insert(RenderPass { render_pass }).0;

                        Ok(handle)
                    }
                    Err(e) => Err(e.into()),
                }
            })
            .collect()
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
