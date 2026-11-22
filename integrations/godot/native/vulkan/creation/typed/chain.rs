// SPDX-License-Identifier: MIT
use super::super::abi::*;
use crate::platform::VkChainFacts;
use std::alloc::{Layout, alloc, dealloc};
use std::ffi::c_void;
use std::ptr::{self, NonNull};

const LIMIT: usize = 64;

fn layout(tag: VkStructureType) -> Option<Layout> {
    macro_rules! supported {
        ($($tag:ident => $ty:ty),+ $(,)?) => {
            match tag { $($tag => Some(Layout::new::<$ty>()),)+ _ => None }
        };
    }
    supported! {
        TAG_PHYSICAL_DEVICE_FEATURES_2 => VkPhysicalDeviceFeatures2,
        TAG_PHYSICAL_DEVICE_VULKAN_1_1_FEATURES => VkPhysicalDeviceVulkan11Features,
        TAG_PHYSICAL_DEVICE_VULKAN_1_2_FEATURES => VkPhysicalDeviceVulkan12Features,
        TAG_PHYSICAL_DEVICE_VULKAN_1_3_FEATURES => VkPhysicalDeviceVulkan13Features,
        TAG_PHYSICAL_DEVICE_16BIT_STORAGE_FEATURES => VkPhysicalDevice16BitStorageFeatures,
        TAG_PHYSICAL_DEVICE_MULTIVIEW_FEATURES => VkPhysicalDeviceMultiviewFeatures,
        TAG_PHYSICAL_DEVICE_SHADER_FLOAT16_INT8_FEATURES => VkPhysicalDeviceShaderFloat16Int8Features,
        TAG_PHYSICAL_DEVICE_BUFFER_DEVICE_ADDRESS_FEATURES => VkPhysicalDeviceBufferDeviceAddressFeatures,
        TAG_PHYSICAL_DEVICE_VULKAN_MEMORY_MODEL_FEATURES => VkPhysicalDeviceVulkanMemoryModelFeatures,
        TAG_PHYSICAL_DEVICE_TIMELINE_SEMAPHORE_FEATURES => VkPhysicalDeviceTimelineSemaphoreFeatures,
        TAG_PHYSICAL_DEVICE_PIPELINE_CREATION_CACHE_CONTROL_FEATURES => VkPhysicalDevicePipelineCreationCacheControlFeatures,
        TAG_PHYSICAL_DEVICE_SYNCHRONIZATION_2_FEATURES => VkPhysicalDeviceSynchronization2Features,
        TAG_PHYSICAL_DEVICE_FRAGMENT_DENSITY_MAP_FEATURES_EXT => VkPhysicalDeviceFragmentDensityMapFeaturesEXT,
        TAG_PHYSICAL_DEVICE_FRAGMENT_DENSITY_MAP_OFFSET_FEATURES_QCOM => VkPhysicalDeviceFragmentDensityMapOffsetFeaturesQCOM,
        TAG_PHYSICAL_DEVICE_FRAGMENT_SHADING_RATE_FEATURES_KHR => VkPhysicalDeviceFragmentShadingRateFeaturesKHR,
        TAG_PHYSICAL_DEVICE_FAULT_FEATURES_EXT => VkPhysicalDeviceFaultFeaturesEXT,
        TAG_PHYSICAL_DEVICE_ACCELERATION_STRUCTURE_FEATURES_KHR => VkPhysicalDeviceAccelerationStructureFeaturesKHR,
        TAG_PHYSICAL_DEVICE_RAY_TRACING_PIPELINE_FEATURES_KHR => VkPhysicalDeviceRayTracingPipelineFeaturesKHR,
        TAG_PHYSICAL_DEVICE_RAY_TRACING_VALIDATION_FEATURES_NV => VkPhysicalDeviceRayTracingValidationFeaturesNV,
        TAG_PHYSICAL_DEVICE_SCALAR_BLOCK_LAYOUT_FEATURES => VkPhysicalDeviceScalarBlockLayoutFeatures,
        TAG_PHYSICAL_DEVICE_SHADER_INTEGER_DOT_PRODUCT_FEATURES => VkPhysicalDeviceShaderIntegerDotProductFeatures,
        TAG_PHYSICAL_DEVICE_ANTI_LAG_FEATURES_AMD => VkPhysicalDeviceAntiLagFeaturesAMD,
        TAG_PHYSICAL_DEVICE_MUTABLE_DESCRIPTOR_TYPE_FEATURES_EXT => VkPhysicalDeviceMutableDescriptorTypeFeaturesEXT,
    }
}

struct Node {
    bytes: NonNull<u8>,
    layout: Layout,
}

impl Drop for Node {
    fn drop(&mut self) {
        unsafe { dealloc(self.bytes.as_ptr(), self.layout) };
    }
}

