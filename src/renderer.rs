use gfx_hal::{
    buffer, command, format as f,
    format::ChannelType,
    image as i, memory as m, pass,
    pass::Subpass,
    pool,
    prelude::*,
    pso,
    pso::{ShaderStageFlags, VertexInputRate},
    queue::{QueueGroup, Submission},
    window,
};

use std::{
    borrow::Borrow,
    iter,
    mem::{self, ManuallyDrop},
    ptr,
};
mod render_texture;
use render_texture::RenderTexture;
#[derive(Debug, Clone, Copy)]
#[allow(non_snake_case)]
struct Vertex {
    a_Pos: [f32; 2],
    a_Uv: [f32; 2],
}
#[cfg_attr(rustfmt, rustfmt_skip)]
pub const DIMS: window::Extent2D = window::Extent2D { width: 1024, height: 768 };
#[cfg_attr(rustfmt, rustfmt_skip)]
const QUAD: [Vertex; 6] = [
    Vertex { a_Pos: [ -1.0, 1.0 ], a_Uv: [0.0, 1.0] },
    Vertex { a_Pos: [  1.0, 1.0 ], a_Uv: [1.0, 1.0] },
    Vertex { a_Pos: [  1.0,-1.0 ], a_Uv: [1.0, 0.0] },

    Vertex { a_Pos: [ -1.0, 1.0 ], a_Uv: [0.0, 1.0] },
    Vertex { a_Pos: [  1.0,-1.0 ], a_Uv: [1.0, 0.0] },
    Vertex { a_Pos: [ -1.0,-1.0 ], a_Uv: [0.0, 0.0] },
];
const ENTRY_NAME: &str = "main";
pub struct Renderer<B: gfx_hal::Backend> {
    instance: Option<B::Instance>,
    device: B::Device,
    queue_group: QueueGroup<B>,
    desc_pool: ManuallyDrop<B::DescriptorPool>,
    surface: ManuallyDrop<B::Surface>,
    adapter: gfx_hal::adapter::Adapter<B>,
    format: gfx_hal::format::Format,
    pub dimensions: window::Extent2D,
    viewport: pso::Viewport,
    render_pass: ManuallyDrop<B::RenderPass>,
    pipeline: ManuallyDrop<B::GraphicsPipeline>,
    pipeline_layout: ManuallyDrop<B::PipelineLayout>,
    desc_set: B::DescriptorSet,
    set_layout: ManuallyDrop<B::DescriptorSetLayout>,
    submission_complete_semaphores: Vec<B::Semaphore>,
    submission_complete_fences: Vec<B::Fence>,
    cmd_pools: Vec<B::CommandPool>,
    cmd_buffers: Vec<B::CommandBuffer>,
    vertex_buffer: ManuallyDrop<B::Buffer>,
    //image_upload_buffer: ManuallyDrop<B::Buffer>,
    //image_logo: ManuallyDrop<B::Image>,
    //image_srv: ManuallyDrop<B::ImageView>,
    buffer_memory: ManuallyDrop<B::Memory>,
    //image_memory: ManuallyDrop<B::Memory>,
    //image_upload_memory: ManuallyDrop<B::Memory>,
    //sampler: ManuallyDrop<B::Sampler>,
    frames_in_flight: usize,
    frame: u64,
    render_texture: RenderTexture<B>,
}
pub trait Updater {
    fn update(&mut self, image: &mut image::RgbaImage);
}
impl<B> Renderer<B>
where
    B: gfx_hal::Backend,
{
    pub fn new(
        instance: Option<B::Instance>,
        mut surface: B::Surface,
        adapter: gfx_hal::adapter::Adapter<B>,
    ) -> Renderer<B> {
        let memory_types = adapter.physical_device.memory_properties().memory_types;
        let limits = adapter.physical_device.limits();

        // Build a new device and associated command queues
        let family = adapter
            .queue_families
            .iter()
            .find(|family| {
                surface.supports_queue_family(family) && family.queue_type().supports_graphics()
            })
            .unwrap();
        let mut gpu = unsafe {
            adapter
                .physical_device
                .open(&[(family, &[1.0])], gfx_hal::Features::empty())
                .unwrap()
        };
        let mut queue_group = gpu.queue_groups.pop().unwrap();
        let device = gpu.device;

        let command_pool = unsafe {
            device.create_command_pool(queue_group.family, pool::CommandPoolCreateFlags::empty())
        }
        .expect("Can't create command pool");

        // Setup renderpass and pipeline
        let set_layout = ManuallyDrop::new(
            unsafe {
                device.create_descriptor_set_layout(
                    &[
                        pso::DescriptorSetLayoutBinding {
                            binding: 0,
                            ty: pso::DescriptorType::Image {
                                ty: pso::ImageDescriptorType::Sampled {
                                    with_sampler: false,
                                },
                            },
                            count: 1,
                            stage_flags: ShaderStageFlags::FRAGMENT,
                            immutable_samplers: false,
                        },
                        pso::DescriptorSetLayoutBinding {
                            binding: 1,
                            ty: pso::DescriptorType::Sampler,
                            count: 1,
                            stage_flags: ShaderStageFlags::FRAGMENT,
                            immutable_samplers: false,
                        },
                    ],
                    &[],
                )
            }
            .expect("Can't create descriptor set layout"),
        );

        // Descriptors
        let mut desc_pool = ManuallyDrop::new(
            unsafe {
                device.create_descriptor_pool(
                    1, // sets
                    &[
                        pso::DescriptorRangeDesc {
                            ty: pso::DescriptorType::Image {
                                ty: pso::ImageDescriptorType::Sampled {
                                    with_sampler: false,
                                },
                            },
                            count: 1,
                        },
                        pso::DescriptorRangeDesc {
                            ty: pso::DescriptorType::Sampler,
                            count: 1,
                        },
                    ],
                    pso::DescriptorPoolCreateFlags::empty(),
                )
            }
            .expect("Can't create descriptor pool"),
        );
        let desc_set = unsafe { desc_pool.allocate_set(&set_layout) }.unwrap();

        // Buffer allocations
        println!("Memory types: {:?}", memory_types);
        let non_coherent_alignment = limits.non_coherent_atom_size as u64;

        let buffer_stride = mem::size_of::<Vertex>() as u64;
        let buffer_len = QUAD.len() as u64 * buffer_stride;
        assert_ne!(buffer_len, 0);
        let padded_buffer_len = ((buffer_len + non_coherent_alignment - 1)
            / non_coherent_alignment)
            * non_coherent_alignment;

        let mut vertex_buffer = ManuallyDrop::new(
            unsafe { device.create_buffer(padded_buffer_len, buffer::Usage::VERTEX) }.unwrap(),
        );

        let buffer_req = unsafe { device.get_buffer_requirements(&vertex_buffer) };

        let upload_type = memory_types
            .iter()
            .enumerate()
            .position(|(id, mem_type)| {
                // type_mask is a bit field where each bit represents a memory type. If the bit is set
                // to 1 it means we can use that type for our buffer. So this code finds the first
                // memory type that has a `1` (or, is allowed), and is visible to the CPU.
                buffer_req.type_mask & (1 << id) != 0
                    && mem_type.properties.contains(m::Properties::CPU_VISIBLE)
            })
            .unwrap()
            .into();
        // TODO: check transitions: read/write mapping and vertex buffer read
        let buffer_memory = unsafe {
            let memory = device
                .allocate_memory(upload_type, buffer_req.size)
                .unwrap();
            device
                .bind_buffer_memory(&memory, 0, &mut vertex_buffer)
                .unwrap();
            let mapping = device.map_memory(&memory, m::Segment::ALL).unwrap();
            ptr::copy_nonoverlapping(QUAD.as_ptr() as *const u8, mapping, buffer_len as usize);
            device
                .flush_mapped_memory_ranges(iter::once((&memory, m::Segment::ALL)))
                .unwrap();
            device.unmap_memory(&memory);
            ManuallyDrop::new(memory)
        };

        let caps = surface.capabilities(&adapter.physical_device);
        let formats = surface.supported_formats(&adapter.physical_device);
        println!("formats: {:?}", formats);
        let format = formats.map_or(f::Format::Rgba8Srgb, |formats| {
            formats
                .iter()
                .find(|format| format.base_format().1 == ChannelType::Srgb)
                .map(|format| *format)
                .unwrap_or(formats[0])
        });

        let swap_config = window::SwapchainConfig::from_caps(&caps, format, DIMS);
        println!("{:?}", swap_config);
        let extent = swap_config.extent;
        unsafe {
            surface
                .configure_swapchain(&device, swap_config)
                .expect("Can't configure swapchain");
        };

        let render_pass = {
            let attachment = pass::Attachment {
                format: Some(format),
                samples: 1,
                ops: pass::AttachmentOps::new(
                    pass::AttachmentLoadOp::Clear,
                    pass::AttachmentStoreOp::Store,
                ),
                stencil_ops: pass::AttachmentOps::DONT_CARE,
                layouts: i::Layout::Undefined..i::Layout::Present,
            };

            let subpass = pass::SubpassDesc {
                colors: &[(0, i::Layout::ColorAttachmentOptimal)],
                depth_stencil: None,
                inputs: &[],
                resolves: &[],
                preserves: &[],
            };

            ManuallyDrop::new(
                unsafe { device.create_render_pass(&[attachment], &[subpass], &[]) }
                    .expect("Can't create render pass"),
            )
        };

        // Define maximum number of frames we want to be able to be "in flight" (being computed
        // simultaneously) at once
        let frames_in_flight = 3;

        // The number of the rest of the resources is based on the frames in flight.
        let mut submission_complete_semaphores = Vec::with_capacity(frames_in_flight);
        let mut submission_complete_fences = Vec::with_capacity(frames_in_flight);
        // Note: We don't really need a different command pool per frame in such a simple demo like this,
        // but in a more 'real' application, it's generally seen as optimal to have one command pool per
        // thread per frame. There is a flag that lets a command pool reset individual command buffers
        // which are created from it, but by default the whole pool (and therefore all buffers in it)
        // must be reset at once. Furthermore, it is often the case that resetting a whole pool is actually
        // faster and more efficient for the hardware than resetting individual command buffers, so it's
        // usually best to just make a command pool for each set of buffers which need to be reset at the
        // same time (each frame). In our case, each pool will only have one command buffer created from it,
        // though.
        let mut cmd_pools = Vec::with_capacity(frames_in_flight);
        let mut cmd_buffers = Vec::with_capacity(frames_in_flight);

        cmd_pools.push(command_pool);
        for _ in 1..frames_in_flight {
            unsafe {
                cmd_pools.push(
                    device
                        .create_command_pool(
                            queue_group.family,
                            pool::CommandPoolCreateFlags::empty(),
                        )
                        .expect("Can't create command pool"),
                );
            }
        }

        for i in 0..frames_in_flight {
            submission_complete_semaphores.push(
                device
                    .create_semaphore()
                    .expect("Could not create semaphore"),
            );
            submission_complete_fences
                .push(device.create_fence(true).expect("Could not create fence"));
            cmd_buffers.push(unsafe { cmd_pools[i].allocate_one(command::Level::Primary) });
        }

        let pipeline_layout = ManuallyDrop::new(
            unsafe {
                device.create_pipeline_layout(
                    iter::once(&*set_layout),
                    &[(pso::ShaderStageFlags::VERTEX, 0..8)],
                )
            }
            .expect("Can't create pipeline layout"),
        );
        let pipeline = {
            let vs_module = {
                let glsl = include_str!("data/quad.vert");
                let file =
                    glsl_to_spirv::compile(&glsl, glsl_to_spirv::ShaderType::Vertex).unwrap();
                let spirv = gfx_auxil::read_spirv(file).unwrap();
                unsafe { device.create_shader_module(&spirv) }.unwrap()
            };
            let fs_module = {
                let glsl = include_str!("data/quad.frag");
                let file =
                    glsl_to_spirv::compile(&glsl, glsl_to_spirv::ShaderType::Fragment).unwrap();
                let spirv = gfx_auxil::read_spirv(file).unwrap();
                unsafe { device.create_shader_module(&spirv) }.unwrap()
            };

            let pipeline = {
                let (vs_entry, fs_entry) = (
                    pso::EntryPoint {
                        entry: ENTRY_NAME,
                        module: &vs_module,
                        specialization: gfx_hal::spec_const_list![0.8f32],
                    },
                    pso::EntryPoint {
                        entry: ENTRY_NAME,
                        module: &fs_module,
                        specialization: pso::Specialization::default(),
                    },
                );

                let subpass = Subpass {
                    index: 0,
                    main_pass: &*render_pass,
                };

                let vertex_buffers = vec![pso::VertexBufferDesc {
                    binding: 0,
                    stride: mem::size_of::<Vertex>() as u32,
                    rate: VertexInputRate::Vertex,
                }];

                let attributes = vec![
                    pso::AttributeDesc {
                        location: 0,
                        binding: 0,
                        element: pso::Element {
                            format: f::Format::Rg32Sfloat,
                            offset: 0,
                        },
                    },
                    pso::AttributeDesc {
                        location: 1,
                        binding: 0,
                        element: pso::Element {
                            format: f::Format::Rg32Sfloat,
                            offset: 8,
                        },
                    },
                ];

                let mut pipeline_desc = pso::GraphicsPipelineDesc::new(
                    pso::PrimitiveAssemblerDesc::Vertex {
                        buffers: &vertex_buffers,
                        attributes: &attributes,
                        input_assembler: pso::InputAssemblerDesc {
                            primitive: pso::Primitive::TriangleList,
                            with_adjacency: false,
                            restart_index: None,
                        },
                        vertex: vs_entry,
                        geometry: None,
                        tessellation: None,
                    },
                    pso::Rasterizer::FILL,
                    Some(fs_entry),
                    &*pipeline_layout,
                    subpass,
                );

                pipeline_desc.blender.targets.push(pso::ColorBlendDesc {
                    mask: pso::ColorMask::ALL,
                    blend: Some(pso::BlendState::ALPHA),
                });

                unsafe { device.create_graphics_pipeline(&pipeline_desc, None) }
            };

            unsafe {
                device.destroy_shader_module(vs_module);
            }
            unsafe {
                device.destroy_shader_module(fs_module);
            }

            ManuallyDrop::new(pipeline.unwrap())
        };

        // Rendering setup
        let viewport = pso::Viewport {
            rect: pso::Rect {
                x: 0,
                y: 0,
                w: extent.width as _,
                h: extent.height as _,
            },
            depth: 0.0..1.0,
        };
        let render_texture = RenderTexture::new(
            &device,
            &mut cmd_pools[0],
            &mut queue_group,
            &desc_set,
            &memory_types,
            upload_type,
            limits,
            (extent.width, extent.height),
        );
        Renderer {
            instance,
            device,
            queue_group,
            desc_pool,
            surface: ManuallyDrop::new(surface),
            adapter,
            format,
            dimensions: DIMS,
            viewport,
            render_pass,
            pipeline,
            pipeline_layout,
            desc_set,
            set_layout,
            submission_complete_semaphores,
            submission_complete_fences,
            cmd_pools,
            cmd_buffers,
            vertex_buffer,
            //image_upload_buffer,
            //image_logo,
            //image_srv,
            buffer_memory,
            //image_memory,
            //image_upload_memory,
            //sampler,
            frames_in_flight,
            frame: 0,
            render_texture,
        }
    }

    pub fn recreate_swapchain(&mut self) {
        let caps = self.surface.capabilities(&self.adapter.physical_device);
        let swap_config = window::SwapchainConfig::from_caps(&caps, self.format, self.dimensions);
        println!("{:?}", swap_config);
        let extent = swap_config.extent.to_extent();

        unsafe {
            self.surface
                .configure_swapchain(&self.device, swap_config)
                .expect("Can't create swapchain");
        }

        self.viewport.rect.w = extent.width as _;
        self.viewport.rect.h = extent.height as _;
    }

    pub fn render<S: Updater>(&mut self, surface: &mut S) {
        let surface_image = unsafe {
            match self.surface.acquire_image(!0) {
                Ok((image, _)) => image,
                Err(_) => {
                    self.recreate_swapchain();
                    return;
                }
            }
        };

        let framebuffer = unsafe {
            self.device
                .create_framebuffer(
                    &self.render_pass,
                    iter::once(surface_image.borrow()),
                    i::Extent {
                        width: self.dimensions.width,
                        height: self.dimensions.height,
                        depth: 1,
                    },
                )
                .unwrap()
        };

        // Compute index into our resource ring buffers based on the frame number
        // and number of frames in flight. Pay close attention to where this index is needed
        // versus when the swapchain image index we got from acquire_image is needed.
        let frame_idx = self.frame as usize % self.frames_in_flight;
        surface.update(&mut self.render_texture.img);
        // Wait for the fence of the previous submission of this frame and reset it; ensures we are
        // submitting only up to maximum number of frames_in_flight if we are submitting faster than
        // the gpu can keep up with. This would also guarantee that any resources which need to be
        // updated with a CPU->GPU data copy are not in use by the GPU, so we can perform those updates.
        // In this case there are none to be done, however.
        unsafe {
            let fence = &self.submission_complete_fences[frame_idx];
            self.device
                .wait_for_fence(fence, !0)
                .expect("Failed to wait for fence");
            self.device
                .reset_fence(fence)
                .expect("Failed to reset fence");
            self.cmd_pools[frame_idx].reset(false);
        }

        // Rendering
        let cmd_buffer = &mut self.cmd_buffers[frame_idx];
        self.render_texture
            .update(&mut self.device, cmd_buffer, &mut self.queue_group);
        unsafe {
            cmd_buffer.begin_primary(command::CommandBufferFlags::ONE_TIME_SUBMIT);

            cmd_buffer.set_viewports(0, &[self.viewport.clone()]);
            cmd_buffer.set_scissors(0, &[self.viewport.rect]);
            cmd_buffer.bind_graphics_pipeline(&self.pipeline);
            cmd_buffer.bind_vertex_buffers(
                0,
                iter::once((&*self.vertex_buffer, buffer::SubRange::WHOLE)),
            );
            cmd_buffer.bind_graphics_descriptor_sets(
                &self.pipeline_layout,
                0,
                iter::once(&self.desc_set),
                &[],
            );

            cmd_buffer.begin_render_pass(
                &self.render_pass,
                &framebuffer,
                self.viewport.rect,
                &[command::ClearValue {
                    color: command::ClearColor {
                        float32: [0.8, 0.8, 0.8, 1.0],
                    },
                }],
                command::SubpassContents::Inline,
            );
            cmd_buffer.draw(0..6, 0..1);
            cmd_buffer.end_render_pass();
            cmd_buffer.finish();

            let submission = Submission {
                command_buffers: iter::once(&*cmd_buffer),
                wait_semaphores: None,
                signal_semaphores: iter::once(&self.submission_complete_semaphores[frame_idx]),
            };
            self.queue_group.queues[0].submit(
                submission,
                Some(&self.submission_complete_fences[frame_idx]),
            );

            // present frame
            let result = self.queue_group.queues[0].present(
                &mut self.surface,
                surface_image,
                Some(&self.submission_complete_semaphores[frame_idx]),
            );

            self.device.destroy_framebuffer(framebuffer);

            if result.is_err() {
                self.recreate_swapchain();
            }
        }

        // Increment our frame
        self.frame += 1;
    }
}

