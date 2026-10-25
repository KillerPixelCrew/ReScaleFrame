"""Apply checked Vulkan correctness fixes to an isolated copy of Streamline 2.14.1."""
from pathlib import Path
import argparse
import json

parser = argparse.ArgumentParser()
parser.add_argument("sdk", type=Path)
args = parser.parse_args()
sdk = args.sdk.resolve()
edits = []

def replace(file, before, after, count=1):
    path = sdk / file
    data = path.read_text(encoding="utf-8")
    observed = data.count(before)
    if observed != count:
        raise SystemExit(f"{file}: expected {count} exact anchors, found {observed}")
    path.write_text(data.replace(before, after), encoding="utf-8", newline="\n")
    edits.append({"file": file, "before": before, "after": after, "count": count})

vulkan = "source/platforms/sl.chi/vulkan.cpp"
replace(vulkan, "    VkSemaphore m_presentSemaphore{};", """    VkSemaphore m_presentSemaphore{};
    // A present wait belongs to the acquired real WSI image, not this command
    // context's rolling frame index. Reacquiring a different image cannot make
    // a previous image's binary semaphore reusable.
    std::unordered_map<VkSwapchainKHR, std::vector<VkSemaphore>> m_presentSemaphores;""")
replace(vulkan, "        VK_CHECK_RV(m_ddt.CreateSemaphore(dev, &createInfo, NULL, &m_presentSemaphore));", "        // Native present semaphores are allocated when the real image count is known.")
replace(vulkan, "        m_compute->setDebugName(&r, (std::string(debugName) + \".present-semaphore\").c_str());", "        // Each real image's semaphore receives its name after successful acquire.")
replace(vulkan, "        m_ddt.DestroySemaphore(m_device, m_presentSemaphore, nullptr);", """        for (const auto& [chain, semaphores] : m_presentSemaphores)
            for (VkSemaphore semaphore : semaphores)
                m_ddt.DestroySemaphore(m_device, semaphore, nullptr);
        m_presentSemaphores.clear();
        m_presentSemaphore = VK_NULL_HANDLE;""")
replace(vulkan, """                signalSemaphores.insert(signalSemaphores.end(), m_presentSemaphore);""", """                if (!m_presentSemaphore) return false; // No successful real-image acquire.
                signalSemaphores.insert(signalSemaphores.end(), m_presentSemaphore);""")
replace(vulkan, "        VK_CHECK_RE(res, m_ddt.ResetFences(m_device, 1, &m_acquireFence[m_acquireIndex]));", "        // Reset only after successful acquire: a failed acquire submits no fence signal.")
replace(vulkan,
        "        VK_CHECK_RE(res, m_ddt.WaitForFences(m_device, 1, &m_acquireFence[m_acquireIndex], VK_TRUE, timeout));",
        """        m_presentSemaphore = VK_NULL_HANDLE;
        res = m_ddt.WaitForFences(m_device, 1, &m_acquireFence[m_acquireIndex], VK_TRUE, timeout);
        if (res != VK_SUCCESS) return res; // VK_TIMEOUT is positive, but the slot is still in use.""")
