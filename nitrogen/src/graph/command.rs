use std::marker::PhantomData;
use std::ops::Range;

use back;
use gfx;
use types;

use image::{ImageHandle, ImageStorage};
use material::{MaterialInstanceHandle, MaterialStorage};
use sampler::SamplerHandle;

use buffer::{BufferHandle, BufferStorage};

pub(crate) struct ReadStorages<'a> {
    pub(crate) buffer: &'a BufferStorage,
    pub(crate) material: &'a MaterialStorage,
}

pub struct CommandBuffer<'a> {
    pub(crate) encoder:
        gfx::command::RenderPassInlineEncoder<'a, back::Backend, gfx::command::Primary>,
    pub(crate) storages: &'a ReadStorages<'a>,

    pub(crate) pipeline_layout: &'a types::PipelineLayout,
}

impl<'a> CommandBuffer<'a> {
    pub fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) {
        self.encoder.draw(vertices, instances);
    }

    pub fn bind_vertex_array(&mut self, buffer: BufferHandle) {
        let buffer = if let Some(buf) = self.storages.buffer.raw(buffer) {
            buf.raw()
        } else {
            return;
        };

        self.encoder.bind_vertex_buffers(0, Some((buffer, 0)));
    }

    pub fn bind_graphics_descriptor_set(
        &mut self,
        binding: usize,
        descriptor_set: MaterialInstanceHandle,
    ) -> Option<()> {
        let layout = self.pipeline_layout;

        let mat = self.storages.material.raw(descriptor_set.0)?;
        let instance = mat.intance_raw(descriptor_set.1)?;

        let set = &instance.set;

        self.encoder
            .bind_graphics_descriptor_sets(layout, binding, Some(set), &[]);

        Some(())
    }
}