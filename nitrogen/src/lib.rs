/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

extern crate gfx_backend_vulkan as back;
pub extern crate gfx_hal as gfx;
extern crate gfx_memory as gfxm;

extern crate smallvec;

extern crate failure;
extern crate failure_derive;

extern crate bitflags;

extern crate ash;

extern crate slab;

#[cfg(feature = "winit_support")]
extern crate winit;

use smallvec::SmallVec;

pub mod types;

pub mod display;
use display::Display;

pub mod device;
use device::DeviceContext;

pub mod util;
pub use util::storage;
pub use util::submit_group;
pub use util::transfer;

pub use util::CowString;

use storage::{Handle, Storage};

pub mod resources;
pub use resources::buffer;
pub use resources::image;
pub use resources::material;
pub use resources::pipeline;
pub use resources::render_pass;
pub use resources::sampler;
pub use resources::vertex_attrib;

pub mod graph;

#[cfg(feature = "x11")]
use ash::vk;

use std::sync::Arc;

pub type DisplayHandle = Handle<Display>;

// DON'T CHANGE THE ORDER OF THE MEMBERS HERE!!!!
//
// Rust drops structs by dropping the members in declaration order, so things that need to be
// dropped first need to be first in the struct declaration.
//
// BAD THINGS WILL HAPPEN IF YOU CHANGE IT.
// MOUNTAINS OF CRASHES WILL POUR ONTO YOU.
// So please, just don't.
pub struct Context {
    pub(crate) graph_storage: graph::GraphStorage,

    pub(crate) render_pass_storage: render_pass::RenderPassStorage,
    pub(crate) pipeline_storage: pipeline::PipelineStorage,
    pub(crate) image_storage: image::ImageStorage,
    pub(crate) sampler_storage: sampler::SamplerStorage,
    pub(crate) buffer_storage: buffer::BufferStorage,
    pub(crate) vertex_attrib_storage: vertex_attrib::VertexAttribStorage,
    pub(crate) material_storage: material::MaterialStorage,

    pub(crate) displays: Storage<Display>,
    pub(crate) transfer: transfer::TransferContext,
    pub(crate) device_ctx: Arc<DeviceContext>,
    pub(crate) instance: back::Instance,
}

impl Context {
    pub fn new(name: &str, version: u32) -> Self {
        let instance = back::Instance::create(name, version);
        let device_ctx = Arc::new(DeviceContext::new(&instance));

        let transfer = transfer::TransferContext::new();

        let image_storage = image::ImageStorage::new();
        let sampler_storage = sampler::SamplerStorage::new();
        let buffer_storage = buffer::BufferStorage::new();
        let vertex_attrib_storage = vertex_attrib::VertexAttribStorage::new();
        let pipeline_storage = pipeline::PipelineStorage::new();
        let render_pass_storage = render_pass::RenderPassStorage::new();
        let material_storage = material::MaterialStorage::new();

        let graph_storage = graph::GraphStorage::new();

        Context {
            instance,
            device_ctx,
            transfer,
            displays: Storage::new(),
            pipeline_storage,
            render_pass_storage,
            image_storage,
            sampler_storage,
            buffer_storage,
            vertex_attrib_storage,
            material_storage,
            graph_storage,
        }
    }

    #[cfg(feature = "x11")]
    pub fn add_x11_display(
        &mut self,
        display: *mut vk::Display,
        window: vk::Window,
    ) -> DisplayHandle {
        use gfx::Surface;

        let surface = self.instance.create_surface_from_xlib(display, window);

        let _ = self
            .device_ctx
            .adapter
            .queue_families
            .iter()
            .position(|fam| surface.supports_queue_family(fam))
            .expect("No queue family that supports this surface was found.");

        let display = Display::new(surface, &self.device_ctx);

        self.displays.insert(display).0
    }

    #[cfg(feature = "winit_support")]
    pub fn add_display(&mut self, window: &winit::Window) -> Handle<Display> {
        use gfx::Surface;

        let surface = self.instance.create_surface(window);

        let _ = self
            .device_ctx
            .adapter
            .queue_families
            .iter()
            .position(|fam| surface.supports_queue_family(fam))
            .expect("No queue family that supports this surface was found.");

        let display = Display::new(surface, &self.device_ctx);

        self.displays.insert(display).0
    }

    pub fn remove_display(&mut self, display: DisplayHandle) -> bool {
        match self.displays.remove(display) {
            None => false,
            Some(display) => {
                display.release(&self.device_ctx);
                true
            }
        }
    }

