use back;
use gfx;

pub type GraphicsPipeline = <back::Backend as gfx::Backend>::GraphicsPipeline;
pub type PipelineLayout = <back::Backend as gfx::Backend>::PipelineLayout;
pub type DescriptorSetLayout = <back::Backend as gfx::Backend>::DescriptorSetLayout;
pub type DescriptorSet = <back::Backend as gfx::Backend>::DescriptorSet;
pub type DescriptorPool = <back::Backend as gfx::Backend>::DescriptorPool;
pub type Sampler = <back::Backend as gfx::Backend>::Sampler;
pub type Swapchain = <back::Backend as gfx::Backend>::Swapchain;
pub type Surface = <back::Backend as gfx::Backend>::Surface;
pub type Framebuffer = <back::Backend as gfx::Backend>::Framebuffer;
pub type RenderPass = <back::Backend as gfx::Backend>::RenderPass;
pub type Image = <back::Backend as gfx::Backend>::Image;
pub type ImageView = <back::Backend as gfx::Backend>::ImageView;
pub type CommandPool = <back::Backend as gfx::Backend>::CommandPool;
pub type ShaderModule = <back::Backend as gfx::Backend>::ShaderModule;
