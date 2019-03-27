/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Types and functions used during execution of passes.

use std::ops::Range;

use gfx;

use crate::types;

use crate::material::{MaterialInstanceHandle, MaterialStorage};

use crate::buffer::{BufferHandle, BufferStorage};

use crate::image::ImageStorage;
use std::cell::Ref;

/// Type used for the indices in the index buffer.
#[derive(Copy, Clone, Debug, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub enum IndexType {
    /// Unsigned 16-bit integers
    U16,

    /// Unsigned 32-bit integers
    U32,
}

pub(crate) struct ReadStorages<'a> {
    pub(crate) buffer: Ref<'a, BufferStorage>,
    pub(crate) material: Ref<'a, MaterialStorage>,
    pub(crate) _image: Ref<'a, ImageStorage>,
}

/// CommandBuffer object used to issue commands to a graphics queue.
pub struct GraphicsCommandBuffer<'a> {
    pub(crate) encoder: gfx::command::RenderPassInlineEncoder<'a, back::Backend>,
    pub(crate) storages: &'a ReadStorages<'a>,

    pub(crate) pipeline_layout: &'a types::PipelineLayout,
    pub(crate) viewport_rect: gfx::pso::Rect,
}

impl<'a> GraphicsCommandBuffer<'a> {
    /// Dispatch a draw call for "array" rendering.
    ///
    /// This draw mode treats every vertex in the vertex buffer as an input-vertex.
    pub unsafe fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) {
        self.encoder.draw(vertices, instances);
    }

    /// Dispatch a draw call for indexed rendering.
    ///
    /// This draw mode uses an index buffer to point to actual vertices in the vertex buffer.
    /// This is useful when a lot of vertices in the vertex data are shared.
    ///
    /// The base-vertex is an offset into the index buffer.
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
        let stores = self.storages;

        let bufs = buffers.into_iter().filter_map(|i| {
            let (buffer, index) = i.borrow();
            stores
                .buffer
                .raw(*buffer)
                .map(|buf| (buf.buffer.raw(), *index as u64))
        });

        self.encoder.bind_vertex_buffers(0, bufs);
    }

    /// Bind an index buffer, starting from `offset` bytes in the buffer represented by `buffer`.
    pub unsafe fn bind_index_buffer(
        &mut self,
        buffer: BufferHandle,
        offset: u64,
        index_type: IndexType,
    ) {
        let stores = self.storages;

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

    /// Bind [`MaterialInstance`] to a descriptor set in the pipeline.
    ///
    /// [`MaterialInstance`]: ../../resources/material/struct.MaterialInstance.html
    pub unsafe fn bind_material(
        &mut self,
        binding: usize,
        material: MaterialInstanceHandle,
    ) -> Option<()> {
        let layout = self.pipeline_layout;

        let mat = self.storages.material.raw(material.material)?;
        let instance = mat.instance_raw(material.instance)?;

        let set = &instance.set;

        self.encoder
            .bind_graphics_descriptor_sets(layout, binding, Some(set), &[]);

        Some(())
    }

    unsafe fn push_constant_raw(&mut self, offset: u32, data: &[u32]) {
        self.encoder.push_graphics_constants(
            self.pipeline_layout,
            gfx::pso::ShaderStageFlags::ALL,
            offset,
            data,
        )
    }

    /// Upload a value to the push-constant memory.
    pub unsafe fn push_constant<T: Sized + Copy>(&mut self, offset: u32, data: T) {
        use smallvec::SmallVec;
        let mut buf = SmallVec::<[u32; 256]>::new();

        buf.set_len(256);

        {
            let u32_slice = data_to_u32_slice(data, &mut buf[..]);
            self.push_constant_raw(offset / 4, u32_slice);
        }
    }

    /// Set the scissor "cutoff".
    pub unsafe fn set_scissor(&mut self, origin: (i16, i16), size: (i16, i16)) {
        self.encoder.set_scissors(
            0,
            &[gfx::pso::Rect {
                x: origin.0,
                y: origin.1,
                w: size.0,
                h: size.1,
            }],
        );
    }

    /// Reset the scissor state. This sets the scissor rect to the area of the framebuffer.
    pub unsafe fn reset_scissor(&mut self) {
        self.encoder.set_scissors(0, &[self.viewport_rect]);
    }
}

/// A command buffer used in compute passes.
pub struct ComputeCommandBuffer<'a> {
    pub(crate) buf: &'a mut crate::resources::command_pool::CmdBufType<gfx::Compute>,
    pub(crate) storages: &'a ReadStorages<'a>,

    pub(crate) pipeline_layout: &'a types::PipelineLayout,
}

impl<'a> ComputeCommandBuffer<'a> {
    /// Execute a workgroup.
    pub unsafe fn dispatch(&mut self, workgroup_count: [u32; 3]) {
        self.buf.dispatch(workgroup_count)
    }

    /// bind a [`MaterialInstance`] to a descriptor set in the pipeline.
    pub unsafe fn bind_material(
        &mut self,
        binding: usize,
        material: MaterialInstanceHandle,
    ) -> Option<()> {
        let layout = self.pipeline_layout;

        let mat = self.storages.material.raw(material.material)?;
        let instance = mat.instance_raw(material.instance)?;

        let set = &instance.set;

        self.buf
            .bind_compute_descriptor_sets(layout, binding, Some(set), &[]);

        Some(())
    }

    unsafe fn push_constant_raw(&mut self, offset: u32, data: &[u32]) {
        self.buf
            .push_compute_constants(self.pipeline_layout, offset, data);
    }

    /// Upload a value to the push-constant memory.
    pub unsafe fn push_constant<T: Sized + Copy>(&mut self, offset: u32, data: T) {
        use smallvec::SmallVec;
        let mut buf = SmallVec::<[u32; 256]>::new();

        buf.set_len(256);

        {
            let u32_slice = data_to_u32_slice(data, &mut buf[..]);
            self.push_constant_raw(offset / 4, u32_slice);
        }
    }
}

// the "buf" slice **MUST** be aligned to u32
unsafe fn data_to_u32_slice<T: Sized>(data: T, buf: &mut [u32]) -> &[u32] {
    use std::mem::size_of;
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

    debug_assert!(buf.len() * u32_size >= data_size);

    {
        let data_ptr: *const u8 = &data as *const _ as *const u8;
        let buf_ptr = &mut buf[0] as *mut u32 as *mut u8;

        copy_nonoverlapping(data_ptr, buf_ptr, data_size);
    }

    // create the "output" slice with correct size
    {
        let buf_ptr: *const u32 = &buf[0] as *const _ as *const u32;
        let slice_len = buf_size / u32_size;

        from_raw_parts(buf_ptr, slice_len)
    }
}
