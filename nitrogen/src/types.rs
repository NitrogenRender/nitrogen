/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use back;
use gfx;

pub type Device = <back::Backend as gfx::Backend>::Device;
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
pub type Buffer = <back::Backend as gfx::Backend>::Buffer;
pub type ShaderModule = <back::Backend as gfx::Backend>::ShaderModule;
pub type Semaphore = <back::Backend as gfx::Backend>::Semaphore;
pub type Fence = <back::Backend as gfx::Backend>::Fence;
pub type CommandPool<T> = gfx::CommandPool<back::Backend, T>;
pub type QueueGroup<T> = gfx::QueueGroup<back::Backend, T>;
pub type CommandQueue<T> = gfx::CommandQueue<back::Backend, T>;
