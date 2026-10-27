// SPDX-License-Identifier: GPL-3.0-only
#include "engine_settings.h"
#include "engine_settings_policy.h"
#include <windows.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
#include <MinHook.h>
#include <array>
#include <atomic>
#include <cstdio>
#include <cstring>
namespace {
using Copy = void* (*)(void*, const void*);
using Method = void (*)(void*);
Copy original_copy{};
Method original_load{}, original_apply{}, original_reconfigure{};
using RefreshDimensions = void (*)();
RefreshDimensions original_refresh_dimensions{};
using Optimal = void (*)(const uint32_t*, void*);
Optimal original_optimal{};
using Dimensions = void (*)(uint32_t*,uint32_t*,uint32_t,uint32_t,uint32_t);
Dimensions original_fsr_dimensions{};
uintptr_t image{};
// Low32: RSF quality. High32: native selector index in the active capability list.
std::atomic<uint64_t> requested_selection{(uint64_t(UINT32_MAX)<<32)|2};
uint32_t requested_quality() { return static_cast<uint32_t>(requested_selection.load()); }
std::atomic<uint64_t> adapter_vendor{UINT64_MAX};
std::atomic<uint32_t> overrides{}, failures{};
bool installed{};
std::atomic<rsf_game_log_fn> logger{};
std::atomic<void*> logger_user{};
// rage__fwuiSystemSettingsCollection offsets from its generated parser tables.
// Collection graphics at +b4: dlssIndex +34, fsr2Index +40.
constexpr size_t kDlss = 0xe8, kDlssQuality = 0xec, kFsr = 0xf4, kVideoAdapter = 0x124;
// Collection video at +124: ReflexSettings +38. Keep native marker/sleep calls
// enabled; the latency hook replaces their driver options with RSF's preference.
constexpr size_t kReflex = 0x15c, kVersion = 0x178, kOwnerCollection = 0xb0;
uint32_t word(const void* object, size_t offset)
{ uint32_t value; std::memcpy(&value, static_cast<const unsigned char*>(object) + offset, 4); return value; }
void own(void* collection)
{
    // Only typed collection copy and the verified settings-owner methods reach here.
    uint16_t version{};
    std::memcpy(&version, static_cast<const unsigned char*>(collection) + kVersion, 2);
    if (version != 0 && version != 37) { ++failures; return; }
    const auto selected = word(collection, kVideoAdapter);
    auto cached = adapter_vendor.load();
    if (selected != static_cast<uint32_t>(cached >> 32)) {
        Microsoft::WRL::ComPtr<IDXGIFactory1> factory;
        Microsoft::WRL::ComPtr<IDXGIAdapter1> gpu;
        DXGI_ADAPTER_DESC1 desc{};
        uint32_t detected{};
        if (SUCCEEDED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))) &&
            SUCCEEDED(factory->EnumAdapters1(selected, &gpu)) && SUCCEEDED(gpu->GetDesc1(&desc))) detected = desc.VendorId;
        cached = (static_cast<uint64_t>(selected) << 32) | detected;
        adapter_vendor = cached;
    }
    // FSR2 is the universal carrier when native NGX cannot exist. Both carriers
    // enter the same canonical DLSS-shaped RSF adapter; neither selects RSF's vendor.
    const auto vendor = static_cast<uint32_t>(cached);
    const auto selection = requested_selection.load();
    const auto quality = static_cast<uint32_t>(selection);
    auto expected = std::array<unsigned char,0x180>{};
    std::memcpy(expected.data(),collection,expected.size());
    if (!rsf_rdr2_pin_engine_settings(expected.data(),expected.size(),vendor,quality)) { ++failures; return; }
    const auto native_index = static_cast<uint32_t>(selection >> 32);
    if (vendor==0x10de && native_index!=UINT32_MAX) std::memcpy(expected.data()+kDlss,&native_index,4);
    const auto dlss = word(expected.data(),kDlss), fsr = word(expected.data(),kFsr);
    const auto mode = word(expected.data(),kDlssQuality);
    const auto old_dlss = word(collection, kDlss), old_fsr = word(collection, kFsr), old_reflex = word(collection, kReflex);
    const auto old_quality = word(collection, kDlssQuality);
    const bool changed = old_dlss != dlss || old_quality != mode || old_fsr != fsr || old_reflex != 1;
    for (const auto offset : {kDlss,kDlssQuality,kFsr,kReflex})
        std::memcpy(static_cast<unsigned char*>(collection)+offset,expected.data()+offset,4);
    const auto log = logger.load(std::memory_order_acquire);
    if (changed && ++overrides <= 32 && log) {
        char text[192]; std::snprintf(text, sizeof(text),
            "RDR2 input carrier enforced: vendor=%x DLSS=%u->%u quality=%u->%u FSR2=%u->%u native_marker_mode=%u->1; RSF settings unchanged.",
            vendor, old_dlss, dlss, old_quality, mode, old_fsr, fsr, old_reflex);
        log(logger_user.load(), text);
    }
}
void* copy(void* destination, const void* source)
{ auto* result = original_copy(destination, source); own(destination); return result; }
void load(void* owner)
{ original_load(owner); own(static_cast<unsigned char*>(owner) + kOwnerCollection); }
void apply(void* owner)
{ own(static_cast<unsigned char*>(owner) + kOwnerCollection); original_apply(owner); }
void optimal(const uint32_t* output, void* result)
{
    original_optimal(output,result);
    // RAGE has no DLAA mode. Keep its active Quality carrier, but supply native
    // dimensions at the producer used by allocations AND NGX feature setup.
    // Table: output[2], five optimal pairs, five minimum pairs, five maximum pairs.
    if (requested_quality()!=0 || !output[0] || !output[1] || output[0]>16384 || output[1]>16384) return;
    for (size_t offset=8;offset<0x80;offset+=8)
        std::memcpy(static_cast<unsigned char*>(result)+offset,output,8);
}
void fsr_dimensions(uint32_t* width,uint32_t* height,uint32_t output_width,uint32_t output_height,uint32_t mode)
{
    original_fsr_dimensions(width,height,output_width,output_height,mode);
    const auto quality=requested_quality();
    if (!output_width || !output_height || output_width>16384 || output_height>16384) return;
    if (quality==0) {*width=output_width;*height=output_height;}
    else if (quality==4) {*width=output_width/3;*height=output_height/3;}
}
void refresh_dimensions()
{
    original_refresh_dimensions();
    if (const auto log=logger.load(std::memory_order_acquire)) {
        char message[160];std::snprintf(message,sizeof(message),
            "RDR2 renderer dimensions refreshed: quality=%u input=%ux%u output=%ux%u thread=%lu.",
            requested_quality(),word(reinterpret_cast<void*>(image),0x3f0db38),
            word(reinterpret_cast<void*>(image),0x3f0db3c),word(reinterpret_cast<void*>(image),0x3f0db30),
            word(reinterpret_cast<void*>(image),0x3f0db34),GetCurrentThreadId());
        log(logger_user.load(),message);
    }
}
bool pin_pending(void* manager, uint32_t quality)
{
    using Owner = void* (*)(void*);
    struct Path { const uint32_t* ids; uint64_t count; };
    using Find = void* (*)(void*,const Path*,void*);
    constexpr uint32_t graphics_id=0x8c463422;
    const Path path{&graphics_id,1};
    auto* active=static_cast<unsigned char*>(reinterpret_cast<Owner>(image+0x2c76114)(manager));
    auto* pending=static_cast<unsigned char*>(reinterpret_cast<Find>(image+0x2c736f4)(
        manager,&path,static_cast<unsigned char*>(manager)+0xb8));
    if (!active || !pending || active==pending) return false;
    // The renderer reads manager+8. Native graphics Apply copies its values
    // FROM manager+b8. Changing the active collection is overwritten at reset.
    uint32_t native_index=UINT32_MAX;
    if (static_cast<uint32_t>(adapter_vendor.load())==0x10de) {
        std::array<uint32_t,7> choices{};
        const auto count=word(active,0x42c);
        if (!count || count>choices.size()) return false;
        std::memcpy(choices.data(),active+0x410,count*sizeof(uint32_t));
        native_index=rsf_rdr2_dlss_choice(choices.data(),count,quality);
        if (native_index==UINT32_MAX) return false;
    }
    requested_selection=(uint64_t(native_index)<<32)|quality;
    // Only the four RSF-owned scalar fields change. No string allocation or
    // borrowed collection copy is involved; all other pending settings survive.
    own(pending+kOwnerCollection);
    own(pending+0x240);
    return true;
}
void reconfigure(void* manager)
{
    // Also covers stock Apply: native Off or another native quality cannot
    // replace the RSF carrier when the game consumes its pending settings.
    if (!pin_pending(manager,requested_quality())) ++failures;
    original_reconfigure(manager);
}
}
bool rsf_rdr2_own_engine_settings(const wchar_t* configuration) noexcept
{
    if (installed) return true;
    if (!configuration) return false;
    const auto quality=GetPrivateProfileIntW(L"RDR2",L"SRQuality",2,configuration);
    requested_selection=(uint64_t(UINT32_MAX)<<32)|(quality<=4?quality:2);
    image = reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    struct Hook { uintptr_t rva; std::array<unsigned char,16> expected; void* replacement; void** original; };
    const Hook hooks[]{
        {0x20b7f8, {0x48,0x89,0x5c,0x24,0x08,0x57,0x48,0x83,0xec,0x20,0x0f,0x10,0x02,0x48,0x8b,0xd9}, reinterpret_cast<void*>(&copy), reinterpret_cast<void**>(&original_copy)},
        {0x2c7c9a4, {0x48,0x8b,0xc4,0x48,0x89,0x58,0x08,0x48,0x89,0x70,0x10,0x48,0x89,0x78,0x18,0x4c}, reinterpret_cast<void*>(&load), reinterpret_cast<void**>(&original_load)},
        {0x2c7e6c0, {0x40,0x53,0x48,0x83,0xec,0x20,0x48,0x8b,0xd9,0xe8,0xca,0xba,0xff,0xff,0x48,0x8b}, reinterpret_cast<void*>(&apply), reinterpret_cast<void**>(&original_apply)},
        {0x26f5270, {0x48,0x89,0x5c,0x24,0x08,0x55,0x56,0x57,0x41,0x54,0x41,0x55,0x41,0x56,0x41,0x57}, reinterpret_cast<void*>(&optimal), reinterpret_cast<void**>(&original_optimal)},
        {0x26ecc3c, {0x48,0x8b,0xc4,0x48,0x89,0x58,0x08,0x48,0x89,0x70,0x10,0x48,0x89,0x78,0x18,0x55}, reinterpret_cast<void*>(&fsr_dimensions), reinterpret_cast<void**>(&original_fsr_dimensions)},
        {0x58a908, {0x48,0x81,0xec,0x38,0x01,0x00,0x00,0x48,0x8d,0x4c,0x24,0x20,0xe8,0xcf,0xd7,0xfc}, reinterpret_cast<void*>(&refresh_dimensions), reinterpret_cast<void**>(&original_refresh_dimensions)},
        {0x2c7e8f4, {0x40,0x53,0x48,0x83,0xec,0x20,0xba,0x09,0x00,0x00,0x00,0x48,0x8b,0xd9,0x39,0x91}, reinterpret_cast<void*>(&reconfigure), reinterpret_cast<void**>(&original_reconfigure)}
    };
    for (const auto& hook : hooks)
        if (std::memcmp(reinterpret_cast<void*>(image + hook.rva), hook.expected.data(), hook.expected.size())) return false;
    constexpr unsigned char manager_bytes[]{0x48,0x8b,0x05,0xe9,0xac,0x1c,0x01,0xc3};
    constexpr unsigned char owner_bytes[]{0x4c,0x8b,0xdc,0x53,0x48,0x81,0xec,0xb0,0x00,0x00,0x00,0x49,0x8d,0x43,0x08};
    constexpr unsigned char find_bytes[]{0x40,0x53,0x48,0x83,0xec,0x30,0x48,0x83,0x7a,0x08,0x00,0x49,0x8b,0xc0};
    if (std::memcmp(reinterpret_cast<void*>(image+0x2cf31f8),manager_bytes,sizeof(manager_bytes)) ||
        std::memcmp(reinterpret_cast<void*>(image+0x2c76114),owner_bytes,sizeof(owner_bytes)) ||
        std::memcmp(reinterpret_cast<void*>(image+0x2c736f4),find_bytes,sizeof(find_bytes))) return false;
    const auto initialized = MH_Initialize();
    if (initialized != MH_OK && initialized != MH_ERROR_ALREADY_INITIALIZED) return false;
    size_t created{};
    for (; created < std::size(hooks); ++created) {
        const auto& hook = hooks[created];
        if (MH_CreateHook(reinterpret_cast<void*>(image + hook.rva), hook.replacement, hook.original) != MH_OK) break;
    }
    if (created != std::size(hooks)) {
        for (size_t i = 0; i < created; ++i) MH_RemoveHook(reinterpret_cast<void*>(image + hooks[i].rva));
        return false;
    }
    for (const auto& hook : hooks) if (MH_QueueEnableHook(reinterpret_cast<void*>(image + hook.rva)) != MH_OK) {
        for (const auto& entry : hooks) MH_RemoveHook(reinterpret_cast<void*>(image + entry.rva));
        return false;
    }
    installed = MH_ApplyQueued() == MH_OK;
    if (!installed) for (const auto& hook : hooks) {
        MH_DisableHook(reinterpret_cast<void*>(image + hook.rva));
        MH_RemoveHook(reinterpret_cast<void*>(image + hook.rva));
    }
    return installed;
}
void rsf_rdr2_engine_quality_tick(uint32_t quality) noexcept
{
    static bool pending_initialized{}; // Only the registered game fiber calls this.
    if (!installed || quality>4) return;
    const bool changed=requested_quality()!=quality;
    if (pending_initialized && !changed) return;
    using Manager = void* (*)();
    auto* manager=reinterpret_cast<Manager>(image+0x2cf31f8)();
    if (!manager) return;
    const auto stage=word(manager,0x2c8);
    // Native initialization and an existing restart/apply operation own their
    // state until completion. Retain the request and retry on a later game tick.
    if (stage<=2 || stage==8 || stage==9) return;
    if (!pin_pending(manager,quality)) return;
    pending_initialized=true;
    if (!changed) return; // Startup already allocated the selected input size.
    // Enter the native graphics reconfiguration state9. This calls the concrete
    // manager's +60 method (+23b570), which requests the renderer's synchronized
    // reset through its +188 method (+266dd1c). Settings notifications alone
    // (+2c7d190) do not request this reset and leave old allocations in use.
    // The engine completes state9 and refreshes dimensions at its own safe point.
    original_reconfigure(manager);
    if (const auto log=logger.load(std::memory_order_acquire)) {
        char text[176];std::snprintf(text,sizeof(text),"RDR2 quality request=%u: pending owner updated, manager=%p stage=%u->%u; native graphics reconfiguration requested.",quality,manager,stage,word(manager,0x2c8));
        log(logger_user.load(),text);
    }
}
void rsf_rdr2_report_engine_settings(rsf_game_log_fn log, void* user) noexcept
{
    if (!log) return;
    logger_user = user; logger.store(log, std::memory_order_release);
    const auto cached = adapter_vendor.load();
    char text[240]; std::snprintf(text, sizeof(text),
        "RDR2 engine settings ownership %s: copy+20b7f8/load+2c7c9a4/apply+2c7e6c0; adapter=%u vendor=%x overrides=%u refused=%u; RSF overlay owns SR/latency.",
        installed ? "armed" : "REFUSED", static_cast<uint32_t>(cached >> 32), static_cast<uint32_t>(cached), overrides.load(), failures.load());
    log(user, text);
}
