/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

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

#[macro_use]
extern crate derive_more;

pub use gfx;

pub(crate) mod types;

pub mod display;
use crate::display::Display;

pub(crate) mod device;
use crate::device::DeviceContext;

pub mod util;
pub use crate::util::storage;
pub use crate::util::submit_group;
pub(crate) use crate::util::transfer;

pub use crate::util::CowString;

use crate::storage::{Handle, Storage};

pub mod resources;
pub use crate::resources::buffer;
pub use crate::resources::image;
pub use crate::resources::material;
pub(crate) use crate::resources::pipeline;
pub(crate) use crate::resources::render_pass;
pub use crate::resources::sampler;
pub use crate::resources::vertex_attrib;

pub mod graph;

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
    pub(crate) graph_storage: graph::GraphStorage,

    pub(crate) render_pass_storage: render_pass::RenderPassStorage,
    pub(crate) pipeline_storage: pipeline::PipelineStorage,
    pub(crate) image_storage: image::ImageStorage,
    pub(crate) sampler_storage: sampler::SamplerStorage,
    pub(crate) buffer_storage: buffer::BufferStorage,
    pub(crate) vertex_attrib_storage: vertex_attrib::VertexAttribStorage,
    pub(crate) material_storage: material::MaterialStorage,

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
        let vertex_attrib_storage = vertex_attrib::VertexAttribStorage::new();
        let pipeline_storage = pipeline::PipelineStorage::new();
        let render_pass_storage = render_pass::RenderPassStorage::new();
        let material_storage = material::MaterialStorage::new();

        let graph_storage = graph::GraphStorage::new();

        Context {
            instance,
            device_ctx,
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

    /// Attach an X11 display to the `Context`
    #[cfg(feature = "x11")]
    pub fn display_add_x11(
        &mut self,
        display: *mut std::os::raw::c_void,
        window: std::os::raw::c_ulong,
    ) -> DisplayHandle {
        use gfx::Surface;
        use std::mem::transmute;

        let surface = unsafe {
            self.instance
                .create_surface_from_xlib(transmute(display), transmute(window))
        };

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
        self.buffer_storage.release(&self.device_ctx);
        self.image_storage.release(&self.device_ctx);
        self.sampler_storage.release(&self.device_ctx);

        self.material_storage.release(&self.device_ctx);

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
    ) -> image::Result<image::ImageHandle> {
        self.image_storage.create(&self.device_ctx, create_info)
    }

    // sampler

    /// Create sampler objects and retrieve handles for them.
    pub unsafe fn sampler_create(
        &mut self,
        create_info: sampler::SamplerCreateInfo,
    ) -> sampler::SamplerHandle {
        self.sampler_storage.create(&self.device_ctx, create_info)
    }

    // buffer

    /// Create buffer objects and retrieve handles for them.
    pub unsafe fn buffer_cpu_visible_create<U>(
        &mut self,
        create_info: buffer::CpuVisibleCreateInfo<U>,
    ) -> buffer::Result<buffer::BufferHandle>
    where
        U: Into<gfx::buffer::Usage> + Clone,
    {
        self.buffer_storage
            .cpu_visible_create(&self.device_ctx, create_info)
    }

    pub unsafe fn buffer_device_local_create<U>(
        &mut self,
        create_info: buffer::DeviceLocalCreateInfo<U>,
    ) -> buffer::Result<buffer::BufferHandle>
    where
        U: Into<gfx::buffer::Usage> + Clone,
    {
        self.buffer_storage
            .device_local_create(&self.device_ctx, create_info)
    }

    // vertex attribs

    /// Create new vertex attribute description objects and retrieve handles for them.
    ///
    /// Such handles can be used to specify the vertex input format in a [`GraphicsPassInfo`] for
    /// creating graphics passes in a graph.
    ///
    /// [`GraphicsPassInfo`]: ./graph/pass/struct.GraphicsPassInfo.html
    pub fn vertex_attribs_create(
        &mut self,
        info: vertex_attrib::VertexAttribInfo,
    ) -> vertex_attrib::VertexAttribHandle {
        self.vertex_attrib_storage.create(info)
    }

    /// Destroy vertex attribute description objects.
    pub fn vertex_attribs_destroy(&mut self, handles: &[vertex_attrib::VertexAttribHandle]) {
        self.vertex_attrib_storage.destroy(handles);
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
        self.material_storage.create(&self.device_ctx, create_info)
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
        self.material_storage.write_instance(
            &self.device_ctx,
            &self.sampler_storage,
            &self.image_storage,
            &self.buffer_storage,
            instance,
            data,
        );
    }

    // graph

    /// Create a new graph and retrieve the handle.
    pub fn graph_create(&mut self) -> graph::GraphHandle {
        self.graph_storage.create()
    }

    /// Attach a graphics pass to a graph, adding it into the dependency chain when
    /// compiling the graph.
    pub fn graph_add_graphics_pass<T: Into<graph::PassName>>(
        &mut self,
        graph: graph::GraphHandle,
        name: T,
        info: graph::GraphicsPassInfo,
        pass_impl: impl graph::GraphicsPassImpl + 'static,
    ) {
        self.graph_storage
            .add_graphics_pass(graph, name, info, Box::new(pass_impl));
    }

    /// Attach a compute pass to a graph, adding it into the dependency chain when
    /// compiling the graph.
    pub fn graph_add_compute_pass<T: Into<graph::PassName>>(
        &mut self,
        graph: graph::GraphHandle,
        name: T,
        info: graph::ComputePassInfo,
        pass_impl: impl graph::ComputePassImpl + 'static,
    ) {
        self.graph_storage
            .add_compute_pass(graph, name, info, Box::new(pass_impl));
    }

    /// Add a resource to the *output list* of a graph.
    ///
    /// Output resources can be retrieved using the [`graph_get_output_image`] and
    /// [`graph_get_output_buffer`] methods.
    ///
    /// [`graph_get_output_image`]: #method.graph_get_output_image
    /// [`graph_get_output_buffer`]: #method.graph_get_output_buffer
    pub fn graph_add_output<T: Into<graph::ResourceName>>(
        &mut self,
        graph: graph::GraphHandle,
        name: T,
    ) {
        self.graph_storage.add_output(graph, name);
    }

    /// Compile a graph so it is optimized for execution.
    ///
    /// Compiling a graph is potentially a rather expensive operation, so results are cached when
    /// it makes sense to do so. As a result it is safe to call this method every frame as it will
    /// only perform computations the first time a "new configuration" of graph is encountered.
    ///
    /// The "user facing" graph operates with resource *names* and any dependencies are only
    /// implied, not manifested in a datastructure somewhere, so the first thing to do is to
    /// get all the "unrelated" nodes into a graph structure that has direct or indirect links to
    /// all dependent nodes.
    ///
    /// This representation is then hashed to see if any further work has been done already
    /// in the past.
    ///
    /// Any "backend" resources (pipelines, render passes...) for this graph permutation are
    /// created and cached as well.
    pub fn graph_compile(
        &mut self,
        graph: graph::GraphHandle,
        store: &mut graph::Store,
    ) -> Result<(), Vec<graph::GraphCompileError>> {
        self.graph_storage.compile(store, graph)
    }

    // submit group

    /// Create a new [`SubmitGroup`] to record and execute commands
    ///
    /// [`SubmitGroup`]: ./util/submit_group/struct.SubmitGroup.html
    pub unsafe fn create_submit_group(&self) -> submit_group::SubmitGroup {
        submit_group::SubmitGroup::new(self.device_ctx.clone())
    }

    pub unsafe fn wait_idle(&self) {
        use gfx::Device;
        // TODO handle this error?
        let _ = self.device_ctx.device.wait_idle();
    }
}
