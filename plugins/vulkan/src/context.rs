use ash::vk;
use gpu_allocator::vulkan::{
    Allocation, AllocationCreateDesc, AllocationScheme, Allocator, AllocatorCreateDesc,
};
use gpu_allocator::MemoryLocation;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Round up to next power of 2, minimum 256 bytes.
fn size_bucket(bytes: usize) -> usize {
    let min = 256;
    let n = bytes.max(min);
    n.next_power_of_two()
}

/// Cached Vulkan buffer + allocation, keyed by (size_bucket, MemoryLocation).
struct PooledBuffer {
    buffer: vk::Buffer,
    allocation: Allocation,
    byte_size: usize,
}

/// Reusable buffer pool. Caches allocations by (size_bucket, location).
pub(crate) struct BufferPool {
    free: HashMap<(usize, MemoryLocation), Vec<PooledBuffer>>,
}

impl BufferPool {
    pub(crate) fn new() -> Self {
        Self {
            free: HashMap::new(),
        }
    }

    /// Acquire a buffer of at least `byte_size` bytes with the given memory location.
    /// Returns None if no cached buffer is available (caller must allocate fresh).
    pub(crate) fn acquire(
        &mut self,
        byte_size: usize,
        location: MemoryLocation,
    ) -> Option<(vk::Buffer, Allocation, usize)> {
        let bucket = size_bucket(byte_size);
        self.free
            .get_mut(&(bucket, location))
            .and_then(|v| v.pop())
            .map(|pb| (pb.buffer, pb.allocation, pb.byte_size))
    }

    /// Return a buffer to the pool for reuse.
    pub(crate) fn release(
        &mut self,
        buffer: vk::Buffer,
        allocation: Allocation,
        byte_size: usize,
        location: MemoryLocation,
    ) {
        let bucket = size_bucket(byte_size);
        self.free
            .entry((bucket, location))
            .or_default()
            .push(PooledBuffer {
                buffer,
                allocation,
                byte_size,
            });
    }

    /// Free all pooled buffers.
    pub(crate) fn drain(&mut self, device: &ash::Device, allocator: &mut Allocator) {
        for (_, buffers) in self.free.drain() {
            for pb in buffers {
                allocator.free(pb.allocation).ok();
                unsafe { device.destroy_buffer(pb.buffer, None) };
            }
        }
    }
}

#[allow(dead_code)]
pub(crate) struct VulkanState {
    pub(crate) _entry: ash::Entry,
    pub(crate) instance: ash::Instance,
    pub(crate) device: ash::Device,
    pub(crate) physical_device: vk::PhysicalDevice,
    pub(crate) queue: vk::Queue,
    pub(crate) queue_family_index: u32,
    pub(crate) allocator: Allocator,
    pub(crate) fence_fd_fn: ash::khr::external_fence_fd::Device,
    pub(crate) buffer_pool: BufferPool,
    /// Reusable command pool (reset between dispatches).
    pub(crate) command_pool: Option<vk::CommandPool>,
}

