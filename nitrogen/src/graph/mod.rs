use storage::Handle;
use storage::Storage;

pub type Module = ();
pub type Pass = ();

pub struct Graph {
    pub passes: Storage<Pass>,
    pub modules: Storage<Module>,
}

impl Graph {
    pub fn new() -> Graph {
        Graph {
            passes: Storage::new(),
            modules: Storage::new(),
        }
    }

    pub fn create_module(&mut self, module: Module) -> Handle<Module> {
        self.modules.insert(module).0
    }

    pub fn destroy_module(&mut self, module: Handle<Module>) -> bool {
        self.modules.remove(module).is_some()
    }

    pub fn create_pass(&mut self, pass: Pass) -> Handle<Pass> {
        self.passes.insert(pass).0
    }

    pub fn destroy_pass(&mut self, pass: Handle<Pass>) -> bool {
        self.passes.remove(pass).is_some()
    }

    pub fn clear(&mut self) {
        self.modules.clear();
        self.passes.clear();
    }
}
