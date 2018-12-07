/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use gfx;
use std;


use buffer::BufferTypeInternal;

use image::ImageType;

use device::DeviceContext;
use resources::semaphore_pool::SemaphoreList;
use resources::semaphore_pool::SemaphorePool;
use types::CommandPool;

pub struct BufferTransfer<'a> {
    pub src: &'a BufferTypeInternal,
    pub dst: &'a BufferTypeInternal,
    pub offset: u64,
    pub data: &'a [u8],
}

pub struct BufferImageTransfer<'a> {
    pub src: &'a BufferTypeInternal,
    pub dst: &'a ImageType,

    pub subresource_range: gfx::image::SubresourceRange,
    pub copy_information: gfx::command::BufferImageCopy,
}

pub struct TransferContext;

impl TransferContext {
    pub fn new() -> Self {
        TransferContext
    }

    pub fn release(self) {}

    pub fn copy_buffers(
        &self,
        device: &DeviceContext,
        sem_pool: &SemaphorePool,
        sem_list: &mut SemaphoreList,
        cmd_pool: &mut CommandPool<gfx::Transfer>,
        buffers: &[BufferTransfer],
    ) {
        use gfx::buffer::Access;
        use gfx::pso::PipelineStage;

        let submit = {
            let mut cmd = cmd_pool.acquire_command_buffer(false);

            for buffer_transfer in buffers {
                let entry_barrier = gfx::memory::Barrier::Buffer {
                    states: Access::empty()..Access::TRANSFER_WRITE,
                    target: buffer_transfer.dst.raw(),
                };

                cmd.pipeline_barrier(
                    PipelineStage::TOP_OF_PIPE..PipelineStage::TRANSFER,
                    gfx::memory::Dependencies::empty(),
                    &[entry_barrier],
                );

                cmd.copy_buffer(
                    buffer_transfer.src.raw(),
                    buffer_transfer.dst.raw(),
                    &[gfx::command::BufferCopy {
                        src: 0,
                        dst: buffer_transfer.offset,
                        size: buffer_transfer.data.len() as u64,
                    }],
                );

                let exit_barrier = gfx::memory::Barrier::Buffer {
                    states: Access::TRANSFER_WRITE..Access::SHADER_READ,
                    target: buffer_transfer.dst.raw(),
                };

                cmd.pipeline_barrier(
                    PipelineStage::TRANSFER..PipelineStage::BOTTOM_OF_PIPE,
                    gfx::memory::Dependencies::empty(),
                    &[exit_barrier],
                );
            }

            cmd.finish()
        };

        let sem = sem_pool.alloc();
        sem_list.add_next_semaphore(sem);

        {
            let submission = gfx::Submission::new()
                .wait_on(
                    sem_pool
                        .list_prev_sems(sem_list)
                        .map(|sem| (sem, gfx::pso::PipelineStage::BOTTOM_OF_PIPE)),
                )
                .signal(sem_pool.list_next_sems(sem_list))
                .submit(std::iter::once(submit));

            device.transfer_queue().submit(submission, None);
        }

        sem_list.advance();
    }

    pub fn copy_buffers_to_images(
        &self,
        device: &DeviceContext,
        sem_pool: &SemaphorePool,
        sem_list: &mut SemaphoreList,
        cmd_pool: &mut CommandPool<gfx::Transfer>,
        images: &[BufferImageTransfer],
    ) {
        use gfx::image::Access;
        use gfx::image::Layout;
        use gfx::memory::Barrier;
        use gfx::pso::PipelineStage;

        let submit = {
            let mut cmd = cmd_pool.acquire_command_buffer(false);

            for transfer in images {
                let entry_barrier = Barrier::Image {
                    states: (Access::empty(), Layout::Undefined)
                        ..(Access::TRANSFER_WRITE, Layout::TransferDstOptimal),
                    target: transfer.dst.raw(),
                    range: transfer.subresource_range.clone(),
                };

                cmd.pipeline_barrier(
                    PipelineStage::TOP_OF_PIPE..PipelineStage::TRANSFER,
                    gfx::memory::Dependencies::empty(),
                    &[entry_barrier],
                );

                cmd.copy_buffer_to_image(
                    transfer.src.raw(),
                    transfer.dst.raw(),
                    Layout::TransferDstOptimal,
                    &[transfer.copy_information.clone()],
                );

                let exit_barrier = Barrier::Image {
                    states: (Access::TRANSFER_WRITE, Layout::TransferDstOptimal)
                        ..(Access::MEMORY_READ, Layout::General),
                    target: transfer.dst.raw(),
                    range: transfer.subresource_range.clone(),
                };

                cmd.pipeline_barrier(
                    PipelineStage::TRANSFER..PipelineStage::BOTTOM_OF_PIPE,
                    gfx::memory::Dependencies::empty(),
                    &[exit_barrier],
                );
            }

            cmd.finish()
        };

        let sem = sem_pool.alloc();
        sem_list.add_next_semaphore(sem);

        {
            let submission = gfx::Submission::new()
                .wait_on(
                    sem_pool
                        .list_prev_sems(sem_list)
                        .map(|sem| (sem, gfx::pso::PipelineStage::BOTTOM_OF_PIPE)),
                )
                .signal(sem_pool.list_next_sems(sem_list))
                .submit(std::iter::once(submit));

            device.transfer_queue().submit(submission, None);
        }

        sem_list.advance();
    }
}
