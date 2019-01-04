/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use std::ops::Range;

use gfx;
use gfx::command;

use crate::types;

use crate::material::{MaterialInstanceHandle, MaterialStorage};

use crate::buffer::{BufferHandle, BufferStorage};

pub enum IndexType {
    U16,
    U32,
}

#[derive(Clone)]
pub(crate) struct ReadStorages<'a> {
    pub(crate) buffer: &'a BufferStorage,
    pub(crate) material: &'a MaterialStorage,
}

pub struct GraphicsCommandBuffer<'a> {
    pub(crate) encoder: gfx::command::RenderPassInlineEncoder<'a, back::Backend>,
    pub(crate) storages: &'a ReadStorages<'a>,

    pub(crate) pipeline_layout: &'a types::PipelineLayout,
}

impl<'a> GraphicsCommandBuffer<'a> {
    pub unsafe fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) {
        self.encoder.draw(vertices, instances);
    }

    pub unsafe fn draw_indexed(
        &mut self,
        indices: Range<u32>,
        base_vertex: i32,
        instances: Range<u32>,
    ) {
        self.encoder.draw_indexed(indices, base_vertex, instances);
    }

    /// Bind vertex buffers for the next draw call.
    /// The provided pairs of buffer and `usize` represent the buffer to bind
    /// and the **offset into the buffer**.
    /// The first pair will be bound to vertex buffer 0, the second to 1, etc...
    pub unsafe fn bind_vertex_buffers<T, I>(&mut self, buffers: T)
    where
        T: IntoIterator<Item = I>,
        T::Item: std::borrow::Borrow<(BufferHandle, usize)>,
    {
        let stores = self.storages.clone();

        let bufs = buffers.into_iter().filter_map(|i| {
            let (buffer, index) = i.borrow();
            stores
                .buffer
                .raw(*buffer)
                .map(|buf| (buf.buffer.raw(), *index as u64))
        });

        self.encoder.bind_vertex_buffers(0, bufs);
    }

    pub unsafe fn bind_index_buffer(
        &mut self,
        buffer: BufferHandle,
        offset: u64,
        index_type: IndexType,
    ) {
        let stores = self.storages.clone();

        let buffer_raw = stores.buffer.raw(buffer).map(|buf| buf.buffer.raw());

        let buffer_raw = match buffer_raw {
            Some(val) => val,
            None => return,
        };

        self.encoder
            .bind_index_buffer(gfx::buffer::IndexBufferView {
                buffer: buffer_raw,
                offset,
                index_type: match index_type {
                    IndexType::U16 => gfx::IndexType::U16,
                    IndexType::U32 => gfx::IndexType::U32,
                },
            });
    }

    pub unsafe fn bind_material(
        &mut self,
        binding: usize,
        material: MaterialInstanceHandle,
    ) -> Option<()> {
        let layout = self.pipeline_layout;

        let mat = self.storages.material.raw(material.0)?;
        let instance = mat.intance_raw(material.1)?;

        let set = &instance.set;

        self.encoder
            .bind_graphics_descriptor_sets(layout, binding, Some(set), &[]);

        Some(())
    }

    pub unsafe fn push_constant_raw(&mut self, offset: u32, data: &[u32]) {
        self.encoder.push_graphics_constants(
            self.pipeline_layout,
            gfx::pso::ShaderStageFlags::ALL,
            offset,
            data,
        )
    }

    pub unsafe fn push_constant<T: Sized>(&mut self, offset: u32, data: T) {
        use smallvec::SmallVec;
        let mut buf = SmallVec::<[u8; 1024]>::new();

        buf.set_len(1024);

        {
            let u32_slice = data_to_u32_slice(data, &mut buf[..]);
            self.push_constant_raw(offset, u32_slice);
        }
    }
}

pub struct ComputeCommandBuffer<'a> {
    pub(crate) buf: &'a mut crate::resources::command_pool::CmdBufType<gfx::Compute>,
    pub(crate) storages: &'a ReadStorages<'a>,

    pub(crate) pipeline_layout: &'a types::PipelineLayout,
}

impl<'a> ComputeCommandBuffer<'a> {
    pub unsafe fn dispatch(&mut self, workgroup_count: [u32; 3]) {
        self.buf.dispatch(workgroup_count)
    }

    pub unsafe fn bind_material(
        &mut self,
        binding: usize,
        material: MaterialInstanceHandle,
    ) -> Option<()> {
        let layout = self.pipeline_layout;

        let mat = self.storages.material.raw(material.0)?;
        let instance = mat.intance_raw(material.1)?;

        let set = &instance.set;

        self.buf
            .bind_compute_descriptor_sets(layout, binding, Some(set), &[]);

        Some(())
    }

    pub unsafe fn push_constant_raw(&mut self, offset: u32, data: &[u32]) {
        self.buf
            .push_compute_constants(self.pipeline_layout, offset, data);
    }

    pub unsafe fn push_constant<T: Sized>(&mut self, offset: u32, data: T) {
        use smallvec::SmallVec;
        let mut buf = SmallVec::<[u8; 1024]>::new();

        buf.set_len(1024);

        {
            let u32_slice = data_to_u32_slice(data, &mut buf[..]);

            self.push_constant_raw(offset, u32_slice);
        }
    }
}

unsafe fn data_to_u32_slice<T: Sized>(data: T, buf: &mut [u8]) -> &[u32] {
    use std::mem::size_of;
    use std::mem::transmute;
    use std::ptr::copy_nonoverlapping;
    use std::slice::from_raw_parts;

    let data_size = size_of::<T>();
    let u32_size = size_of::<u32>();

    let rest = data_size % u32_size;
    let needs_padding = rest != 0;
    let padding = u32_size - rest;

    let buf_size = if needs_padding {
        data_size + padding
    } else {
        data_size
    };

    debug_assert!(buf.len() >= data_size);

    {
        let data_ptr: *const u8 = transmute(&data as *const _);
        let buf_ptr = &mut buf[0] as *mut _;

        copy_nonoverlapping(data_ptr, buf_ptr, data_size);
    }

    let u32_slice = {
        let buf_ptr: *const u32 = transmute(&buf[0] as *const _);
        let slice_len = buf_size / u32_size;

        from_raw_parts(buf_ptr, slice_len)
    };

    u32_slice
}
