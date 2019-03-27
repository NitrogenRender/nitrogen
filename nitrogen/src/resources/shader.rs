/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Storage for shader programs.

use crate::util::storage::{Handle, Storage};
use crate::util::CowString;
use std::marker::PhantomData;

/// Name of the "entry point" of a shader program.
///
/// The entry point is the name of the function which will be invoked during
/// the shader execution.
pub type EntryPoint = CowString;

/// Information describing a shader program.
pub struct ShaderInfo<'a> {
    /// The compiled SPIR-V code of the shader.
    pub spirv_content: &'a [u8],

    /// The entry point (function name) of the shader program.
    pub entry_point: EntryPoint,
}

/// A strongly typed shader program resource.
///
/// The type parameter `T` denotes the shader type.
pub struct Shader<T> {
    pub(crate) spirv_content: Vec<u8>,
    pub(crate) entry_point: EntryPoint,

    pub(crate) _marker: PhantomData<T>,
}

/// Type denoting a compute shader program.
pub struct Compute;
/// Type denoting a vertex shader program.
pub struct Vertex;
/// Type denoting a fragment shader program.
pub struct Fragment;
/// Type denoting a geometry shader program.
pub struct Geometry;

/// Opaque handle to a compute shader program resource.
pub type ComputeShaderHandle = Handle<Shader<Compute>>;
/// Opaque handle to a vertex shader program resource.
pub type VertexShaderHandle = Handle<Shader<Vertex>>;
/// Opaque handle to a fragment shader program resource.
pub type FragmentShaderHandle = Handle<Shader<Fragment>>;
/// Opaque handle to a geometry shader program resource.
pub type GeometryShaderHandle = Handle<Shader<Geometry>>;

pub(crate) struct ShaderStorage {
    pub(crate) compute_storage: Storage<Shader<Compute>>,
    pub(crate) vertex_storage: Storage<Shader<Vertex>>,
    pub(crate) fragment_storage: Storage<Shader<Fragment>>,
    pub(crate) geometry_storage: Storage<Shader<Geometry>>,
}

impl ShaderStorage {
    pub(crate) fn new() -> Self {
        ShaderStorage {
            compute_storage: Storage::new(),
            vertex_storage: Storage::new(),
            fragment_storage: Storage::new(),
            geometry_storage: Storage::new(),
        }
    }

    fn create_shader<T>(
        storage: &mut Storage<Shader<T>>,
        info: ShaderInfo<'_>,
    ) -> Handle<Shader<T>> {
        let shader = Shader {
            spirv_content: info.spirv_content.to_owned(),
            entry_point: info.entry_point.clone(),

            _marker: PhantomData,
        };

        storage.insert(shader)
    }

    fn destroy_shader<T>(storage: &mut Storage<Shader<T>>, handle: Handle<Shader<T>>) {
        if let Some(shader) = storage.remove(handle) {
            // TODO some de-initialization behavior?
            drop(shader);
        }
    }

    fn raw<T>(storage: &Storage<Shader<T>>, handle: Handle<Shader<T>>) -> Option<&Shader<T>> {
        storage.get(handle)
    }

    // compute

    pub(crate) fn create_compute_shader(&mut self, info: ShaderInfo<'_>) -> ComputeShaderHandle {
        ShaderStorage::create_shader(&mut self.compute_storage, info)
    }

    pub(crate) fn destroy_compute_shader(&mut self, handle: ComputeShaderHandle) {
        ShaderStorage::destroy_shader(&mut self.compute_storage, handle);
    }

    pub(crate) fn raw_compute(&self, handle: ComputeShaderHandle) -> Option<&Shader<Compute>> {
        ShaderStorage::raw(&self.compute_storage, handle)
    }

    // vertex

    pub(crate) fn create_vertex_shader(&mut self, info: ShaderInfo<'_>) -> VertexShaderHandle {
        ShaderStorage::create_shader(&mut self.vertex_storage, info)
    }

    pub(crate) fn destroy_vertex_shader(&mut self, handle: VertexShaderHandle) {
        ShaderStorage::destroy_shader(&mut self.vertex_storage, handle);
    }

    pub(crate) fn raw_vertex(&self, handle: VertexShaderHandle) -> Option<&Shader<Vertex>> {
        ShaderStorage::raw(&self.vertex_storage, handle)
    }

    // fragment

    pub(crate) fn create_fragment_shader(&mut self, info: ShaderInfo<'_>) -> FragmentShaderHandle {
        ShaderStorage::create_shader(&mut self.fragment_storage, info)
    }

    pub(crate) fn destroy_fragment_shader(&mut self, handle: FragmentShaderHandle) {
        ShaderStorage::destroy_shader(&mut self.fragment_storage, handle);
    }

    pub(crate) fn raw_fragment(&self, handle: FragmentShaderHandle) -> Option<&Shader<Fragment>> {
        ShaderStorage::raw(&self.fragment_storage, handle)
    }

    // geometry

    pub(crate) fn create_geometry_shader(&mut self, info: ShaderInfo<'_>) -> GeometryShaderHandle {
        ShaderStorage::create_shader(&mut self.geometry_storage, info)
    }

    pub(crate) fn destroy_geometry_shader(&mut self, handle: GeometryShaderHandle) {
        ShaderStorage::destroy_shader(&mut self.geometry_storage, handle);
    }

    pub(crate) fn raw_geometry(&self, handle: GeometryShaderHandle) -> Option<&Shader<Geometry>> {
        ShaderStorage::raw(&self.geometry_storage, handle)
    }
}
