use crate::util::storage::{Handle, Storage};
use crate::util::CowString;
use std::marker::PhantomData;

pub type EntryPoint = CowString;

pub struct ShaderInfo<'a> {
    pub spirv_content: &'a [u8],
    pub entry_point: EntryPoint,
}

pub struct Shader<T> {
    pub spirv_content: Vec<u8>,
    pub entry_point: EntryPoint,

    pub(crate) _marker: PhantomData<T>,
}

pub struct Compute;
pub struct Vertex;
pub struct Fragment;
pub struct Geometry;

pub type ComputeShaderHandle = Handle<Shader<Compute>>;
pub type VertexShaderHandle = Handle<Shader<Vertex>>;
pub type FragmentShaderHandle = Handle<Shader<Fragment>>;
pub type GeometryShaderHandle = Handle<Shader<Geometry>>;

pub struct VertexShader {}

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
