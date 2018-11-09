use std::ops::Range;

pub struct CommandBuffer {}

impl CommandBuffer {
    pub fn draw(&mut self, _vertices: Range<u32>, _instance: Range<u32>) {}
}
