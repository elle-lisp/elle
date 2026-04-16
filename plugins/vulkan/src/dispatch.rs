use crate::context::VulkanState;
use ash::vk;
use gpu_allocator::vulkan::Allocation;
use gpu_allocator::MemoryLocation;
use std::os::unix::io::RawFd;
use std::sync::{Arc, Mutex};

/// Which direction data flows for a buffer.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum BufferUsage {
    Input,
    Output,
    InOut,
}

/// Buffer source for a dispatch: either a fresh spec or a persistent buffer reference.
pub(crate) enum DispatchBuffer {
    /// Fresh buffer — allocated (or pooled), uploaded, freed after dispatch.
    Spec(BufferSpec),
    /// Persistent buffer — already allocated and uploaded. Not freed after dispatch.
    Persistent {
        buffer: vk::Buffer,
        byte_size: usize,
        usage: BufferUsage,
    },
}

/// Extracted buffer specification. Data is raw bytes (not f32-specific).
pub(crate) struct BufferSpec {
    pub(crate) data: Vec<u8>,
    pub(crate) byte_size: usize,
    pub(crate) usage: BufferUsage,
}

/// A live Vulkan buffer + allocation pair.
pub(crate) struct LiveBuffer {
    pub(crate) buffer: vk::Buffer,
    pub(crate) allocation: Option<Allocation>,
    pub(crate) byte_size: usize,
    pub(crate) usage: BufferUsage,
    pub(crate) element_count: u32,
    pub(crate) location: MemoryLocation,
}

/// Persistent GPU buffer. Lives across dispatches, freed on GC.
/// Data is uploaded once on creation; can be re-uploaded via update.
pub(crate) struct GpuBuffer {
    pub(crate) ctx: Arc<Mutex<VulkanState>>,
    pub(crate) buffer: vk::Buffer,
    pub(crate) allocation: Allocation,
    pub(crate) byte_size: usize,
    pub(crate) location: MemoryLocation,
}

impl Drop for GpuBuffer {
    fn drop(&mut self) {
        if let Ok(mut state) = self.ctx.lock() {
            // Return to pool (not freed immediately)
            let alloc = std::mem::replace(
                &mut self.allocation,
                // Dummy — won't be used after this
                unsafe { std::mem::zeroed() },
            );
            state
                .buffer_pool
                .release(self.buffer, alloc, self.byte_size, self.location);
        }
    }
}

impl GpuBuffer {
    /// Upload data to the buffer.
    pub(crate) fn upload(&self, data: &[u8]) -> Result<(), String> {
        if data.len() > self.byte_size {
            return Err(format!(
                "data ({} bytes) exceeds buffer size ({} bytes)",
                data.len(),
                self.byte_size
            ));
        }
        let mapped = self
            .allocation
            .mapped_ptr()
            .ok_or("buffer not host-mapped")?
            .as_ptr() as *mut u8;
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), mapped, data.len()) };
        Ok(())
    }
}

/// Handle to an in-flight GPU dispatch. Holds all state needed for
/// wait (fence fd) and collect (readback + cleanup).
pub(crate) struct GpuHandle {
    pub(crate) ctx: Arc<Mutex<VulkanState>>,
    pub(crate) fence: vk::Fence,
    pub(crate) fence_fd: RawFd,
    pub(crate) descriptor_pool: vk::DescriptorPool,
    pub(crate) buffers: Vec<LiveBuffer>,
}

impl Drop for GpuHandle {
    fn drop(&mut self) {
        if let Ok(mut state) = self.ctx.lock() {
            let device = state.device.clone();
            unsafe { device.destroy_descriptor_pool(self.descriptor_pool, None) };
            for lb in self.buffers.drain(..) {
                if let Some(allocation) = lb.allocation {
                    // Return to pool instead of freeing
                    state
                        .buffer_pool
                        .release(lb.buffer, allocation, lb.byte_size, lb.location);
                } else {
                    unsafe { device.destroy_buffer(lb.buffer, None) };
                }
            }
            unsafe { device.destroy_fence(self.fence, None) };
        }
        if self.fence_fd >= 0 {
            unsafe { libc::close(self.fence_fd) };
        }
    }
}

