use graph;

pub struct GraphBuilder {}

impl GraphBuilder {
    pub fn enable(&mut self) {
        println!("builder.build() !!!");
    }

    pub fn create_image(&mut self, name: &str, create_info: graph::ImageCreateInfo) {}

    pub fn write_image(&mut self, name: &str) {}
}