replace(vulkan, """        VK_CHECK_RE(res, m_ddt.AcquireNextImageKHR(m_device, (VkSwapchainKHR)sc->native, timeout, m_acquireSemaphore[m_acquireIndex], nullptr, &bufferIndex));
        m_bufferToPresent = bufferIndex;""", """        const auto chainHandle = (VkSwapchainKHR)sc->native;
        auto found = m_presentSemaphores.find(chainHandle);
        if (found == m_presentSemaphores.end())
        {
            uint32_t imageCount{};
            VkResult countResult = m_ddt.GetSwapchainImagesKHR(m_device, chainHandle, &imageCount, nullptr);
            if (countResult != VK_SUCCESS) return countResult;
            if (imageCount == 0) return VK_ERROR_INITIALIZATION_FAILED;
            std::vector<VkSemaphore> semaphores(imageCount, VK_NULL_HANDLE);
            VkSemaphoreCreateInfo create{VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO};
            for (uint32_t i = 0; i < imageCount; ++i)
            {
                VkResult created = m_ddt.CreateSemaphore(m_device, &create, nullptr, &semaphores[i]);
                if (created != VK_SUCCESS)
                {
                    for (VkSemaphore semaphore : semaphores)
                        if (semaphore) m_ddt.DestroySemaphore(m_device, semaphore, nullptr);
                    return created;
                }
                sl::Resource named{};
                named.type = ResourceType::eFence;
                named.native = semaphores[i];
                auto name = extra::utf16ToUtf8(m_name.c_str()) + ".present-semaphore." + std::to_string(i);
                m_compute->setDebugName(&named, name.c_str());
            }
            found = m_presentSemaphores.emplace(chainHandle, std::move(semaphores)).first;
        }
        // Allocate synchronization before acquiring: allocation failure must not
        // leave a presentation-engine acquire signal without a consumer.
        res = m_ddt.AcquireNextImageKHR(m_device, chainHandle, timeout,
            m_acquireSemaphore[m_acquireIndex], nullptr, &bufferIndex);
        if (res != VK_SUCCESS && res != VK_SUBOPTIMAL_KHR) return res;
        if (bufferIndex >= found->second.size()) return VK_ERROR_INITIALIZATION_FAILED;
        VkResult resetResult = m_ddt.ResetFences(m_device, 1, &m_acquireFence[m_acquireIndex]);
        if (resetResult != VK_SUCCESS) return resetResult;
        m_presentSemaphore = found->second[bufferIndex];
        m_bufferToPresent = bufferIndex;""")

# Both public SDK transfers and NGX's private Vulkan commands use the same
# corrected dispatch. New-image tracking covers every mip/layer/aspect once.
dispatch = Path(__file__).parent / "ngx-vulkan-dispatch.h"
target = sdk / "source/platforms/sl.chi/ngxVulkanDispatch.h"
if target.exists():
    raise SystemExit("SDK dispatch correction already exists; use a fresh source copy")
target.write_bytes(dispatch.read_bytes())
for file in [vulkan, "source/plugins/sl.common/commonEntry.cpp"]:
    replace(file, '#include "source/platforms/sl.chi/vulkan.h"',
            '#include "source/platforms/sl.chi/vulkan.h"\n#include "source/platforms/sl.chi/ngxVulkanDispatch.h"')
replace(vulkan, "    m_ddt = m_vk->dispatchDeviceMap[m_device];", """    m_ddt = m_vk->dispatchDeviceMap[m_device];
    ngxDispatch::configure(m_device, m_vk->getInstanceProcAddr, m_vk->getDeviceProcAddr);
    m_ddt.CreateImage = ngxDispatch::CreateImage;
    m_ddt.DestroyImage = ngxDispatch::DestroyImage;
    m_ddt.CmdPipelineBarrier = ngxDispatch::Barrier;
    m_ddt.CmdPipelineBarrier2 = ngxDispatch::Barrier2;
    m_ddt.CmdCopyImage = ngxDispatch::CopyImage;
    m_ddt.CmdClearColorImage = ngxDispatch::ClearColor;""")
replace("source/plugins/sl.common/commonEntry.cpp",
        "nullptr/* TODO TBD plumb in vkGetInstanceProcAddr*/, nullptr/* TODO TBD plumb in vkGetDeviceProcAddr*/",
        "chi::ngxDispatch::GetInstanceProcAddr, chi::ngxDispatch::GetDeviceProcAddr", count=2)

replace("source/core/sl.interposer/vulkan/wrapper.cpp",
        "        bool synchronization2Enabled = false;",
        """        bool maintenance1Enabled = false;
        for (auto* p = (const VkBaseInStructure*)createInfo.pNext; p; p = p->pNext)
        {
            // KHR and EXT maintenance1 use the same structure type and ABI.
            if (p->sType == (VkStructureType)1000275000)
            {
                struct Feature { VkStructureType sType; const void* pNext; VkBool32 enabled; };
                maintenance1Enabled = ((const Feature*)p)->enabled == VK_TRUE;
            }
        }
        sl::param::getInterface()->set("sl.param.vulkanstory.maintenance1", maintenance1Enabled);
        bool synchronization2Enabled = false;""")
