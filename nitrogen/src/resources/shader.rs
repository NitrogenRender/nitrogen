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
}

impl ShaderStorage {
    pub(crate) fn new() -> Self {
        ShaderStorage {
            compute_storage: Storage::new(),
        }
    }

    pub(crate) fn create_compute_shader(&mut self, info: ShaderInfo<'_>) -> ComputeShaderHandle {
        let shader = Shader {
            spirv_content: info.spirv_content.to_owned(),
            entry_point: info.entry_point.clone(),

            _marker: PhantomData,
        };

        self.compute_storage.insert(shader)
    }

    pub(crate) fn destroy_compute_shader(&mut self, handle: ComputeShaderHandle) {
        if let Some(shader) = self.compute_storage.remove(handle) {
            drop(shader);
        }
    }

    pub(crate) fn raw_compute(&self, handle: ComputeShaderHandle) -> Option<&Shader<Compute>> {
        self.compute_storage.get(handle)
    }
}
