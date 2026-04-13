use crate::context::VulkanState;
use ash::vk;
use gpu_allocator::vulkan::{Allocation, AllocationCreateDesc, AllocationScheme};
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

/// Extracted buffer specification ready for the Send closure.
pub(crate) struct BufferSpec {
    pub(crate) data: Vec<f32>,
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
}

/// Handle to an in-flight GPU dispatch. Holds all state needed for
/// wait (fence fd) and collect (readback + cleanup).
pub(crate) struct GpuHandle {
    pub(crate) ctx: Arc<Mutex<VulkanState>>,
    pub(crate) fence: vk::Fence,
    pub(crate) fence_fd: RawFd,
    pub(crate) command_pool: vk::CommandPool,
    pub(crate) descriptor_pool: vk::DescriptorPool,
    pub(crate) buffers: Vec<LiveBuffer>,
}

impl Drop for GpuHandle {
    fn drop(&mut self) {
        // Safety cleanup if collect was never called
        if let Ok(mut state) = self.ctx.lock() {
            let device = state.device.clone();
            unsafe { device.destroy_descriptor_pool(self.descriptor_pool, None) };
            for lb in self.buffers.drain(..) {
                if let Some(allocation) = lb.allocation {
                    state.allocator.free(allocation).ok();
                }
                unsafe { device.destroy_buffer(lb.buffer, None) };
            }
            unsafe {
                device.destroy_fence(self.fence, None);
                device.destroy_command_pool(self.command_pool, None);
            }
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
    specs: Vec<BufferSpec>,
) -> Result<GpuHandle, String> {
    let mut state = ctx_arc.lock().map_err(|e| format!("lock: {e}"))?;
    let device = state.device.clone();
    let queue = state.queue;
    let queue_family_index = state.queue_family_index;

    // ── Command pool ────────────────────────────────────────────
    let pool_info = vk::CommandPoolCreateInfo::default()
        .queue_family_index(queue_family_index)
        .flags(vk::CommandPoolCreateFlags::TRANSIENT);
    let command_pool = unsafe { device.create_command_pool(&pool_info, None) }
        .map_err(|e| format!("create_command_pool: {e}"))?;

    // ── Command buffer ──────────────────────────────────────────
    let alloc_info = vk::CommandBufferAllocateInfo::default()
        .command_pool(command_pool)
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(1);
    let cmd = unsafe { device.allocate_command_buffers(&alloc_info) }
        .map_err(|e| format!("allocate_command_buffers: {e}"))?[0];

    // ── Create + upload buffers ─────────────────────────────────
    let mut live_buffers: Vec<LiveBuffer> = Vec::with_capacity(specs.len());
    if let Err(e) = create_buffers(&mut state, &device, &specs, &mut live_buffers) {
        cleanup_buffers(&mut state, &device, &mut live_buffers);
        unsafe { device.destroy_command_pool(command_pool, None) };
        return Err(e);
    }

    // Upload input data
    for (i, (lb, spec)) in live_buffers.iter().zip(specs.iter()).enumerate() {
        if lb.usage == BufferUsage::Output {
            continue;
        }
        let alloc = lb.allocation.as_ref().unwrap();
        let mapped = alloc
            .mapped_ptr()
            .ok_or_else(|| format!("buffer[{i}] not host-mapped"))?
            .as_ptr() as *mut u8;
        let src =
            unsafe { std::slice::from_raw_parts(spec.data.as_ptr() as *const u8, spec.byte_size) };
        unsafe { std::ptr::copy_nonoverlapping(src.as_ptr(), mapped, src.len()) };
    }

    // ── Descriptor pool + set ───────────────────────────────────
    let pool_size = vk::DescriptorPoolSize::default()
        .ty(vk::DescriptorType::STORAGE_BUFFER)
        .descriptor_count(specs.len() as u32);
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

    // ── Record command buffer ───────────────────────────────────
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

    // ── Fence with exportable fd ────────────────────────────────
    let mut export_info = vk::ExportFenceCreateInfo::default()
        .handle_types(vk::ExternalFenceHandleTypeFlags::SYNC_FD);
    let fence_info = vk::FenceCreateInfo::default().push_next(&mut export_info);
    let fence = unsafe { device.create_fence(&fence_info, None) }
        .map_err(|e| format!("create_fence: {e}"))?;

    // ── Submit ──────────────────────────────────────────────────
    let submit_info = vk::SubmitInfo::default().command_buffers(std::slice::from_ref(&cmd));
    unsafe { device.queue_submit(queue, &[submit_info], fence) }.map_err(|e| {
        unsafe { device.destroy_fence(fence, None) };
        format!("queue_submit: {e}")
    })?;

    // ── Export fence fd ─────────────────────────────────────────
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
        command_pool,
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

fn create_buffers(
    state: &mut VulkanState,
    device: &ash::Device,
    specs: &[BufferSpec],
    live_buffers: &mut Vec<LiveBuffer>,
) -> Result<(), String> {
    for (i, spec) in specs.iter().enumerate() {
        let buf_info = vk::BufferCreateInfo::default()
            .size(spec.byte_size as u64)
            .usage(vk::BufferUsageFlags::STORAGE_BUFFER)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        let buffer = unsafe { device.create_buffer(&buf_info, None) }
            .map_err(|e| format!("create_buffer[{i}]: {e}"))?;

        let requirements = unsafe { device.get_buffer_memory_requirements(buffer) };

        let location = match spec.usage {
            BufferUsage::Input => MemoryLocation::CpuToGpu,
            BufferUsage::Output => MemoryLocation::GpuToCpu,
            BufferUsage::InOut => MemoryLocation::CpuToGpu,
        };

        let allocation = state
            .allocator
            .allocate(&AllocationCreateDesc {
                name: &format!("buf-{i}"),
                requirements,
                location,
                linear: true,
                allocation_scheme: AllocationScheme::GpuAllocatorManaged,
            })
            .map_err(|e| {
                unsafe { device.destroy_buffer(buffer, None) };
                format!("allocate[{i}]: {e}")
            })?;

        let bind_result =
            unsafe { device.bind_buffer_memory(buffer, allocation.memory(), allocation.offset()) };
        if let Err(e) = bind_result {
            state.allocator.free(allocation).ok();
            unsafe { device.destroy_buffer(buffer, None) };
            return Err(format!("bind_buffer_memory[{i}]: {e}"));
        }

        let element_count = (spec.byte_size / 4) as u32;
        live_buffers.push(LiveBuffer {
            buffer,
            allocation: Some(allocation),
            byte_size: spec.byte_size,
            usage: spec.usage,
            element_count,
        });
    }
    Ok(())
}

fn cleanup_buffers(state: &mut VulkanState, device: &ash::Device, buffers: &mut Vec<LiveBuffer>) {
    for lb in buffers.drain(..) {
        if let Some(allocation) = lb.allocation {
            state.allocator.free(allocation).ok();
        }
        unsafe { device.destroy_buffer(lb.buffer, None) };
    }
}
