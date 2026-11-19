// SPDX-License-Identifier: MIT
#define CINTERFACE
#include <initguid.h>
#include <d3d12.h>
#include <rescaleframe/fg_leases.h>
#include <cstdio>
#include <cstdlib>

static void check(bool ok, int line)
{
    if (!ok) {
        std::fprintf(stderr, "Lease failure-path check failed at %d\n", line);
        std::exit(1);
    }
}
#define CHECK(x) check((x), __LINE__)

// These COM objects provide only the methods the runtime uses. No device is created.
template<class Interface>
struct Mock {
    Interface iface;
    ULONG refs = 1;
    Mock<ID3D12Device>* device = nullptr;
    uint64_t completed = 0;

    static Mock& from(Interface* self) { return *reinterpret_cast<Mock*>(self); }

    static ULONG STDMETHODCALLTYPE add_ref(Interface* self)
    {
        auto& refs = from(self).refs;
        CHECK(refs != 0);
        return ++refs;
    }

    static ULONG STDMETHODCALLTYPE release(Interface* self)
    {
        auto& refs = from(self).refs;
        CHECK(refs != 0);
        return --refs;
    }

    static HRESULT STDMETHODCALLTYPE get_device(Interface* self, REFIID iid, void** out)
    {
        CHECK(IsEqualIID(iid, IID_ID3D12Device));
        auto* device = from(self).device;
        *out = device ? &device->iface : nullptr;
        if (!device) return E_NOINTERFACE;
        Mock<ID3D12Device>::add_ref(&device->iface);
        return S_OK;
    }

    static uint64_t STDMETHODCALLTYPE completed_value(Interface* self)
    {
        return from(self).completed;
    }
};

static void check_fence_failure(bool queue_removed)
{
    constexpr uint64_t failed_id = 2;
    constexpr uint64_t completed_id = 3;
    ID3D12DeviceVtbl device_methods{};
    device_methods.AddRef = Mock<ID3D12Device>::add_ref;
    device_methods.Release = Mock<ID3D12Device>::release;
    Mock<ID3D12Device> device{{&device_methods}}, foreign{{&device_methods}};

    ID3D12ResourceVtbl resource_methods{};
    resource_methods.AddRef = Mock<ID3D12Resource>::add_ref;
    resource_methods.Release = Mock<ID3D12Resource>::release;
    resource_methods.GetDevice = Mock<ID3D12Resource>::get_device;
    Mock<ID3D12Resource> resource{{&resource_methods}, 1, &device};
    Mock<ID3D12Resource> other_resource{{&resource_methods}, 1, &foreign};

    ID3D12FenceVtbl fence_methods{};
    fence_methods.AddRef = Mock<ID3D12Fence>::add_ref;
    fence_methods.Release = Mock<ID3D12Fence>::release;
    fence_methods.GetDevice = Mock<ID3D12Fence>::get_device;
    fence_methods.GetCompletedValue = Mock<ID3D12Fence>::completed_value;
    Mock<ID3D12Fence> queue{{&fence_methods}, 1, &device};
    Mock<ID3D12Fence> vendor{{&fence_methods}, 1, &device};
    Mock<ID3D12Fence> foreign_fence{{&fence_methods}, 1, &foreign};
    Mock<ID3D12Fence> completed{{&fence_methods}, 1, &device, 1};

    rsf_fg_leases* leases = nullptr;
    CHECK(rsf_fg_leases_create(2, &leases) == RSF_BACKEND_OK);
    void* resources[] = {&resource.iface, &other_resource.iface};
    CHECK(rsf_fg_leases_acquire(leases, failed_id, 1, resources, 2) == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    CHECK(resource.refs == 1 && other_resource.refs == 1 && device.refs == 1 && foreign.refs == 1);
    other_resource.device = nullptr;
    CHECK(rsf_fg_leases_acquire(leases, failed_id, 1, resources, 2) == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    CHECK(resource.refs == 1 && other_resource.refs == 1 && device.refs == 1);
    resources[1] = &resource.iface;
    CHECK(rsf_fg_leases_acquire(leases, failed_id, 1, resources, 2) == RSF_BACKEND_OK);
    CHECK(rsf_fg_leases_submit(leases, failed_id, &foreign_fence.iface, 1) == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    rsf_fg_retirement retirement{sizeof(retirement), &foreign_fence.iface, 1};
    CHECK(rsf_fg_leases_seal(leases, failed_id, &retirement) == RSF_BACKEND_ERROR_NOT_READY);
    CHECK(rsf_fg_leases_submit(leases, failed_id, &queue.iface, 1) == RSF_BACKEND_OK);
    CHECK(rsf_fg_leases_seal(leases, failed_id, &retirement) == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    CHECK(foreign_fence.refs == 1 && device.refs == 1 && foreign.refs == 1);
    retirement.fence = &vendor.iface;
    CHECK(rsf_fg_leases_seal(leases, failed_id, &retirement) == RSF_BACKEND_OK);

    CHECK(rsf_fg_leases_acquire(leases, completed_id, 1, resources, 1) == RSF_BACKEND_OK);
    CHECK(rsf_fg_leases_submit(leases, completed_id, &completed.iface, 1) == RSF_BACKEND_OK);
    retirement.fence = nullptr;
    retirement.value = 0;
    CHECK(rsf_fg_leases_seal(leases, completed_id, &retirement) == RSF_BACKEND_OK);

    // Removal on either fence must win even while the other fence is pending.
    auto& removed = queue_removed ? queue : vendor;
    removed.completed = UINT64_MAX;
    uint32_t pending = 0;
    CHECK(rsf_fg_leases_collect(leases, &pending) == RSF_BACKEND_ERROR_FEATURE_FAILED && pending == 1);
    CHECK(completed.refs == 1); // Collection continued past the failed slot.
    CHECK(rsf_fg_leases_destroy(leases) == RSF_BACKEND_ERROR_FEATURE_FAILED);
    CHECK(resource.refs == 3 && queue.refs == 2 && vendor.refs == 2);
    CHECK(rsf_fg_leases_acquire(leases, 4, 1, resources, 1) == RSF_BACKEND_ERROR_NOT_READY);
    CHECK(rsf_fg_leases_cancel(leases, failed_id) == RSF_BACKEND_ERROR_NOT_READY);

    // Restore the synthetic fence values to check the retained references can retire normally.
    queue.completed = 1;
    vendor.completed = 1;
    CHECK(rsf_fg_leases_destroy(leases) == RSF_BACKEND_OK);
    CHECK(resource.refs == 1 && queue.refs == 1 && vendor.refs == 1 && device.refs == 1);
}

int main()
{
    check_fence_failure(true);
    check_fence_failure(false);
}
