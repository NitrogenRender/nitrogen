/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::device::DeviceContext;
use crate::types;
use crate::util::pool::{Pool, PoolElem, PoolImpl};

use gfx::command::Primary;
use gfx::command::Shot;
use gfx::queue::capability::Capability;

pub(crate) type CmdBufType<C> =
    gfx::command::CommandBuffer<back::Backend, C, gfx::command::OneShot, Primary>;
pub(crate) type CommandBuffer<'a, C> = PoolElem<'a, CommandPoolImpl<C>, CmdBufType<C>>;

pub(crate) struct CommandPoolImpl<T: gfx::queue::capability::Capability> {
    pub(crate) pool: gfx::pool::CommandPool<back::Backend, T>,
}

impl<C> PoolImpl<CmdBufType<C>> for CommandPoolImpl<C>
where
    C: gfx::queue::capability::Capability,
{
    fn new_elem(&mut self) -> CmdBufType<C> {
        println!("new");

        let mut buf = self.pool.acquire_command_buffer::<gfx::command::OneShot>();
        unsafe {
            buf.begin();
        }
        buf
    }

    fn reset_elem(&mut self, elem: &mut CmdBufType<C>) {
        println!("reset/begin()");
        unsafe {
            elem.begin();
        }
    }

    fn free_elem(&mut self, elem: CmdBufType<C>) {
        unsafe {
            self.pool.free(Some(elem));
        }
    }

    fn free_on_drop() -> bool {
        false
    }
}

pub(crate) struct CommandPool<C: Capability>(pub(crate) Pool<CmdBufType<C>, CommandPoolImpl<C>>);

pub(crate) type CommandPoolGraphics = CommandPool<gfx::Graphics>;
pub(crate) type CommandPoolCompute = CommandPool<gfx::Compute>;
pub(crate) type CommandPoolTransfer = CommandPool<gfx::Transfer>;

impl<C: Capability> CommandPool<C> {
    pub(crate) fn new(pool: gfx::pool::CommandPool<back::Backend, C>) -> Self {
        CommandPool(Pool::new(CommandPoolImpl { pool }))
    }

    pub(crate) unsafe fn alloc(&self) -> CommandBuffer<'_, C> {
        self.0.alloc()
    }

    pub(crate) unsafe fn reset(&mut self) {
        // reset the command pool instead of resetting individual elements
        self.0.impl_ref_mut().pool.reset();
        self.clear();
    }

    pub(crate) unsafe fn clear(&mut self) {
        self.0.clear()
    }
}
