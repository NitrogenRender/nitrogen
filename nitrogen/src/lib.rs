/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#![doc(html_logo_url = "https://raw.githubusercontent.com/NitrogenRender/nitrogen/master/logo.png")]

//! A render graph based rendering engine.
//!
//! Nitrogen is intended to provide high-level abstractions to build well performing graphics
//! applications.
//!
//! The [`Context`] holds all information needed for rendering and dispatching compute jobs.
//! It is the central part that a user will interface with.
//!
//! To build graphics or compute applications, various resources can be created.
//! Those resources can be used as inputs (or storages/outputs) when executing a *"render graph"*.
//!
//! A render graph consists of passes which can create resources and operate upon them, as well as
//! depend on resources created from other passes.
//!
//! This makes for nicely structured and decoupled "building blocks" for building
//! rendering pipelines.
//!
//! # Conceptual example: a simple deferred pipeline
//!
//! In order to build a simple deferred rendering pipeline, following passes could be set up that
//! each take a small part in achieving the final result.
//!
//! ## Passes
//!
//! - DepthPrePass
//!
//!   - creates `"PreDepth"` resource
//!
//! - GbufferPass
//!
//!   - reads `"PreDepth"`
//!   - creates `"AlbedoRoughness"` resource
//!   - creates `"NormalMetallic"` resource
//!   - creates `"Emission"` resource
//!
//! - LightingPass
//!
//!   - reads `"PreDepth"`, `"AlbedoRoughness"`, `"NormalMetallic"` and `"Emission"`
//!   - creates `"Shaded"` resource
//!
//! - PostProcess
//!
//!   - moves `"Shaded"` to `"Final"`
//!
//! [`Context`]: ./struct.Context.html

#![warn(missing_docs)]

#[macro_use]
extern crate derive_more;

pub extern crate gfx;

pub(crate) mod types;

pub mod display;
use crate::display::Display;

pub mod submit_group;
pub use crate::submit_group::SubmitGroup;

pub(crate) mod device;
use crate::device::DeviceContext;

pub mod util;
pub use crate::util::storage;
pub(crate) use crate::util::transfer;

use crate::storage::{Handle, Storage};

pub mod resources;
pub use crate::resources::buffer;
pub use crate::resources::image;
pub use crate::resources::material;
pub(crate) use crate::resources::pipeline;
pub(crate) use crate::resources::render_pass;
pub use crate::resources::sampler;
pub use crate::resources::shader;
pub use crate::resources::vertex_attrib;

pub mod graph;

use crate::resources::image::ImageHandle;
use std::cell::RefCell;
use std::sync::Arc;

/// An opaque handle to a display
pub type DisplayHandle = Handle<Display>;

// DON'T CHANGE THE ORDER OF THE MEMBERS HERE!!!!
//
// Rust drops structs by dropping the members in declaration order, so things that need to be
// dropped first need to be first in the struct declaration.
//
// BAD THINGS WILL HAPPEN IF YOU CHANGE IT.
// MOUNTAINS OF CRASHES WILL POUR ONTO YOU.
// So please, just don't.
/// Contains all state needed for executing graphics or compute programs.
///
/// The `Context` contains multiple "sub-contexts" all managing specific resources.
/// Sub-contexts try to use [`Handle`]s as much as possible.
///
/// Since the shutdown sequence of many GPU-resources requires moving data the `Drop` trait is
/// **not implemented**. Dropping the context will result in a panic. Instead, use the [`release`]
/// method. (It is planned in future to improve this)
///
/// For ease of use, all functionality of sub-contexts that the programmer needs to deal with are
/// replicated as [methods].
///
/// [`Handle`]: ./util/storage/struct.Handle.html
/// [`release`]: #method.release
/// [methods]: #methods
pub struct Context {
    pub(crate) graph_storage: RefCell<graph::GraphStorage>,

    pub(crate) render_pass_storage: RefCell<render_pass::RenderPassStorage>,
    pub(crate) pipeline_storage: RefCell<pipeline::PipelineStorage>,
    pub(crate) image_storage: RefCell<image::ImageStorage>,
    pub(crate) sampler_storage: RefCell<sampler::SamplerStorage>,
    pub(crate) buffer_storage: RefCell<buffer::BufferStorage>,
    pub(crate) material_storage: RefCell<material::MaterialStorage>,
    pub(crate) shader_storage: RefCell<shader::ShaderStorage>,

    pub(crate) displays: Storage<Display>,
    pub(crate) device_ctx: Arc<DeviceContext>,
    pub(crate) instance: back::Instance,
}