/// Submit GPU work and return a handle with a pollable fence fd.
/// Does NOT block — returns immediately after queue submission.
pub(crate) fn dispatch(
    ctx_arc: Arc<Mutex<VulkanState>>,
    pipeline: vk::Pipeline,
    pipeline_layout: vk::PipelineLayout,
    descriptor_set_layout: vk::DescriptorSetLayout,
    workgroups: [u32; 3],
    dispatch_bufs: Vec<DispatchBuffer>,
) -> Result<GpuHandle, String> {
    let mut state = ctx_arc.lock().map_err(|e| format!("lock: {e}"))?;
    let device = state.device.clone();
    let queue = state.queue;

    // ── Reusable command pool + fresh command buffer ──────────
    let command_pool = state.get_command_pool()?;
    let alloc_info = vk::CommandBufferAllocateInfo::default()
        .command_pool(command_pool)
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(1);
    let cmd = unsafe { device.allocate_command_buffers(&alloc_info) }
        .map_err(|e| format!("allocate_command_buffers: {e}"))?[0];

    // ── Resolve buffers: allocate fresh for Specs, reference for Persistent ──
    let mut live_buffers: Vec<LiveBuffer> = Vec::with_capacity(dispatch_bufs.len());
    for (i, db) in dispatch_bufs.iter().enumerate() {
        match db {
            DispatchBuffer::Spec(spec) => {
                let location = match spec.usage {
                    BufferUsage::Input => MemoryLocation::CpuToGpu,
                    BufferUsage::Output => MemoryLocation::GpuToCpu,
                    BufferUsage::InOut => MemoryLocation::CpuToGpu,
                };
                let (buffer, allocation) = state.acquire_buffer(spec.byte_size, location, i)?;
                // Upload data
                if spec.usage != BufferUsage::Output && !spec.data.is_empty() {
                    let mapped = allocation
                        .mapped_ptr()
                        .ok_or_else(|| format!("buffer[{i}] not host-mapped"))?
                        .as_ptr() as *mut u8;
                    unsafe {
                        std::ptr::copy_nonoverlapping(spec.data.as_ptr(), mapped, spec.data.len())
                    };
                }
                live_buffers.push(LiveBuffer {
                    buffer,
                    allocation: Some(allocation),
                    byte_size: spec.byte_size,
                    usage: spec.usage,
                    element_count: (spec.byte_size / 4) as u32,
                    location,
                });
            }
            DispatchBuffer::Persistent {
                buffer,
                byte_size,
                usage,
            } => {
                live_buffers.push(LiveBuffer {
                    buffer: *buffer,
                    allocation: None, // Not owned — persistent buffer manages its own lifetime
                    byte_size: *byte_size,
                    usage: *usage,
                    element_count: (*byte_size / 4) as u32,
                    location: MemoryLocation::CpuToGpu, // doesn't matter, not pooled
                });
            }
        }
    }

    // ── Descriptor pool + set ────────────────────────────────
    let pool_size = vk::DescriptorPoolSize::default()
        .ty(vk::DescriptorType::STORAGE_BUFFER)
        .descriptor_count(dispatch_bufs.len() as u32);
    let dp_info = vk::DescriptorPoolCreateInfo::default()
        .max_sets(1)
        .pool_sizes(std::slice::from_ref(&pool_size));
    let descriptor_pool = unsafe { device.create_descriptor_pool(&dp_info, None) }
        .map_err(|e| format!("create_descriptor_pool: {e}"))?;

    let ds_alloc_info = vk::DescriptorSetAllocateInfo::default()
        .descriptor_pool(descriptor_pool)
        .set_layouts(std::slice::from_ref(&descriptor_set_layout));
    let descriptor_set = unsafe { device.allocate_descriptor_sets(&ds_alloc_info) }
        .map_err(|e| format!("allocate_descriptor_sets: {e}"))?[0];

    let buffer_infos: Vec<vk::DescriptorBufferInfo> = live_buffers
        .iter()
        .map(|lb| {
            vk::DescriptorBufferInfo::default()
                .buffer(lb.buffer)
                .offset(0)
                .range(lb.byte_size as u64)
        })
        .collect();

    let writes: Vec<vk::WriteDescriptorSet> = buffer_infos
        .iter()
        .enumerate()
        .map(|(i, bi)| {
            vk::WriteDescriptorSet::default()
                .dst_set(descriptor_set)
                .dst_binding(i as u32)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(std::slice::from_ref(bi))
        })
        .collect();

    unsafe { device.update_descriptor_sets(&writes, &[]) };

    // ── Record command buffer ────────────────────────────────
    let begin_info =
        vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
    unsafe {
        device
            .begin_command_buffer(cmd, &begin_info)
            .map_err(|e| format!("begin_command_buffer: {e}"))?;
        device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, pipeline);
        device.cmd_bind_descriptor_sets(
            cmd,
            vk::PipelineBindPoint::COMPUTE,
            pipeline_layout,
            0,
            &[descriptor_set],
            &[],
        );
        device.cmd_dispatch(cmd, workgroups[0], workgroups[1], workgroups[2]);
    }

    // Memory barrier: compute write → host read
    let barrier = vk::MemoryBarrier::default()
        .src_access_mask(vk::AccessFlags::SHADER_WRITE)
        .dst_access_mask(vk::AccessFlags::HOST_READ);

    unsafe {
        device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::COMPUTE_SHADER,
            vk::PipelineStageFlags::HOST,
            vk::DependencyFlags::empty(),
            &[barrier],
            &[],
            &[],
        );
        device
            .end_command_buffer(cmd)
            .map_err(|e| format!("end_command_buffer: {e}"))?;
    }

    // ── Fence with exportable fd ─────────────────────────────
    let mut export_info = vk::ExportFenceCreateInfo::default()
        .handle_types(vk::ExternalFenceHandleTypeFlags::SYNC_FD);
    let fence_info = vk::FenceCreateInfo::default().push_next(&mut export_info);
    let fence = unsafe { device.create_fence(&fence_info, None) }
        .map_err(|e| format!("create_fence: {e}"))?;

    // ── Submit ───────────────────────────────────────────────
    let submit_info = vk::SubmitInfo::default().command_buffers(std::slice::from_ref(&cmd));
    unsafe { device.queue_submit(queue, &[submit_info], fence) }.map_err(|e| {
        unsafe { device.destroy_fence(fence, None) };
        format!("queue_submit: {e}")
    })?;

    // ── Export fence fd ──────────────────────────────────────
    let fd_info = vk::FenceGetFdInfoKHR::default()
        .fence(fence)
        .handle_type(vk::ExternalFenceHandleTypeFlags::SYNC_FD);
    let fence_fd = unsafe { state.fence_fd_fn.get_fence_fd(&fd_info) }
        .map_err(|e| format!("get_fence_fd: {e}"))?;

    // Drop the lock before returning — GPU is working independently
    drop(state);

    Ok(GpuHandle {
        ctx: ctx_arc,
        fence,
        fence_fd,
        descriptor_pool,
        buffers: live_buffers,
    })
}

/// Read back results from a completed dispatch (by reference).
/// Cleanup happens via GpuHandle::drop when the handle is garbage collected.
pub(crate) fn collect_ref(handle: &GpuHandle) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    let mut output_count: u32 = 0;
    output.extend_from_slice(&0u32.to_le_bytes());

    for (i, lb) in handle.buffers.iter().enumerate() {
        if lb.usage == BufferUsage::Input {
            continue;
        }
        output_count += 1;
        output.extend_from_slice(&lb.element_count.to_le_bytes());

        let alloc = lb.allocation.as_ref().unwrap();
        let mapped = alloc
            .mapped_ptr()
            .ok_or_else(|| format!("output buffer[{i}] not host-mapped"))?
            .as_ptr() as *const u8;
        let data = unsafe { std::slice::from_raw_parts(mapped, lb.byte_size) };
        output.extend_from_slice(data);
    }

    output[0..4].copy_from_slice(&output_count.to_le_bytes());
    Ok(output)
}
