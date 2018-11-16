use storage::{Handle, Storage};

use gfx;
use smallvec::SmallVec;

pub type VertexAttribHandle = Handle<VertexAttrib>;

pub struct VertexAttrib {
    pub(crate) buffer_stride: usize,
    pub(crate) attribs: Vec<gfx::pso::AttributeDesc>,
}

pub struct VertexAttribStorage {
    storage: Storage<VertexAttrib>,
}

pub struct VertexAttribInfo<'a> {
    pub buffer_stride: usize,
    pub buffer_infos: &'a [VertexAttribBufferInfo<'a>],
}

pub struct VertexAttribBufferInfo<'a> {
    pub index: u32,
    pub elements: &'a [VertexAttribBufferElementInfo],
}

pub struct VertexAttribBufferElementInfo {
    pub location: u32,
    pub format: gfx::format::Format,
    pub offset: u32,
}

impl VertexAttribStorage {
    pub fn new() -> Self {
        VertexAttribStorage {
            storage: Storage::new(),
        }
    }

    pub fn create(
        &mut self,
        create_infos: &[VertexAttribInfo],
    ) -> SmallVec<[VertexAttribHandle; 16]> {
        create_infos
            .iter()
            .map(|create_info| {
                let num_attribs = {
                    create_info
                        .buffer_infos
                        .iter()
                        .map(|buffer| buffer.elements.len())
                        .sum()
                };

                let mut attribs = Vec::with_capacity(num_attribs);

                let attrib_iter = create_info.buffer_infos.iter().flat_map(|buffer| {
                    let index = buffer.index;

                    buffer
                        .elements
                        .iter()
                        .map(move |elem| gfx::pso::AttributeDesc {
                            location: elem.location,
                            binding: index,
                            element: gfx::pso::Element {
                                format: elem.format,
                                offset: elem.offset,
                            },
                        })
                });

                attribs.extend(attrib_iter);

                VertexAttrib { attribs, buffer_stride: create_info.buffer_stride }
            }).map(|attrib| self.storage.insert(attrib).0)
            .collect()
    }

    pub fn raw(&self, handle: VertexAttribHandle) -> Option<&VertexAttrib> {
        if self.storage.is_alive(handle) {
            Some(&self.storage[handle])
        } else {
            None
        }
    }

    pub fn destroy(&mut self, handles: &[VertexAttribHandle]) {
        for handle in handles {
            self.storage.remove(*handle);
        }
    }
}