impl Drop for VulkanState {
    fn drop(&mut self) {
        unsafe {
            self.device.device_wait_idle().ok();
            self.buffer_pool.drain(&self.device, &mut self.allocator);
            if let Some(cp) = self.command_pool.take() {
                self.device.destroy_command_pool(cp, None);
            }
            // allocator is dropped automatically (it's a field)
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

impl VulkanState {
    /// Get or create the reusable command pool.
    pub(crate) fn get_command_pool(&mut self) -> Result<vk::CommandPool, String> {
        if let Some(cp) = self.command_pool {
            unsafe {
                self.device
                    .reset_command_pool(cp, vk::CommandPoolResetFlags::empty())
            }
            .map_err(|e| format!("reset_command_pool: {e}"))?;
            Ok(cp)
        } else {
            let info = vk::CommandPoolCreateInfo::default()
                .queue_family_index(self.queue_family_index)
                .flags(vk::CommandPoolCreateFlags::TRANSIENT);
            let cp = unsafe { self.device.create_command_pool(&info, None) }
                .map_err(|e| format!("create_command_pool: {e}"))?;
            self.command_pool = Some(cp);
            Ok(cp)
        }
    }

    /// Allocate or reuse a buffer from the pool.
    pub(crate) fn acquire_buffer(
        &mut self,
        byte_size: usize,
        location: MemoryLocation,
        index: usize,
    ) -> Result<(vk::Buffer, Allocation), String> {
        if let Some((buf, alloc, _)) = self.buffer_pool.acquire(byte_size, location) {
            return Ok((buf, alloc));
        }
        // Allocate fresh
        let bucket_size = size_bucket(byte_size);
        let buf_info = vk::BufferCreateInfo::default()
            .size(bucket_size as u64)
            .usage(vk::BufferUsageFlags::STORAGE_BUFFER)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        let buffer = unsafe { self.device.create_buffer(&buf_info, None) }
            .map_err(|e| format!("create_buffer[{index}]: {e}"))?;

        let requirements = unsafe { self.device.get_buffer_memory_requirements(buffer) };
        let allocation = self
            .allocator
            .allocate(&AllocationCreateDesc {
                name: &format!("buf-{index}"),
                requirements,
                location,
                linear: true,
                allocation_scheme: AllocationScheme::GpuAllocatorManaged,
            })
            .map_err(|e| {
                unsafe { self.device.destroy_buffer(buffer, None) };
                format!("allocate[{index}]: {e}")
            })?;

        let bind_result = unsafe {
            self.device
                .bind_buffer_memory(buffer, allocation.memory(), allocation.offset())
        };
        if let Err(e) = bind_result {
            self.allocator.free(allocation).ok();
            unsafe { self.device.destroy_buffer(buffer, None) };
            return Err(format!("bind_buffer_memory[{index}]: {e}"));
        }

        Ok((buffer, allocation))
    }
}

pub(crate) struct GpuCtx {
    pub(crate) inner: Arc<Mutex<VulkanState>>,
}

pub(crate) fn init_vulkan() -> Result<GpuCtx, String> {
    let entry = unsafe { ash::Entry::load() }.map_err(|e| format!("failed to load Vulkan: {e}"))?;

    // ── Instance ────────────────────────────────────────────────
    let app_info = vk::ApplicationInfo::default()
        .application_name(c"elle-vulkan")
        .application_version(vk::make_api_version(0, 1, 0, 0))
        .engine_name(c"elle")
        .engine_version(vk::make_api_version(0, 1, 0, 0))
        .api_version(vk::make_api_version(0, 1, 2, 0));

    let create_info = vk::InstanceCreateInfo::default().application_info(&app_info);

    let instance = unsafe { entry.create_instance(&create_info, None) }
        .map_err(|e| format!("vkCreateInstance failed: {e}"))?;

    // ── Physical device ─────────────────────────────────────────
    let phys_devices = unsafe { instance.enumerate_physical_devices() }
        .map_err(|e| format!("enumerate_physical_devices: {e}"))?;

    if phys_devices.is_empty() {
        unsafe { instance.destroy_instance(None) };
        return Err("no Vulkan physical devices found".into());
    }

    // Pick first device with a compute queue
    let mut chosen = None;
    for pd in &phys_devices {
        let qf_props = unsafe { instance.get_physical_device_queue_family_properties(*pd) };
        for (idx, qf) in qf_props.iter().enumerate() {
            if qf.queue_flags.contains(vk::QueueFlags::COMPUTE) {
                chosen = Some((*pd, idx as u32));
                break;
            }
        }
        if chosen.is_some() {
            break;
        }
    }

    let (physical_device, queue_family_index) = match chosen {
        Some(c) => c,
        None => {
            unsafe { instance.destroy_instance(None) };
            return Err("no compute-capable queue family found".into());
        }
    };

    // ── Logical device + queue ──────────────────────────────────
    let queue_priorities = [1.0f32];
    let queue_create_info = vk::DeviceQueueCreateInfo::default()
        .queue_family_index(queue_family_index)
        .queue_priorities(&queue_priorities);

    let ext_names: Vec<*const i8> = vec![ash::khr::external_fence_fd::NAME.as_ptr()];
    let device_create_info = vk::DeviceCreateInfo::default()
        .queue_create_infos(std::slice::from_ref(&queue_create_info))
        .enabled_extension_names(&ext_names);

    let device = unsafe { instance.create_device(physical_device, &device_create_info, None) }
        .map_err(|e| format!("vkCreateDevice failed: {e}"))?;

    let fence_fd_fn = ash::khr::external_fence_fd::Device::new(&instance, &device);

    let queue = unsafe { device.get_device_queue(queue_family_index, 0) };

    // ── Allocator ───────────────────────────────────────────────
    let allocator = Allocator::new(&AllocatorCreateDesc {
        instance: instance.clone(),
        device: device.clone(),
        physical_device,
        debug_settings: Default::default(),
        buffer_device_address: false,
        allocation_sizes: Default::default(),
    })
    .map_err(|e| format!("gpu-allocator init failed: {e}"))?;

    let state = VulkanState {
        _entry: entry,
        instance,
        device,
        physical_device,
        queue,
        queue_family_index,
        allocator,
        fence_fd_fn,
        buffer_pool: BufferPool::new(),
        command_pool: None,
    };

    Ok(GpuCtx {
        inner: Arc::new(Mutex::new(state)),
    })
}
