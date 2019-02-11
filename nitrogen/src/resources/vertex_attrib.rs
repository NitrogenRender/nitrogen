/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::storage::{Handle, Storage};

use smallvec::SmallVec;

pub type VertexAttribHandle = Handle<VertexAttrib>;

#[derive(Debug)]
pub struct VertexAttrib {
    /// stride and attributes
    pub(crate) buffers: Vec<VertexBufferDesc>,
    pub(crate) attribs: Vec<gfx::pso::AttributeDesc>,
}

#[derive(Debug)]
pub(crate) struct VertexBufferDesc {
    pub(crate) stride: usize,
    pub(crate) binding: usize,
}

pub struct VertexAttribInfo<'a> {
    pub buffer_infos: &'a [VertexAttribBufferInfo<'a>],
}

pub struct VertexAttribBufferInfo<'a> {
    pub stride: usize,
    pub index: u32,
    pub elements: &'a [VertexAttribBufferElementInfo],
}

pub struct VertexAttribBufferElementInfo {
    pub location: u32,
    pub format: gfx::format::Format,
    pub offset: u32,
}

pub(crate) struct VertexAttribStorage {
    storage: Storage<VertexAttrib>,
}

impl VertexAttribStorage {
    pub(crate) fn new() -> Self {
        VertexAttribStorage {
            storage: Storage::new(),
        }
    }

    pub(crate) fn create(&mut self, create_info: VertexAttribInfo) -> VertexAttribHandle {
        let num_attribs = {
            create_info
                .buffer_infos
                .iter()
                .map(|buffer| buffer.elements.len())
                .sum()
        };

        let mut attribs = Vec::with_capacity(num_attribs);
        let mut bufs = Vec::with_capacity(create_info.buffer_infos.len());

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

        let bufs_iter = create_info
            .buffer_infos
            .iter()
            .map(|buf_info| VertexBufferDesc {
                stride: buf_info.stride,
                binding: buf_info.index as _,
            });

        bufs.extend(bufs_iter);

        let attrib = VertexAttrib {
            buffers: bufs,
            attribs,
        };

        self.storage.insert(attrib)
    }

    pub(crate) fn raw(&self, handle: VertexAttribHandle) -> Option<&VertexAttrib> {
        if self.storage.is_alive(handle) {
            Some(&self.storage[handle])
        } else {
            None
        }
    }

    pub(crate) fn destroy(&mut self, handles: &[VertexAttribHandle]) {
        for handle in handles {
            self.storage.remove(*handle);
        }
    }
}