replace(vulkan,
        "    std::unordered_map<VkSwapchainKHR, std::vector<VkSemaphore>> m_presentSemaphores;",
        """    std::unordered_map<VkSwapchainKHR, std::vector<VkSemaphore>> m_presentSemaphores;
    struct PresentFence { VkFence fence{}; bool pending{}; };
    std::unordered_map<VkSwapchainKHR, std::vector<PresentFence>> m_presentFences;
    bool m_maintenance1Enabled{};""")
replace(vulkan, "        m_name = extra::utf8ToUtf16(debugName);",
        """        m_name = extra::utf8ToUtf16(debugName);
        m_maintenance1Enabled = maintenance1Enabled;""")
replace(vulkan, "uint32_t count, bool sync2Enabled)", "uint32_t count, bool sync2Enabled, bool maintenance1Enabled)")
replace(vulkan,
        "    tmp->init(this, m_vk, friendlyName, m_device, (CommandQueueVk*)queue, count, m_sync2SubmitEnabled);",
        """    bool maintenance1Enabled = false;
    m_parameters->get("sl.param.vulkanstory.maintenance1", &maintenance1Enabled);
    tmp->init(this, m_vk, friendlyName, m_device, (CommandQueueVk*)queue, count, m_sync2SubmitEnabled, maintenance1Enabled);""")
replace(vulkan, "        for (const auto& [chain, semaphores] : m_presentSemaphores)",
        """        for (const auto& [chain, fences] : m_presentFences)
            for (const auto& present : fences)
            {
                if (present.pending)
                    VK_CHECK_RV(m_ddt.WaitForFences(m_device, 1, &present.fence, VK_TRUE, UINT64_MAX));
                m_ddt.DestroyFence(m_device, present.fence, nullptr);
            }
        m_presentFences.clear();
        for (const auto& [chain, semaphores] : m_presentSemaphores)""")
replace(vulkan, "        // No present-id is chained here: SL does not use VK_KHR_present_wait, and LL2 frame",
        """        // Use a private fence only when the caller hasn't supplied one. The fence
        // covers the real WSI operation, unlike the SDK virtual-image acquisition.
        struct PresentFenceInfo { VkStructureType sType; const void* pNext; uint32_t count; const VkFence* fences; };
        PresentFenceInfo fenceInfo{(VkStructureType)1000275001, params, 1, nullptr};
        bool hasFence = false;
        for (auto* p = (const VkBaseInStructure*)params; p; p = p->pNext)
            hasFence |= p->sType == fenceInfo.sType;
        PresentFence* ownedFence = nullptr;
        if (m_maintenance1Enabled && !hasFence)
        {
            auto& fences = m_presentFences[swapChain];
            if (fences.empty()) fences.resize(m_presentSemaphores.at(swapChain).size());
            ownedFence = &fences.at(m_bufferToPresent);
            if (!ownedFence->fence)
            {
                VkFenceCreateInfo create{VK_STRUCTURE_TYPE_FENCE_CREATE_INFO};
                VkResult created = m_ddt.CreateFence(m_device, &create, nullptr, &ownedFence->fence);
                if (created != VK_SUCCESS) return created;
            }
            if (ownedFence->pending)
            {
                VkResult waited = m_ddt.WaitForFences(m_device, 1, &ownedFence->fence, VK_TRUE, UINT64_MAX);
                if (waited != VK_SUCCESS) return waited;
                VkResult reset = m_ddt.ResetFences(m_device, 1, &ownedFence->fence);
                if (reset != VK_SUCCESS) return reset;
                ownedFence->pending = false;
            }
            fenceInfo.fences = &ownedFence->fence;
        }
        // No present-id is chained here: SL does not use VK_KHR_present_wait, and LL2 frame""")
replace(vulkan, """            VK_STRUCTURE_TYPE_PRESENT_INFO_KHR,
            params,""", """            VK_STRUCTURE_TYPE_PRESENT_INFO_KHR,
            ownedFence ? &fenceInfo : params,""")
replace(vulkan, "        res = m_ddt.QueuePresentKHR(m_cmdQueue, &info);",
        """        res = m_ddt.QueuePresentKHR(m_cmdQueue, &info);
        // OUT_OF_DATE/SURFACE_LOST still enqueue the semaphore wait and fence.
        if (ownedFence && (res == VK_SUCCESS || res == VK_SUBOPTIMAL_KHR ||
            res == VK_ERROR_OUT_OF_DATE_KHR || res == VK_ERROR_SURFACE_LOST_KHR))
            ownedFence->pending = true;""")

