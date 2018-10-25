use back;
use gfx;
use std;

use gfx::Device;

use buffer::BufferTypeInternal;

use image::ImageType;

use device::DeviceContext;

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


pub struct TransferContext {
    command_pool: gfx::CommandPool<back::Backend, gfx::General>,
}

impl TransferContext {
    pub fn new(device: &DeviceContext) -> Self {

        let command_pool = {
            let queue_group = device.queue_group();

            device.device.create_command_pool_typed(
                &queue_group,
                gfx::pool::CommandPoolCreateFlags::TRANSIENT,
                1,
            ).expect("Can't create command pool")
        };

        TransferContext {
            command_pool,
        }
    }

    pub fn release(self, device: &DeviceContext) {
        device.device.destroy_command_pool(self.command_pool.into_raw());
    }


    pub fn copy_buffers(
        &mut self,
        device: &DeviceContext,
        buffers: &[BufferTransfer],
    ) {
        use gfx::pso::PipelineStage;
        use gfx::buffer::Access;

        let submit = {
            let mut cmd = self.command_pool.acquire_command_buffer(false);

            for buffer_transfer in buffers {

                let entry_barrier = gfx::memory::Barrier::Buffer {
                    states: Access::empty()..Access::TRANSFER_WRITE,
                    target: buffer_transfer.dst.raw(),
                };

                cmd.pipeline_barrier(
                    PipelineStage::TOP_OF_PIPE..PipelineStage::TRANSFER,
                    gfx::memory::Dependencies::empty(),
                    &[entry_barrier]
                );

                cmd.copy_buffer(
                    buffer_transfer.src.raw(),
                    buffer_transfer.dst.raw(),
                    &[
                        gfx::command::BufferCopy {
                            src: 0,
                            dst: buffer_transfer.offset,
                            size: buffer_transfer.data.len() as u64,
                        }
                    ]
                );

                let exit_barrier = gfx::memory::Barrier::Buffer {
                    states: Access::TRANSFER_WRITE..Access::SHADER_READ,
                    target: buffer_transfer.dst.raw(),
                };

                cmd.pipeline_barrier(
                    PipelineStage::TRANSFER..PipelineStage::BOTTOM_OF_PIPE,
                    gfx::memory::Dependencies::empty(),
                    &[exit_barrier]
                );
            }

            cmd.finish()
        };

        let fence = device.device.create_fence(false).expect("can't create submission fence");

        {
            let submission = gfx::Submission::new()
                .submit(std::iter::once(submit));

            let mut queue_group = device.queue_group();
            queue_group.queues[0].submit(submission, Some(&fence));
        }

        device.device.wait_for_fence(&fence, !0);
        device.device.destroy_fence(fence);

        self.command_pool.reset();
    }

    pub fn copy_buffers_to_images(
        &mut self,
        device: &DeviceContext,
        images: &[BufferImageTransfer],
    ) {
        use gfx::memory::Barrier;
        use gfx::image::Access;
        use gfx::image::Layout;
        use gfx::pso::PipelineStage;

        let submit = {
            let mut cmd = self.command_pool.acquire_command_buffer(false);

            for transfer in images {

                let entry_barrier = Barrier::Image {
                    states: (Access::empty(), Layout::Undefined)
                        .. (Access::TRANSFER_WRITE, Layout::TransferDstOptimal),
                    target: transfer.dst.raw(),
                    range: transfer.subresource_range.clone(),
                };

                cmd.pipeline_barrier(
                    PipelineStage::TOP_OF_PIPE .. PipelineStage::TRANSFER,
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
                        .. (Access::MEMORY_READ, Layout::General),
                    target: transfer.dst.raw(),
                    range: transfer.subresource_range.clone(),
                };

                cmd.pipeline_barrier(
                    PipelineStage::TRANSFER .. PipelineStage::BOTTOM_OF_PIPE,
                    gfx::memory::Dependencies::empty(),
                    &[exit_barrier],
                );

            }

            cmd.finish()
        };

        let fence = device.device.create_fence(false).expect("Can't create submission fence");

        {
            let submission = gfx::Submission::new()
                .submit(std::iter::once(submit));

            let mut queue_group = device.queue_group();
            queue_group.queues[0].submit(submission, Some(&fence));
        }

        device.device.wait_for_fence(&fence, !0);
        device.device.destroy_fence(fence);

        self.command_pool.reset();

    }
}
