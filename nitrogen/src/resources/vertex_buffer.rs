use storage::{Handle, Storage};

pub struct VertexBuffer;

pub struct VertexBufferStorage {
    storage: Storage<VertexBuffer>,
}

impl VertexBufferStorage {
    pub fn new() -> Self {
        VertexBufferStorage {
            storage: Storage::new(),
        }
    }
}