impl Context {
    /// Create a new `Context` instance.
    ///
    /// The `name` and `version` fields are passed down to the graphics driver. They don't have any
    /// special meaning attached to them (as far as I know)
    pub unsafe fn new(name: &str, version: u32) -> Self {
        use gfx::adapter::PhysicalDevice;

        let instance = back::Instance::create(name, version);
        let device_ctx = Arc::new(DeviceContext::new(&instance));

        let memory_atom_size = device_ctx
            .adapter
            .physical_device
            .limits()
            .non_coherent_atom_size;

        let image_storage = image::ImageStorage::new();
        let sampler_storage = sampler::SamplerStorage::new();
        let buffer_storage = buffer::BufferStorage::new(memory_atom_size);
        let pipeline_storage = pipeline::PipelineStorage::new();
        let render_pass_storage = render_pass::RenderPassStorage::new();
        let material_storage = material::MaterialStorage::new();
        let shader_storage = shader::ShaderStorage::new();

        let graph_storage = graph::GraphStorage::new();

        Context {
            instance,
            device_ctx,
            displays: Storage::new(),

            pipeline_storage: RefCell::new(pipeline_storage),
            render_pass_storage: RefCell::new(render_pass_storage),
            image_storage: RefCell::new(image_storage),
            sampler_storage: RefCell::new(sampler_storage),
            buffer_storage: RefCell::new(buffer_storage),
            material_storage: RefCell::new(material_storage),
            shader_storage: RefCell::new(shader_storage),

            graph_storage: RefCell::new(graph_storage),
        }
    }

    /// Attach an X11 display to the `Context`
    #[cfg(feature = "x11")]
    pub unsafe fn display_add_x11(
        &mut self,
        display: *mut std::os::raw::c_void,
        window: std::os::raw::c_ulong,
    ) -> DisplayHandle {
        use gfx::Surface;

        let surface = self
            .instance
            .create_surface_from_xlib(display as *mut _, window);

        let _ = self
            .device_ctx
            .adapter
            .queue_families
            .iter()
            .position(|fam| surface.supports_queue_family(fam))
            .expect("No queue family that supports this surface was found.");

        let display = Display::new(surface, &self.device_ctx);

        self.displays.insert(display)
    }

    /// Attach a winit display to the `Context`
    #[cfg(feature = "winit_support")]
    pub fn display_add(&mut self, window: &winit::Window) -> Handle<Display> {
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

        self.displays.insert(display)
    }

    /// Detach a display from the `Context`
    pub unsafe fn display_remove(&mut self, display: DisplayHandle) -> bool {
        match self.displays.remove(display) {
            None => false,
            Some(display) => {
                display.release(&self.device_ctx);
                true
            }
        }
    }

    /// Free all resources and release the `Context`
    pub unsafe fn release(self) {
        self.buffer_storage.into_inner().release(&self.device_ctx);
        self.image_storage.into_inner().release(&self.device_ctx);
        self.sampler_storage.into_inner().release(&self.device_ctx);

        self.material_storage.into_inner().release(&self.device_ctx);

        for (_, display) in self.displays {
            display.release(&self.device_ctx);
        }

        Arc::try_unwrap(self.device_ctx).ok().unwrap().release();
    }

    // image

    /// Create image objects and retrieve handles for them.
    pub unsafe fn image_create<I: Into<gfx::image::Usage> + Clone>(
        &mut self,
        create_info: image::ImageCreateInfo<I>,
    ) -> Result<image::ImageHandle, image::ImageError> {
        self.image_storage
            .borrow_mut()
            .create(&self.device_ctx, create_info)
    }

    /// Get the format of an image.
    pub fn image_format(&self, image: ImageHandle) -> Option<gfx::format::Format> {
        self.image_storage.borrow().format(image)
    }

    /// Get the usage flags of an image.
    pub fn image_usage(&self, image: ImageHandle) -> Option<gfx::image::Usage> {
        self.image_storage.borrow().usage(image)
    }

    // sampler

    /// Create sampler objects and retrieve handles for them.
    pub unsafe fn sampler_create(
        &mut self,
        create_info: sampler::SamplerCreateInfo,
    ) -> sampler::SamplerHandle {
        self.sampler_storage
            .borrow_mut()
            .create(&self.device_ctx, create_info)
    }

    // buffer

    /// Create buffer objects and retrieve handles for them.
    pub unsafe fn buffer_cpu_visible_create<U>(
        &mut self,
        create_info: buffer::CpuVisibleCreateInfo<U>,
    ) -> Result<buffer::BufferHandle, buffer::BufferError>
    where
        U: Into<gfx::buffer::Usage> + Clone,
    {
        self.buffer_storage
            .borrow_mut()
            .cpu_visible_create(&self.device_ctx, create_info)
    }

    /// Create a buffer object that is backed by device-local memory.
    ///
    /// A buffer that resides in device-local memory can not be accessed directly by the CPU.
    /// Instead, "staging buffers" are used (which are CPU visible) to read or set data.
    pub unsafe fn buffer_device_local_create<U>(
        &mut self,
        create_info: buffer::DeviceLocalCreateInfo<U>,
    ) -> Result<buffer::BufferHandle, buffer::BufferError>
    where
        U: Into<gfx::buffer::Usage> + Clone,
    {
        self.buffer_storage
            .borrow_mut()
            .device_local_create(&self.device_ctx, create_info)
    }

