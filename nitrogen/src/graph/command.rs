use std::ops::Range;

use back;
use gfx;

pub struct CommandBuffer<'a> {
    pub(crate) command_buffer:
        gfx::command::RenderPassInlineEncoder<'a, back::Backend, gfx::command::Primary>,
}

impl<'a> CommandBuffer<'a> {
    pub(crate) fn new(
        cmd_buffer: gfx::command::RenderPassInlineEncoder<'a, back::Backend, gfx::command::Primary>,
    ) -> Self {
        CommandBuffer {
            command_buffer: cmd_buffer,
        }
    }

    pub fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) {
        self.command_buffer.draw(vertices, instances);
    }
}
