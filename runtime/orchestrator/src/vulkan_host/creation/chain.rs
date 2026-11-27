// SPDX-License-Identifier: MIT
//! A private copy of the engine's device feature chain, which the XeSS and latency requirements
//! patch without touching the engine's records.
use ash::vk::{self, StructureType, TaggedStructure};
use std::alloc::{Layout, alloc, dealloc};
use std::ffi::c_void;
use std::ptr::{self, NonNull};

pub(super) const LIMIT: usize = 64;

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

    pub(super) fn contains(&self, tag: StructureType) -> bool {
        let mut node = self.head;
        // SAFETY: the copied nodes are owned by this chain and linked by copy and prepend.
        while let Some(value) = unsafe { node.as_ref() } {
            if value.s_type == tag {
                return true;
            }
            node = value.p_next;
        }
        false
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
