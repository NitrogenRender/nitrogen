/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use std;

use crate::buffer::BufferTypeInternal;

use crate::image::ImageType;

use crate::device::DeviceContext;
use crate::resources::command_pool::CommandPoolTransfer;
use crate::resources::semaphore_pool::SemaphoreList;
use crate::resources::semaphore_pool::SemaphorePool;

pub(crate) struct BufferTransfer<'a> {
    pub(crate) src: &'a BufferTypeInternal,
    pub(crate) dst: &'a BufferTypeInternal,
    pub(crate) offset: u64,
    pub(crate) data: &'a [u8],
}

pub(crate) struct BufferImageTransfer<'a> {
    pub(crate) src: &'a BufferTypeInternal,
    pub(crate) dst: &'a ImageType,

    pub(crate) subresource_range: gfx::image::SubresourceRange,
    pub(crate) copy_information: gfx::command::BufferImageCopy,
}

pub(crate) unsafe fn copy_buffers<'a, T>(
    device: &DeviceContext,
    sem_pool: &SemaphorePool,
    sem_list: &mut SemaphoreList,
    cmd_pool: &CommandPoolTransfer,
    buffers: T,
) where
    T: 'a,
    T: IntoIterator,
    T::Item: std::borrow::Borrow<BufferTransfer<'a>>,
{
    use gfx::buffer::Access;
    use gfx::pso::PipelineStage;
    use std::borrow::Borrow;

    let submit = {
        let mut cmd = cmd_pool.alloc();

        for buffer_transfer in buffers.into_iter() {
            let buffer_transfer = buffer_transfer.borrow();

            let entry_barrier = gfx::memory::Barrier::Buffer {
                states: Access::empty()..Access::TRANSFER_WRITE,
                families: None,
                target: buffer_transfer.dst.raw(),
                range: None..None,
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
                families: None,
                target: buffer_transfer.dst.raw(),
                range: None..None,
            };

            cmd.pipeline_barrier(
                PipelineStage::TRANSFER..PipelineStage::BOTTOM_OF_PIPE,
                gfx::memory::Dependencies::empty(),
                &[exit_barrier],
            );
        }

        cmd.finish();
        cmd
    };

    let sem = sem_pool.alloc();
    sem_list.add_next_semaphore(sem);

    {
        let submission = gfx::Submission {
            command_buffers: Some(&*submit),
            wait_semaphores: sem_pool
                .list_prev_sems(sem_list)
                .map(|sem| (sem, gfx::pso::PipelineStage::BOTTOM_OF_PIPE)),
            signal_semaphores: sem_pool.list_next_sems(sem_list),
        };
        device.transfer_queue().submit(submission, None);
    }

    sem_list.advance();
}

pub(crate) unsafe fn copy_buffers_to_images(
    device: &DeviceContext,
    sem_pool: &SemaphorePool,
    sem_list: &mut SemaphoreList,
    cmd_pool: &CommandPoolTransfer,
    images: &[BufferImageTransfer],
) {
    use gfx::image::Access;
    use gfx::image::Layout;
    use gfx::memory::Barrier;
    use gfx::pso::PipelineStage;

    let submit = {
        let mut cmd = cmd_pool.alloc();

        for transfer in images {
            let entry_barrier = Barrier::Image {
                states: (Access::empty(), Layout::Undefined)
                    ..(Access::TRANSFER_WRITE, Layout::TransferDstOptimal),
                target: transfer.dst.raw(),
                families: None,
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
                families: None,
                range: transfer.subresource_range.clone(),
            };

            cmd.pipeline_barrier(
                PipelineStage::TRANSFER..PipelineStage::BOTTOM_OF_PIPE,
                gfx::memory::Dependencies::empty(),
                &[exit_barrier],
            );
        }

        cmd.finish();
        cmd
    };

    let sem = sem_pool.alloc();
    sem_list.add_next_semaphore(sem);

    {
        let submission = gfx::Submission {
            command_buffers: Some(&*submit),
            wait_semaphores: sem_pool
                .list_prev_sems(sem_list)
                .map(|sem| (sem, gfx::pso::PipelineStage::BOTTOM_OF_PIPE)),
            signal_semaphores: sem_pool.list_next_sems(sem_list),
        };

        device.transfer_queue().submit(submission, None);
    }

    sem_list.advance();
}
