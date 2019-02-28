/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Types and functions used during execution of passes.

use std::ops::Range;

use gfx;

use crate::types;

use crate::material::{MaterialInstanceHandle, MaterialStorage};

use crate::buffer::{BufferHandle, BufferStorage};

use crate::graph::ImageClearValue;
use crate::image::{ImageHandle, ImageStorage};

/// Type used for the indices in the index buffer.
#[derive(Copy, Clone, Debug, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub enum IndexType {
    /// Unsigned 16-bit integers
    U16,

    /// Unsigned 32-bit integers
    U32,
}

#[derive(Clone)]
pub(crate) struct ReadStorages<'a> {
    pub(crate) buffer: &'a BufferStorage,
    pub(crate) material: &'a MaterialStorage,
    pub(crate) image: &'a ImageStorage,
}

/// CommandBuffer object used to issue commands to a graphics queue.
pub struct GraphicsCommandBuffer<'a> {
    pub(crate) buf: &'a mut crate::resources::command_pool::CmdBufType<gfx::Graphics>,
    pub(crate) storages: &'a ReadStorages<'a>,

    pub(crate) framebuffer: Option<&'a types::Framebuffer>,
    pub(crate) viewport_rect: gfx::pso::Rect,

    pub(crate) render_pass: &'a types::RenderPass,
    pub(crate) pipeline_layout: &'a types::PipelineLayout,
}

impl<'a> GraphicsCommandBuffer<'a> {
    /// Start a render pass. Draw calls can only be issued from a [`RenderPassEncoder`].
    ///
    /// If any attachments have to be cleared, values from `clear_values` are used **in order of
    /// declaration**.
    ///
    /// [`RenderPassEncoder`]: ./struct.RenderPassEncoder.html
    pub unsafe fn begin_render_pass<C>(&mut self, clear_values: C) -> Option<RenderPassEncoder<'_>>
    where
        C: IntoIterator,
        C::Item: std::borrow::Borrow<ImageClearValue>,
    {
        let encoder = self.buf.begin_render_pass_inline(
            self.render_pass,
            self.framebuffer?,
            self.viewport_rect,
            clear_values.into_iter().map(|clear_value| {
                use std::borrow::Borrow;
                match clear_value.borrow() {
                    ImageClearValue::Color(color) => {
                        gfx::command::ClearValue::Color(gfx::command::ClearColor::Float(*color))
                    }
                    ImageClearValue::DepthStencil(depth, stencil) => {
                        gfx::command::ClearValue::DepthStencil(gfx::command::ClearDepthStencil(
                            *depth, *stencil,
                        ))
                    }
                }
            }),
        );

        Some(RenderPassEncoder {
            encoder,
            storages: self.storages,
            pipeline_layout: self.pipeline_layout,
        })
    }

    /// Dispatch a clearing command for image `image` using the clear value `clear`.
    pub unsafe fn clear_image(&mut self, image: ImageHandle, clear: ImageClearValue) -> Option<()> {
        let img = self.storages.image.raw(image)?;

        let entry_barrier = gfx::memory::Barrier::Image {
            states: (gfx::image::Access::empty(), gfx::image::Layout::Undefined)
                ..(
                    gfx::image::Access::TRANSFER_WRITE,
                    gfx::image::Layout::TransferDstOptimal,
                ),
            target: img.image.raw(),
            families: None,
            range: gfx::image::SubresourceRange {
                aspects: img.aspect,
                levels: 0..1,
                layers: 0..1,
            },
        };

        self.buf.pipeline_barrier(
            gfx::pso::PipelineStage::TOP_OF_PIPE..gfx::pso::PipelineStage::TRANSFER,
            gfx::memory::Dependencies::empty(),
            &[entry_barrier],
        );

        self.buf.clear_image(
            img.image.raw(),
            gfx::image::Layout::TransferDstOptimal,
            match clear {
                ImageClearValue::Color(color) => gfx::command::ClearColor::Float(color),
                _ => gfx::command::ClearColor::Float([0.0; 4]),
            },
            match clear {
                ImageClearValue::DepthStencil(depth, stencil) => {
                    gfx::command::ClearDepthStencil(depth, stencil)
                }
                _ => gfx::command::ClearDepthStencil(1.0, 0),
            },
            &[gfx::image::SubresourceRange {
                aspects: img.aspect,
                levels: 0..1,
                layers: 0..1,
            }],
        );

        let exit_barrier = gfx::memory::Barrier::Image {
            states: (
                gfx::image::Access::TRANSFER_WRITE,
                gfx::image::Layout::TransferDstOptimal,
            )..(gfx::image::Access::empty(), gfx::image::Layout::General),
            target: img.image.raw(),
            families: None,
            range: gfx::image::SubresourceRange {
                aspects: img.aspect,
                levels: 0..1,
                layers: 0..1,
            },
        };

        self.buf.pipeline_barrier(
            gfx::pso::PipelineStage::TRANSFER..gfx::pso::PipelineStage::BOTTOM_OF_PIPE,
            gfx::memory::Dependencies::empty(),
            &[exit_barrier],
        );

        Some(())
    }
}

/// An Encoder used to dispatch commands inside a render pass
pub struct RenderPassEncoder<'a> {
    pub(crate) encoder: gfx::command::RenderPassInlineEncoder<'a, back::Backend>,
    pub(crate) storages: &'a ReadStorages<'a>,

    pub(crate) pipeline_layout: &'a types::PipelineLayout,
}

impl<'a> RenderPassEncoder<'a> {
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

    /// Bind an index buffer, starting from `offset` bytes in the buffer represented by `buffer`.
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

    /// Bind [`MaterialInstance`] to a descriptor set in the pipeline.
    ///
    /// [`MaterialInstance`]: ../../resources/material/struct.MaterialInstance.html
    pub unsafe fn bind_material(
        &mut self,
        binding: usize,
        material: MaterialInstanceHandle,
    ) -> Option<()> {
        let layout = self.pipeline_layout;

        let mat = self.storages.material.raw(material.0)?;
        let instance = mat.instance_raw(material.1)?;

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
    ///
    /// **NOTE**: the offset is in 4-bytes, not bytes!!!
    pub unsafe fn push_constant<T: Sized + Copy>(&mut self, offset: u32, data: T) {
        use smallvec::SmallVec;
        let mut buf = SmallVec::<[u32; 256]>::new();

        buf.set_len(256);

        {
            let u32_slice = data_to_u32_slice(data, &mut buf[..]);
            self.push_constant_raw(offset, u32_slice);
        }
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

        let mat = self.storages.material.raw(material.0)?;
        let instance = mat.instance_raw(material.1)?;

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
    ///
    /// **NOTE**: the offset is in 4-bytes, not bytes!!!
    pub unsafe fn push_constant<T: Sized + Copy>(&mut self, offset: u32, data: T) {
        use smallvec::SmallVec;
        let mut buf = SmallVec::<[u32; 256]>::new();

        buf.set_len(256);

        {
            let u32_slice = data_to_u32_slice(data, &mut buf[..]);
            self.push_constant_raw(offset, u32_slice);
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
