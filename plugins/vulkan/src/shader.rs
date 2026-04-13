use crate::context::VulkanState;
use ash::vk;
use std::sync::{Arc, Mutex};

pub(crate) struct GpuShader {
    pub(crate) ctx: Arc<Mutex<VulkanState>>,
    pub(crate) pipeline: vk::Pipeline,
    pub(crate) pipeline_layout: vk::PipelineLayout,
    pub(crate) descriptor_set_layout: vk::DescriptorSetLayout,
    pub(crate) shader_module: vk::ShaderModule,
    pub(crate) num_buffers: u32,
}

impl Drop for GpuShader {
    fn drop(&mut self) {
        if let Ok(state) = self.ctx.lock() {
            unsafe {
                state.device.destroy_pipeline(self.pipeline, None);
                state
                    .device
                    .destroy_pipeline_layout(self.pipeline_layout, None);
                state
                    .device
                    .destroy_descriptor_set_layout(self.descriptor_set_layout, None);
                state.device.destroy_shader_module(self.shader_module, None);
            }
        }
    }
}

pub(crate) fn create_shader(
    ctx: &Arc<Mutex<VulkanState>>,
    spirv: &[u8],
    num_buffers: u32,
) -> Result<GpuShader, String> {
    if spirv.len() < 4 || spirv.len() % 4 != 0 {
        return Err("SPIR-V data must be non-empty and 4-byte aligned".into());
    }

    // Reinterpret bytes as u32 words
    let code: Vec<u32> = spirv
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();

    let state = ctx.lock().map_err(|e| format!("lock failed: {e}"))?;

    // ── Shader module ───────────────────────────────────────────
    let module_info = vk::ShaderModuleCreateInfo::default().code(&code);
    let shader_module = unsafe { state.device.create_shader_module(&module_info, None) }
        .map_err(|e| format!("vkCreateShaderModule failed: {e}"))?;

    // ── Descriptor set layout ───────────────────────────────────
    let bindings: Vec<vk::DescriptorSetLayoutBinding> = (0..num_buffers)
        .map(|i| {
            vk::DescriptorSetLayoutBinding::default()
                .binding(i)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
        })
        .collect();

    let ds_layout_info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
    let descriptor_set_layout = unsafe {
        state
            .device
            .create_descriptor_set_layout(&ds_layout_info, None)
    }
    .map_err(|e| format!("vkCreateDescriptorSetLayout failed: {e}"))?;

    // ── Pipeline layout ─────────────────────────────────────────
    let pl_layout_info = vk::PipelineLayoutCreateInfo::default()
        .set_layouts(std::slice::from_ref(&descriptor_set_layout));
    let pipeline_layout = unsafe { state.device.create_pipeline_layout(&pl_layout_info, None) }
        .map_err(|e| {
            unsafe {
                state
                    .device
                    .destroy_descriptor_set_layout(descriptor_set_layout, None);
                state.device.destroy_shader_module(shader_module, None);
            }
            format!("vkCreatePipelineLayout failed: {e}")
        })?;

    // ── Compute pipeline ────────────────────────────────────────
    let stage_info = vk::PipelineShaderStageCreateInfo::default()
        .stage(vk::ShaderStageFlags::COMPUTE)
        .module(shader_module)
        .name(c"main");

    let pipeline_info = vk::ComputePipelineCreateInfo::default()
        .stage(stage_info)
        .layout(pipeline_layout);

    let pipelines = unsafe {
        state.device.create_compute_pipelines(
            vk::PipelineCache::null(),
            std::slice::from_ref(&pipeline_info),
            None,
        )
    }
    .map_err(|(_pipelines, e)| {
        unsafe {
            state.device.destroy_pipeline_layout(pipeline_layout, None);
            state
                .device
                .destroy_descriptor_set_layout(descriptor_set_layout, None);
            state.device.destroy_shader_module(shader_module, None);
        }
        format!("vkCreateComputePipelines failed: {e}")
    })?;

    Ok(GpuShader {
        ctx: ctx.clone(),
        pipeline: pipelines[0],
        pipeline_layout,
        descriptor_set_layout,
        shader_module,
        num_buffers,
    })
}