replace("source/core/sl.security/secureLoadLibrary.cpp",
        '#include "include/sl_security.h"',
        '''#include "include/sl_security.h"
#if defined(SL_INTERPOSER)
#include "vulkanstory-common-hash.h"
#pragma comment(lib, "advapi32.lib")

// The source-built common plugin has no NVIDIA signature. Pin only this one
// locally built module; all closed NVIDIA plugins retain signature validation.
static HMODULE loadPinnedCommon(const wchar_t* path)
{
    HANDLE file = CreateFileW(path, GENERIC_READ, FILE_SHARE_READ, nullptr,
        OPEN_EXISTING, FILE_FLAG_SEQUENTIAL_SCAN, nullptr);
    if (file == INVALID_HANDLE_VALUE) return nullptr;
    HCRYPTPROV provider{};
    HCRYPTHASH hash{};
    BYTE digest[32]{};
    DWORD size = sizeof(digest);
    bool valid = CryptAcquireContextW(&provider, nullptr, nullptr, PROV_RSA_AES, CRYPT_VERIFYCONTEXT) &&
        CryptCreateHash(provider, CALG_SHA_256, 0, 0, &hash);
    BYTE bytes[65536];
    DWORD read{};
    while (valid)
    {
        if (!ReadFile(file, bytes, sizeof(bytes), &read, nullptr)) { valid = false; break; }
        if (!read) break;
        valid = CryptHashData(hash, bytes, read, 0) != FALSE;
    }
    valid = valid && CryptGetHashParam(hash, HP_HASHVAL, digest, &size, 0) &&
        size == sizeof(vulkanstoryCommonSha256) &&
        memcmp(digest, vulkanstoryCommonSha256, size) == 0;
    if (hash) CryptDestroyHash(hash);
    if (provider) CryptReleaseContext(provider, 0);
    HMODULE module = valid ? LoadLibraryW(path) : nullptr;
    CloseHandle(file);
    return module;
}
#endif''')
replace("source/core/sl.security/secureLoadLibrary.cpp",
        "    HMODULE mod = {};",
        '''#if defined(SL_INTERPOSER)
    const wchar_t* name = wcsrchr(path, L'\\\\');
    const wchar_t* slash = wcsrchr(path, L'/');
    if (!name || (slash && slash > name)) name = slash;
    name = name ? name + 1 : path;
    if (_wcsicmp(name, L"sl.common.dll") == 0) return loadPinnedCommon(path);
#endif
    HMODULE mod = {};''')

# Godot supplies individual 1.3 features. The SDK only merges synchronization2
# in that branch, and its compute backend creates private-data slots based on
# function availability rather than the enabled feature. Keep the ABI unchanged
# and publish the actual enablement through the existing shared parameter store.
wrapper = "source/core/sl.interposer/vulkan/wrapper.cpp"
replace(wrapper, "std::unordered_set<std::string> requiredSLPhysicalDevice13FeatureNames{};",
        'std::unordered_set<std::string> requiredSLPhysicalDevice13FeatureNames{"privateData"};')
replace(wrapper, "VkPhysicalDevicePrivateDataFeatures*                    pClientPrivateDataFeatures{};",
        "VkPhysicalDevicePrivateDataFeatures                     privateDataFeatures{}, *pClientPrivateDataFeatures{};")
replace(wrapper, "            if ((requiredSLPhysicalDevice13Features.synchronization2 || s_vk.nativeOpticalFlowHWSupport)",
        """            if (requiredSLPhysicalDevice13Features.privateData && supportedPhysicalDevice13Features.privateData)
            {
                if (!pClientPrivateDataFeatures)
                {
                    privateDataFeatures.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PRIVATE_DATA_FEATURES;
                    privateDataFeatures.pNext = (void*)createInfo.pNext;
                    createInfo.pNext = pClientPrivateDataFeatures = &privateDataFeatures;
                }
                pClientPrivateDataFeatures->privateData = VK_TRUE;
                requiredSLDeviceExtensionNames.emplace(VK_EXT_PRIVATE_DATA_EXTENSION_NAME);
            }
            if ((requiredSLPhysicalDevice13Features.synchronization2 || s_vk.nativeOpticalFlowHWSupport)""")
