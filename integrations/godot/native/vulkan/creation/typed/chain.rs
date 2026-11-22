// SPDX-License-Identifier: MIT
use crate::platform::VkChainFacts;
use ash::vk::{self, StructureType, TaggedStructure};
use std::alloc::{Layout, alloc, dealloc};
use std::ffi::c_void;
use std::ptr::{self, NonNull};

const LIMIT: usize = 64;

/// `VkPhysicalDeviceAntiLagFeaturesAMD`; ash's Vulkan headers predate VK_AMD_anti_lag.
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct AntiLagFeatures {
    pub(super) s_type: StructureType,
    pub(super) p_next: *mut c_void,
    pub(super) anti_lag: vk::Bool32,
}

pub(super) const ANTI_LAG_FEATURES: StructureType = StructureType::from_raw(1_000_476_000);

/// The engine's feature structures a device request may carry; any other tag is not cloned.
fn layout(tag: StructureType) -> Option<Layout> {
    macro_rules! supported {
        ($($ty:ty),+ $(,)?) => {{
            $(if tag == <$ty as TaggedStructure>::STRUCTURE_TYPE { return Some(Layout::new::<$ty>()); })+
            if tag == ANTI_LAG_FEATURES { return Some(Layout::new::<AntiLagFeatures>()); }
            None
        }};
    }
    supported! {
        vk::PhysicalDeviceFeatures2<'static>,
        vk::PhysicalDeviceVulkan11Features<'static>,
        vk::PhysicalDeviceVulkan12Features<'static>,
        vk::PhysicalDeviceVulkan13Features<'static>,
        vk::PhysicalDevice16BitStorageFeatures<'static>,
        vk::PhysicalDeviceMultiviewFeatures<'static>,
        vk::PhysicalDeviceShaderFloat16Int8Features<'static>,
        vk::PhysicalDeviceBufferDeviceAddressFeatures<'static>,
        vk::PhysicalDeviceVulkanMemoryModelFeatures<'static>,
        vk::PhysicalDeviceTimelineSemaphoreFeatures<'static>,
        vk::PhysicalDevicePipelineCreationCacheControlFeatures<'static>,
        vk::PhysicalDeviceSynchronization2Features<'static>,
        vk::PhysicalDeviceFragmentDensityMapFeaturesEXT<'static>,
        vk::PhysicalDeviceFragmentDensityMapOffsetFeaturesQCOM<'static>,
        vk::PhysicalDeviceFragmentShadingRateFeaturesKHR<'static>,
        vk::PhysicalDeviceFaultFeaturesEXT<'static>,
        vk::PhysicalDeviceAccelerationStructureFeaturesKHR<'static>,
        vk::PhysicalDeviceRayTracingPipelineFeaturesKHR<'static>,
        vk::PhysicalDeviceRayTracingValidationFeaturesNV<'static>,
        vk::PhysicalDeviceScalarBlockLayoutFeatures<'static>,
        vk::PhysicalDeviceShaderIntegerDotProductFeatures<'static>,
        vk::PhysicalDeviceMutableDescriptorTypeFeaturesEXT<'static>,
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
    pub(super) head: *mut vk::BaseOutStructure<'static>,
}

impl Chain {
    /// The engine lends valid Vulkan headers and each whitelisted node's full record.
    /// Refuse unknown tags, repeats and oversized chains. A separate error means allocation failed.
    pub(super) unsafe fn copy(
        mut source: *const vk::BaseInStructure<'static>,
    ) -> Result<Option<Self>, ()> {
        let mut copied = Self::default();
        let mut seen = [StructureType::default(); LIMIT];
        let mut link = &raw mut copied.head;
        while let Some(node) = unsafe { source.as_ref() } {
            let index = copied.storage.len();
            let Some(layout) = layout(node.s_type) else {
                return Ok(None);
            };
            if index >= LIMIT || seen[..index].contains(&node.s_type) {
                return Ok(None);
            }
            seen[index] = node.s_type;
            copied.storage.try_reserve(1).map_err(|_| ())?;
            let bytes = NonNull::new(unsafe { alloc(layout) }).ok_or(())?;
            let output = bytes.cast::<vk::BaseOutStructure<'static>>().as_ptr();
            unsafe {
                ptr::copy_nonoverlapping(source.cast::<u8>(), bytes.as_ptr(), layout.size());
                (*output).p_next = ptr::null_mut();
                link.write(output);
                link = &raw mut (*output).p_next;
            }
            copied.storage.push(Node { bytes, layout });
            source = node.p_next;
        }
        Ok(Some(copied))
    }

    /// The writable injected node lives in the same create call as this chain.
    pub(super) unsafe fn prepend(&mut self, node: *mut vk::BaseOutStructure<'static>) {
        unsafe { (*node).p_next = self.head };
        self.head = node;
    }

    pub(super) unsafe fn feature_root_first(&mut self) {
        let mut link = &raw mut self.head;
        loop {
            let node = unsafe { link.read() };
            if node.is_null() {
                break;
            }
            if unsafe { (*node).s_type } == StructureType::PHYSICAL_DEVICE_FEATURES_2 {
                if node != self.head {
                    unsafe {
                        link.write((*node).p_next);
                        (*node).p_next = self.head;
                    }
                    self.head = node;
                }
                break;
            }
            link = unsafe { &raw mut (*node).p_next };
        }
    }
}

pub(super) unsafe extern "C" fn chain_facts(request: *const c_void, out: *mut VkChainFacts) {
    let info = unsafe { &*request.cast::<vk::DeviceCreateInfo<'static>>() };
    let mut current = info.p_next.cast::<vk::BaseInStructure<'static>>();
    let mut seen = [StructureType::default(); LIMIT];
    let mut facts = VkChainFacts {
        enabled_features: u32::from(!info.p_enabled_features.is_null()),
        cloneable: 1,
        ..Default::default()
    };
    for index in 0..LIMIT {
        let Some(node) = (unsafe { current.as_ref() }) else {
            break;
        };
        facts.cloneable &=
            u32::from(layout(node.s_type).is_some() && !seen[..index].contains(&node.s_type));
        seen[index] = node.s_type;
        match node.s_type {
            StructureType::PHYSICAL_DEVICE_FEATURES_2 => facts.features2 = 1,
            StructureType::PHYSICAL_DEVICE_VULKAN_1_2_FEATURES => {
                facts.vulkan12 = 1;
                let value = unsafe { &*current.cast::<vk::PhysicalDeviceVulkan12Features<'_>>() };
                facts.timeline_semaphore |= u32::from(value.timeline_semaphore != 0);
            }
            StructureType::PHYSICAL_DEVICE_VULKAN_1_3_FEATURES => facts.vulkan13 = 1,
            StructureType::PHYSICAL_DEVICE_SHADER_FLOAT16_INT8_FEATURES => facts.float16_int8 = 1,
            StructureType::PHYSICAL_DEVICE_SCALAR_BLOCK_LAYOUT_FEATURES => {
                facts.scalar_block_layout = 1
            }
            StructureType::PHYSICAL_DEVICE_SHADER_INTEGER_DOT_PRODUCT_FEATURES => {
                facts.integer_dot_product = 1
            }
            StructureType::PHYSICAL_DEVICE_TIMELINE_SEMAPHORE_FEATURES => {
                let value =
                    unsafe { &*current.cast::<vk::PhysicalDeviceTimelineSemaphoreFeatures<'_>>() };
                facts.timeline_semaphore |= u32::from(value.timeline_semaphore != 0);
            }
            _ => (),
        }
        current = node.p_next;
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
