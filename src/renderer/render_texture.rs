use gfx_hal::{
    buffer, command, format as f,
    format::{AsFormat, ChannelType, Rgba8Srgb as ColorFormat, Swizzle},
    image as i, memory as m, pass,
    pass::Subpass,
    pool,
    prelude::*,
    pso,
    pso::{PipelineStage, ShaderStageFlags, VertexInputRate},
    queue::{QueueGroup, Submission},
    window,
};
use std::mem::ManuallyDrop;
use std::ptr;
use std::iter;
pub struct RenderTexture<B:gfx_hal::Backend> {
    image_logo:ManuallyDrop<B::Image>,
    image_upload_memory: ManuallyDrop<B::Memory>,
}
impl<B: gfx_hal::Backend> RenderTexture<B> {
    pub fn new(
        device: &B::Device,
        command_pool: &mut B::CommandPool,
        queue_group: &mut QueueGroup<B>,

        image_upload_buffer: &mut ManuallyDrop<B::Buffer>,
        row_pitch: u32,
        image_stride: usize,
        height: u32,
        width: u32,
        kind: gfx_hal::image::Kind,
        desc_set: &B::DescriptorSet,
        memory_types: &std::vec::Vec<gfx_hal::adapter::MemoryType>,
        upload_type: gfx_hal::MemoryTypeId,
        image_mem_reqs:gfx_hal::memory::Requirements,
        img:image::RgbaImage,
    ) -> RenderTexture<B> {
        let image_upload_memory = unsafe {
            let memory = device
                .allocate_memory(upload_type, image_mem_reqs.size)
                .unwrap();
            device
                .bind_buffer_memory(&memory, 0, image_upload_buffer)
                .unwrap();
            let mapping = device.map_memory(&memory, m::Segment::ALL).unwrap();
            for y in 0..height as usize {
                let row = &(*img)[y * (width as usize) * image_stride
                    ..(y + 1) * (width as usize) * image_stride];
                ptr::copy_nonoverlapping(
                    row.as_ptr(),
                    mapping.offset(y as isize * row_pitch as isize),
                    width as usize * image_stride,
                );
            }
            device
                .flush_mapped_memory_ranges(iter::once((&memory, m::Segment::ALL)))
                .unwrap();
            device.unmap_memory(&memory);
            ManuallyDrop::new(memory)
        };
        let mut image_logo = ManuallyDrop::new(
            unsafe {
                device.create_image(
                    kind,
                    1,
                    ColorFormat::SELF,
                    i::Tiling::Optimal,
                    i::Usage::TRANSFER_DST | i::Usage::SAMPLED,
                    i::ViewCapabilities::empty(),
                )
            }
            .unwrap(),
        );
        let image_req = unsafe { device.get_image_requirements(&image_logo) };

        let device_type = memory_types
            .iter()
            .enumerate()
            .position(|(id, memory_type)| {
                image_req.type_mask & (1 << id) != 0
                    && memory_type.properties.contains(m::Properties::DEVICE_LOCAL)
            })
            .unwrap()
            .into();
        let image_memory = ManuallyDrop::new(
            unsafe { device.allocate_memory(device_type, image_req.size) }.unwrap(),
        );

        unsafe { device.bind_image_memory(&image_memory, 0, &mut image_logo) }.unwrap();
        let image_srv = ManuallyDrop::new(
            unsafe {
                device.create_image_view(
                    &image_logo,
                    i::ViewKind::D2,
                    ColorFormat::SELF,
                    Swizzle::NO,
                    i::SubresourceRange {
                        aspects: f::Aspects::COLOR,
                        ..Default::default()
                    },
                )
            }
            .unwrap(),
        );

        let sampler = ManuallyDrop::new(
            unsafe {
                device.create_sampler(&i::SamplerDesc::new(i::Filter::Linear, i::WrapMode::Clamp))
            }
            .expect("Can't create sampler"),
        );

        unsafe {
            device.write_descriptor_sets(vec![
                pso::DescriptorSetWrite {
                    set: &*desc_set,
                    binding: 0,
                    array_offset: 0,
                    descriptors: Some(pso::Descriptor::Image(
                        &*image_srv,
                        i::Layout::ShaderReadOnlyOptimal,
                    )),
                },
                pso::DescriptorSetWrite {
                    set: &*desc_set,
                    binding: 1,
                    array_offset: 0,
                    descriptors: Some(pso::Descriptor::Sampler(&*sampler)),
                },
            ]);
        }

        //buffering texture
        let mut copy_fence = device.create_fence(false).expect("Could not create fence");
        unsafe {
            let mut cmd_buffer = command_pool.allocate_one(command::Level::Primary);
            cmd_buffer.begin_primary(command::CommandBufferFlags::ONE_TIME_SUBMIT);

            let image_barrier = m::Barrier::Image {
                states: (i::Access::empty(), i::Layout::Undefined)
                    ..(i::Access::TRANSFER_WRITE, i::Layout::TransferDstOptimal),
                target: &*image_logo,
                families: None,
                range: i::SubresourceRange {
                    aspects: f::Aspects::COLOR,
                    ..Default::default()
                },
            };

            cmd_buffer.pipeline_barrier(
                PipelineStage::TOP_OF_PIPE..PipelineStage::TRANSFER,
                m::Dependencies::empty(),
                &[image_barrier],
            );

            cmd_buffer.copy_buffer_to_image(
                &image_upload_buffer,
                &image_logo,
                i::Layout::TransferDstOptimal,
                &[command::BufferImageCopy {
                    buffer_offset: 0,
                    buffer_width: row_pitch / (image_stride as u32),
                    buffer_height: height as u32,
                    image_layers: i::SubresourceLayers {
                        aspects: f::Aspects::COLOR,
                        level: 0,
                        layers: 0..1,
                    },
                    image_offset: i::Offset { x: 0, y: 0, z: 0 },
                    image_extent: i::Extent {
                        width,
                        height,
                        depth: 1,
                    },
                }],
            );

            let image_barrier = m::Barrier::Image {
                states: (i::Access::TRANSFER_WRITE, i::Layout::TransferDstOptimal)
                    ..(i::Access::SHADER_READ, i::Layout::ShaderReadOnlyOptimal),
                target: &*image_logo,
                families: None,
                range: i::SubresourceRange {
                    aspects: f::Aspects::COLOR,
                    ..Default::default()
                },
            };
            cmd_buffer.pipeline_barrier(
                PipelineStage::TRANSFER..PipelineStage::FRAGMENT_SHADER,
                m::Dependencies::empty(),
                &[image_barrier],
            );

            cmd_buffer.finish();

            queue_group.queues[0]
                .submit_without_semaphores(Some(&cmd_buffer), Some(&mut copy_fence));

            device
                .wait_for_fence(&copy_fence, !0)
                .expect("Can't wait for fence");
        }
        unsafe {
            device.destroy_fence(copy_fence);
        }
        RenderTexture {
            image_logo,
            image_upload_memory,
        }
    }
    pub unsafe fn drop(&mut self,device: &B::Device){
        
        device
            .destroy_image(ManuallyDrop::into_inner(ptr::read(&self.image_logo)));
            device.free_memory(ManuallyDrop::into_inner(ptr::read(
                &self.image_upload_memory,
            )));
        
        }
        
        
}
