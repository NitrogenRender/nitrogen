/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

pub(crate) type GraphicsPipeline = <back::Backend as gfx::Backend>::GraphicsPipeline;
pub(crate) type ComputePipeline = <back::Backend as gfx::Backend>::ComputePipeline;
pub(crate) type PipelineLayout = <back::Backend as gfx::Backend>::PipelineLayout;
pub(crate) type DescriptorSetLayout = <back::Backend as gfx::Backend>::DescriptorSetLayout;
pub(crate) type DescriptorSet = <back::Backend as gfx::Backend>::DescriptorSet;
pub(crate) type DescriptorPool = <back::Backend as gfx::Backend>::DescriptorPool;
pub(crate) type Sampler = <back::Backend as gfx::Backend>::Sampler;
pub(crate) type Swapchain = <back::Backend as gfx::Backend>::Swapchain;
pub(crate) type Surface = <back::Backend as gfx::Backend>::Surface;
pub(crate) type Framebuffer = <back::Backend as gfx::Backend>::Framebuffer;
pub(crate) type RenderPass = <back::Backend as gfx::Backend>::RenderPass;
pub(crate) type Image = <back::Backend as gfx::Backend>::Image;
pub(crate) type ImageView = <back::Backend as gfx::Backend>::ImageView;
pub(crate) type ShaderModule = <back::Backend as gfx::Backend>::ShaderModule;
pub(crate) type Semaphore = <back::Backend as gfx::Backend>::Semaphore;
pub(crate) type CommandPool<T> = gfx::CommandPool<back::Backend, T>;
pub(crate) type QueueGroup<T> = gfx::QueueGroup<back::Backend, T>;
pub(crate) type CommandQueue<T> = gfx::CommandQueue<back::Backend, T>;
pub(crate) type Memory = <back::Backend as gfx::Backend>::Memory;
pub(crate) type Buffer = <back::Backend as gfx::Backend>::Buffer;