replace(wrapper, "        std::vector<const char*> extensions;", """        if (requiredSLDeviceExtensionNames.count("VK_NV_low_latency2"))
        {
            // NV low_latency2 requires present_id (or present_id2). The pinned
            // Reflex plugin requests LL2 but omits this dependency.
            requiredSLDeviceExtensionNames.emplace(VK_KHR_PRESENT_ID_EXTENSION_NAME);
        }
        std::vector<const char*> extensions;""")
replace(wrapper, "        sl::param::getInterface()->set(sl::param::global::kVulkanSynchronization2Enabled, synchronization2Enabled);",
        """        sl::param::getInterface()->set(sl::param::global::kVulkanSynchronization2Enabled, synchronization2Enabled);
        bool privateDataEnabled = false;
        for (auto* p = (const VkBaseInStructure*)createInfo.pNext; p; p = p->pNext)
        {
            if (p->sType == VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PRIVATE_DATA_FEATURES)
                privateDataEnabled |= ((const VkPhysicalDevicePrivateDataFeatures*)p)->privateData == VK_TRUE;
            else if (p->sType == VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_3_FEATURES)
                privateDataEnabled |= ((const VkPhysicalDeviceVulkan13Features*)p)->privateData == VK_TRUE;
        }
        sl::param::getInterface()->set("rsf.vulkan.privateDataEnabled", privateDataEnabled);""")
replace(vulkan, "    if (m_ddt.CreatePrivateDataSlot)", """    bool privateDataEnabled = false;
    m_parameters->get("rsf.vulkan.privateDataEnabled", &privateDataEnabled);
    if (privateDataEnabled && m_ddt.CreatePrivateDataSlot)""")

replace("source/platforms/sl.chi/vknvll2.cpp", """        m_swapchain = (VkSwapchainKHR)chain;
        m_isLatencyModeEnabled = isLatencyModeEnabled;""", """        if (!isLatencyModeEnabled)
        {
            SL_LOG_INFO("RSF Reflex secondary swapchain ignored: %p owner=%p", (void*)chain, (void*)m_swapchain);
            return ComputeStatus::eOk;
        }
        SL_LOG_INFO("RSF Reflex primary swapchain selected: %p previous=%p", (void*)chain, (void*)m_swapchain);
        m_swapchain = (VkSwapchainKHR)chain;
        m_isLatencyModeEnabled = true;""")

replace("source/platforms/sl.chi/vknvll2.cpp", "class VkNvLowLatency2 : public IReflexVk", """namespace { std::mutex rsfLatencySwapchainMutex; }
// Host WSI and LL2 calls share this lock. It covers the immediate Vulkan call,
// never the subsequent timeline-semaphore wait inside Reflex sleep.
extern "C" __declspec(dllexport) VkResult rsfSlVulkanQueuePresent(PFN_vkQueuePresentKHR present, VkQueue queue, const VkPresentInfoKHR* info)
{
    if (!present || !queue || !info) return VK_ERROR_INITIALIZATION_FAILED;
    std::lock_guard<std::mutex> lock(rsfLatencySwapchainMutex);
    return present(queue, info);
}
class VkNvLowLatency2 : public IReflexVk""")
replace("source/platforms/sl.chi/vknvll2.cpp", "class VkNvLowLatency2 : public IReflexVk", """extern "C" __declspec(dllexport) int32_t rsfSlVulkanWsiCall(int32_t (*callback)(void*), void* user)
{
    if (!callback || !user) return VK_ERROR_INITIALIZATION_FAILED;
    std::lock_guard<std::mutex> lock(rsfLatencySwapchainMutex);
    return callback(user);
}
class VkNvLowLatency2 : public IReflexVk""")
replace("source/platforms/sl.chi/vknvll2.cpp", "    std::mutex m_swapchainMtx;", "    std::mutex& m_swapchainMtx = rsfLatencySwapchainMutex;")

(sdk / "vulkanstory-fixes.json").write_text(json.dumps({
    "sdkCommit": "2122257e0fce486f91b385aa63b9a09b0a34b363",
    "edits": edits,
}, indent=2), encoding="utf-8")
print(f"Applied {len(edits)} checked SDK corrections to {sdk}")
