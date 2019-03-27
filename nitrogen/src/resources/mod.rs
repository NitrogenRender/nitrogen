/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Descriptions for various kinds of resources useful for rendering.

pub use super::*;

pub mod buffer;
pub(crate) mod command_pool;
pub mod image;
pub mod material;
pub(crate) mod pipeline;
pub(crate) mod render_pass;
pub mod sampler;
pub(crate) mod semaphore_pool;
pub mod shader;
pub mod vertex_attrib;

use bitflags::bitflags;

bitflags!(

    /// Memory property flags.
    pub struct MemoryProperties: u16 {
        /// Device local memory on the GPU.
        const DEVICE_LOCAL   = 0x1;

        /// CPU-GPU coherent.
        ///
        /// Non-coherent memory requires explicit flushing.
        const COHERENT     = 0x2;

        /// Host visible memory can be accessed by the CPU.
        ///
        /// Backends must provide at least one cpu visible memory.
        const CPU_VISIBLE   = 0x4;

        /// Cached memory by the CPU
        const CPU_CACHED = 0x8;

        /// Memory that may be lazily allocated as needed on the GPU
        /// and *must not* be visible to the CPU.
        const LAZILY_ALLOCATED = 0x20;
    }

);

impl From<MemoryProperties> for gfx::memory::Properties {
    fn from(props: MemoryProperties) -> Self {
        use gfx::memory::Properties;

        let mut p = Properties::empty();

        if props.contains(MemoryProperties::DEVICE_LOCAL) {
            p |= Properties::DEVICE_LOCAL;
        }
        if props.contains(MemoryProperties::COHERENT) {
            p |= Properties::COHERENT;
        }
        if props.contains(MemoryProperties::CPU_VISIBLE) {
            p |= Properties::CPU_VISIBLE;
        }
        if props.contains(MemoryProperties::CPU_CACHED) {
            p |= Properties::CPU_CACHED;
        }
        if props.contains(MemoryProperties::LAZILY_ALLOCATED) {
            p |= Properties::LAZILY_ALLOCATED;
        }

        p
    }
}