    // material

    /// Create material objects and retrieve handles for them.
    ///
    /// For more information about materials, see the [`material` module] documentation.
    ///
    /// [`material` module]: ./resources/material/index.html
    pub unsafe fn material_create(
        &mut self,
        create_info: material::MaterialCreateInfo,
    ) -> Result<material::MaterialHandle, material::MaterialError> {
        self.material_storage
            .borrow_mut()
            .create(&self.device_ctx, create_info)
    }

    /// Create material instances and retrieve handles for them.
    ///
    /// For more information about material instances, see the [`material` module] documentation.
    ///
    /// [`material` module]: ./resources/material/index.html
    pub unsafe fn material_create_instance(
        &mut self,
        material: material::MaterialHandle,
    ) -> Result<material::MaterialInstanceHandle, material::MaterialError> {
        self.material_storage
            .borrow_mut()
            .create_instance(&self.device_ctx, material)
    }

    /// Update a material instance with resource handles.
    pub unsafe fn material_write_instance<T>(
        &mut self,
        instance: material::MaterialInstanceHandle,
        data: T,
    ) where
        T: IntoIterator,
        T::Item: ::std::borrow::Borrow<material::InstanceWrite>,
    {
        self.material_storage.borrow_mut().write_instance(
            &self.device_ctx,
            &*self.sampler_storage.borrow(),
            &*self.image_storage.borrow(),
            &*self.buffer_storage.borrow(),
            instance,
            data,
        );
    }

    // graph

    /// Create a new graph and retrieve the handle.
    pub unsafe fn graph_create(
        &mut self,
        builder: graph::GraphBuilder,
    ) -> Result<graph::GraphHandle, graph::GraphError> {
        let mut storages = graph::Storages {
            shader: &self.shader_storage,
            render_pass: &mut self.render_pass_storage,
            pipeline: &mut self.pipeline_storage,
            image: &mut self.image_storage,
            buffer: &mut self.buffer_storage,
            sampler: &mut self.sampler_storage,
            material: &mut self.material_storage,
        };

        self.graph_storage
            .borrow_mut()
            .create(&self.device_ctx, &mut storages, builder)
    }

    // shader

    /// Create a compute shader and retrieve the handle.
    pub fn compute_shader_create(
        &mut self,
        info: shader::ShaderInfo,
    ) -> shader::ComputeShaderHandle {
        self.shader_storage.borrow_mut().create_compute_shader(info)
    }

    /// Destroy a compute shader object.
    pub fn compute_shader_destroy(&mut self, handle: shader::ComputeShaderHandle) {
        self.shader_storage
            .borrow_mut()
            .destroy_compute_shader(handle);
    }

    /// Create a vertex shader and retrieve the handle.
    pub fn vertex_shader_create(&mut self, info: shader::ShaderInfo) -> shader::VertexShaderHandle {
        self.shader_storage.borrow_mut().create_vertex_shader(info)
    }

    /// Destroy a vertex shader object.
    pub fn vertex_shader_destroy(&mut self, handle: shader::VertexShaderHandle) {
        self.shader_storage
            .borrow_mut()
            .destroy_vertex_shader(handle);
    }

    /// Create a fragment shader and retrieve the handle.
    pub fn fragment_shader_create(
        &mut self,
        info: shader::ShaderInfo,
    ) -> shader::FragmentShaderHandle {
        self.shader_storage
            .borrow_mut()
            .create_fragment_shader(info)
    }

    /// Destroy a fragment shader object.
    pub fn fragment_shader_destroy(&mut self, handle: shader::FragmentShaderHandle) {
        self.shader_storage
            .borrow_mut()
            .destroy_fragment_shader(handle);
    }

    /// Create a geometry shader and retrieve the handle.
    pub fn geometry_shader_create(
        &mut self,
        info: shader::ShaderInfo,
    ) -> shader::GeometryShaderHandle {
        self.shader_storage
            .borrow_mut()
            .create_geometry_shader(info)
    }

    /// Destroy a geometry shader object.
    pub fn geometry_shader_destroy(&mut self, handle: shader::GeometryShaderHandle) {
        self.shader_storage
            .borrow_mut()
            .destroy_geometry_shader(handle);
    }

    // submit group

    /// Create a new [`SubmitGroup`] to record and execute commands
    ///
    /// [`SubmitGroup`]: ./submit_group/struct.SubmitGroup.html
    pub unsafe fn create_submit_group(&self) -> submit_group::SubmitGroup {
        submit_group::SubmitGroup::new(self.device_ctx.clone())
    }

    /// Blocks on the calling site until the device is idling.
    ///
    /// This can be used to make sure that no resources are currently in use and are free to be
    /// destroyed or changed.
    ///
    /// This function should generally only be used upon application shutdown.
    pub unsafe fn wait_idle(&self) {
        use gfx::Device;
        // TODO handle this error?
        let _ = self.device_ctx.device.wait_idle();
    }
}