    pub fn release(self) {
        self.buffer_storage.release();
        self.image_storage.release(&self.device_ctx);

        self.material_storage.release(&self.device_ctx);

        for (_, display) in self.displays {
            display.release(&self.device_ctx);
        }

        self.transfer.release();

        Arc::try_unwrap(self.device_ctx).ok().unwrap().release();
    }

    // image

    pub fn image_create(
        &mut self,
        create_infos: &[image::ImageCreateInfo<image::ImageUsage>],
    ) -> SmallVec<[image::Result<image::ImageHandle>; 16]> {
        self.image_storage.create(&self.device_ctx, create_infos)
    }

    // sampler

    pub fn sampler_create(
        &mut self,
        create_infos: &[sampler::SamplerCreateInfo],
    ) -> SmallVec<[sampler::SamplerHandle; 16]> {
        self.sampler_storage.create(&self.device_ctx, create_infos)
    }

    // buffer

    pub fn buffer_create(
        &mut self,
        create_infos: &[buffer::BufferCreateInfo],
    ) -> SmallVec<[buffer::Result<buffer::BufferHandle>; 16]> {
        self.buffer_storage.create(&self.device_ctx, create_infos)
    }

    // vertex attribs

    pub fn vertex_attribs_create(
        &mut self,
        infos: &[vertex_attrib::VertexAttribInfo],
    ) -> SmallVec<[vertex_attrib::VertexAttribHandle; 16]> {
        self.vertex_attrib_storage.create(infos)
    }

    pub fn vertex_attribs_destroy(&mut self, handles: &[vertex_attrib::VertexAttribHandle]) {
        self.vertex_attrib_storage.destroy(handles);
    }

    // material

    pub fn material_create(
        &mut self,
        create_infos: &[material::MaterialCreateInfo],
    ) -> SmallVec<[Result<material::MaterialHandle, material::MaterialError>; 16]> {
        self.material_storage.create(&self.device_ctx, create_infos)
    }

    pub fn material_destroy(&mut self, materials: &[material::MaterialHandle]) {
        self.material_storage.destroy(&self.device_ctx, materials)
    }

    pub fn material_create_instance(
        &mut self,
        materials: &[material::MaterialHandle],
    ) -> SmallVec<[Result<material::MaterialInstanceHandle, material::MaterialError>; 16]> {
        self.material_storage
            .create_instances(&self.device_ctx, materials)
    }

    pub fn material_destroy_instance(&mut self, instances: &[material::MaterialInstanceHandle]) {
        self.material_storage
            .destroy_instances(&self.device_ctx, instances)
    }

    pub fn material_write_instance<T>(
        &mut self,
        instance: material::MaterialInstanceHandle,
        data: T,
    ) where
        T: IntoIterator,
        T::Item: ::std::borrow::Borrow<material::InstanceWrite>,
    {
        self.material_storage.write_instance(
            &self.device_ctx,
            &self.sampler_storage,
            &self.image_storage,
            &self.buffer_storage,
            instance,
            data,
        );
    }

    pub fn graph_create(&mut self) -> graph::GraphHandle {
        self.graph_storage.create()
    }

    pub fn graph_add_pass<T: Into<graph::PassName>>(
        &mut self,
        graph: graph::GraphHandle,
        name: T,
        info: graph::PassInfo,
        pass_impl: Box<dyn graph::PassImpl>,
    ) {
        self.graph_storage.add_pass(graph, name, info, pass_impl);
    }

    pub fn graph_add_output<T: Into<graph::ResourceName>>(
        &mut self,
        graph: graph::GraphHandle,
        name: T,
    ) {
        self.graph_storage.add_output(graph, name);
    }

    pub fn graph_destroy(&mut self, graph: graph::GraphHandle) {
        self.graph_storage.destroy(
            &self.device_ctx,
            &mut self.render_pass_storage,
            &mut self.pipeline_storage,
            &mut self.image_storage,
            &mut self.buffer_storage,
            &self.vertex_attrib_storage,
            &mut self.sampler_storage,
            &self.material_storage,
            graph,
        );
    }

    pub fn graph_compile(
        &mut self,
        graph: graph::GraphHandle,
    ) -> Result<(), Vec<graph::GraphCompileError>> {
        self.graph_storage.compile(graph)
    }

    // submit group

    pub fn create_submit_group(&self) -> submit_group::SubmitGroup {
        submit_group::SubmitGroup::new(self.device_ctx.clone())
    }
}