impl<B> Drop for Renderer<B>
where
    B: gfx_hal::Backend,
{
    fn drop(&mut self) {
        self.device.wait_idle().unwrap();
        unsafe {
            self.render_texture.drop(&self.device);
            // TODO: When ManuallyDrop::take (soon to be renamed to ManuallyDrop::read) is stabilized we should use that instead.
            self.device
                .destroy_descriptor_pool(ManuallyDrop::into_inner(ptr::read(&self.desc_pool)));
            self.device
                .destroy_descriptor_set_layout(ManuallyDrop::into_inner(ptr::read(
                    &self.set_layout,
                )));

            self.device
                .destroy_buffer(ManuallyDrop::into_inner(ptr::read(&self.vertex_buffer)));

            //TODO DESTROY ON RENDER_TEXTURE
            //self.device
            //    .destroy_image(ManuallyDrop::into_inner(ptr::read(&self.image_logo)));
            //self.device
            //    .destroy_image_view(ManuallyDrop::into_inner(ptr::read(&self.image_srv)));
            //self.device
            //    .destroy_sampler(ManuallyDrop::into_inner(ptr::read(&self.sampler)));
            for p in self.cmd_pools.drain(..) {
                self.device.destroy_command_pool(p);
            }
            for s in self.submission_complete_semaphores.drain(..) {
                self.device.destroy_semaphore(s);
            }
            for f in self.submission_complete_fences.drain(..) {
                self.device.destroy_fence(f);
            }
            self.device
                .destroy_render_pass(ManuallyDrop::into_inner(ptr::read(&self.render_pass)));
            self.surface.unconfigure_swapchain(&self.device);
            self.device
                .free_memory(ManuallyDrop::into_inner(ptr::read(&self.buffer_memory)));
            //TODO DO THIS ON RENDER_TEXTURE
            //self.device
            //    .free_memory(ManuallyDrop::into_inner(ptr::read(&self.image_memory)));
            //self.device.free_memory(ManuallyDrop::into_inner(ptr::read(
            //    &self.image_upload_memory,
            //)));
            self.device
                .destroy_graphics_pipeline(ManuallyDrop::into_inner(ptr::read(&self.pipeline)));
            self.device
                .destroy_pipeline_layout(ManuallyDrop::into_inner(ptr::read(
                    &self.pipeline_layout,
                )));
            if let Some(instance) = &self.instance {
                let surface = ManuallyDrop::into_inner(ptr::read(&self.surface));
                instance.destroy_surface(surface);
            }
        }
        println!("DROPPED!");
    }
}
