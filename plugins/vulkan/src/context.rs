use ash::vk;
use gpu_allocator::vulkan::{Allocator, AllocatorCreateDesc};
use std::sync::{Arc, Mutex};

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
}

impl Drop for VulkanState {
    fn drop(&mut self) {
        unsafe {
            self.device.device_wait_idle().ok();
            // allocator is dropped automatically (it's a field)
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
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
    };

    Ok(GpuCtx {
        inner: Arc::new(Mutex::new(state)),
    })
}
