use std::ops::Range;
use std::marker::PhantomData;

use gfx;
use back;
use types;

use image::{ImageHandle, ImageStorage};
use sampler::SamplerHandle;

use buffer::{BufferHandle, BufferStorage};

pub(crate) struct ReadStorages<'a> {
    pub(crate) image: &'a ImageStorage,
    pub(crate) buffer: &'a BufferStorage,
}


pub struct CommandBuffer<'a> {
    pub(crate) encoder: gfx::command::RenderPassInlineEncoder<'a, back::Backend, gfx::command::Primary>,
    pub(crate) storages: &'a ReadStorages<'a>,
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
}
