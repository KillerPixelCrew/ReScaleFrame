// SPDX-License-Identifier: MIT
// Keep a real compiler-emitted RTTI and vtable specimen in the fixture executable.
#include <rescaleframe/module_dump.h>
#include <windows.h>

struct RsfDumpProbeBase {
    virtual ~RsfDumpProbeBase() = default;
    virtual int value() const { return 1; }
};

struct RsfDumpProbeDerived : RsfDumpProbeBase {
    int value() const override { return 2; }
};

extern "C" int rsf_test_module_dump_v2(int, char**, uint64_t, decltype(&rsf_dump_module));

int main(int argc, char** argv)
{
    RsfDumpProbeBase* probe = new RsfDumpProbeDerived();
    volatile int value = probe->value();
    (void)value;
    const auto table = *reinterpret_cast<const uintptr_t*>(probe);
    const auto base = reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    const int result = rsf_test_module_dump_v2(argc, argv, table - base, rsf_dump_module);
    delete probe;
    return result;
}