#[derive(Default)]
pub(super) struct Chain {
    storage: Vec<Node>,
    pub(super) head: *mut VkBaseOutStructure,
}

impl Chain {
    /// The engine lends valid Vulkan headers and each whitelisted node's full record.
    /// Refuse unknown tags, repeats and oversized chains. A separate error means allocation failed.
    pub(super) unsafe fn copy(mut source: *const VkBaseInStructure) -> Result<Option<Self>, ()> {
        let mut copied = Self::default();
        let mut seen = [0; LIMIT];
        let mut link = &raw mut copied.head;
        while let Some(node) = unsafe { source.as_ref() } {
            let index = copied.storage.len();
            let Some(layout) = layout(node.sType) else {
                return Ok(None);
            };
            if index >= LIMIT || seen[..index].contains(&node.sType) {
                return Ok(None);
            }
            seen[index] = node.sType;
            copied.storage.try_reserve(1).map_err(|_| ())?;
            let bytes = NonNull::new(unsafe { alloc(layout) }).ok_or(())?;
            let output = bytes.cast::<VkBaseOutStructure>().as_ptr();
            unsafe {
                ptr::copy_nonoverlapping(source.cast::<u8>(), bytes.as_ptr(), layout.size());
                (*output).pNext = ptr::null_mut();
                link.write(output);
                link = &raw mut (*output).pNext;
            }
            copied.storage.push(Node { bytes, layout });
            source = node.pNext;
        }
        Ok(Some(copied))
    }

    /// The writable injected node lives in the same create call as this chain.
    pub(super) unsafe fn prepend(&mut self, node: *mut VkBaseOutStructure) {
        unsafe { (*node).pNext = self.head };
        self.head = node;
    }

    pub(super) unsafe fn feature_root_first(&mut self) {
        let mut link = &raw mut self.head;
        loop {
            let node = unsafe { link.read() };
            if node.is_null() {
                break;
            }
            if unsafe { (*node).sType } == TAG_PHYSICAL_DEVICE_FEATURES_2 {
                if node != self.head {
                    unsafe {
                        link.write((*node).pNext);
                        (*node).pNext = self.head;
                    }
                    self.head = node;
                }
                break;
            }
            link = unsafe { &raw mut (*node).pNext };
        }
    }
}

pub(super) unsafe extern "C" fn chain_facts(request: *const c_void, out: *mut VkChainFacts) {
    let info = unsafe { &*request.cast::<VkDeviceCreateInfo>() };
    let mut current = info.pNext.cast::<VkBaseInStructure>();
    let mut seen = [0; LIMIT];
    let mut facts = VkChainFacts {
        enabled_features: u32::from(!info.pEnabledFeatures.is_null()),
        cloneable: 1,
        ..Default::default()
    };
    for index in 0..LIMIT {
        let Some(node) = (unsafe { current.as_ref() }) else {
            break;
        };
        facts.cloneable &=
            u32::from(layout(node.sType).is_some() && !seen[..index].contains(&node.sType));
        seen[index] = node.sType;
        match node.sType {
            TAG_PHYSICAL_DEVICE_FEATURES_2 => facts.features2 = 1,
            TAG_PHYSICAL_DEVICE_VULKAN_1_2_FEATURES => {
                facts.vulkan12 = 1;
                let value = unsafe { &*current.cast::<VkPhysicalDeviceVulkan12Features>() };
                facts.timeline_semaphore |= u32::from(value.timelineSemaphore != 0);
            }
            TAG_PHYSICAL_DEVICE_VULKAN_1_3_FEATURES => facts.vulkan13 = 1,
            TAG_PHYSICAL_DEVICE_SHADER_FLOAT16_INT8_FEATURES => facts.float16_int8 = 1,
            TAG_PHYSICAL_DEVICE_SCALAR_BLOCK_LAYOUT_FEATURES => facts.scalar_block_layout = 1,
            TAG_PHYSICAL_DEVICE_SHADER_INTEGER_DOT_PRODUCT_FEATURES => {
                facts.integer_dot_product = 1
            }
            TAG_PHYSICAL_DEVICE_TIMELINE_SEMAPHORE_FEATURES => {
                let value =
                    unsafe { &*current.cast::<VkPhysicalDeviceTimelineSemaphoreFeatures>() };
                facts.timeline_semaphore |= u32::from(value.timelineSemaphore != 0);
            }
            _ => (),
        }
        current = node.pNext;
    }
    facts.cloneable &= u32::from(current.is_null());
    // Presence flags for an uncloneable chain do not enter XeSS negotiation.
    if facts.cloneable == 0 {
        facts = VkChainFacts {
            enabled_features: facts.enabled_features,
            timeline_semaphore: facts.timeline_semaphore,
            ..Default::default()
        };
    }
    unsafe { out.write(facts) };
}
