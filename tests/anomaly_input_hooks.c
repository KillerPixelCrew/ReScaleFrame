// SPDX-License-Identifier: MIT
#include "input_hooks.h"
#include <string.h>
#include <windows.h>

static bool read_image(void* output, const void* input, size_t size) {
    if (!output || !input) return false;
    if (!size) return true;
#ifdef _MSC_VER
    __try {
        memcpy(output, input, size);
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
#else
    const DWORD error = GetLastError();
    SIZE_T copied = 0;
    const bool complete = ReadProcessMemory(GetCurrentProcess(), input, output, size, &copied) && copied == size;
    SetLastError(error);
    return complete;
#endif
}

static bool exchange_slot(void* slot, void* expected, void* value, void** previous) {
#ifdef _MSC_VER
    __try {
#endif
        *previous = InterlockedCompareExchangePointer((void* volatile*)slot, value, expected);
        return true;
#ifdef _MSC_VER
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
#endif
}

int rsf_anomaly_input_fixture(int argc, const char* const* argv, rsf_anomaly_input_read, rsf_anomaly_input_exchange);

int main(int argc, char** argv) {
    return rsf_anomaly_input_fixture(argc, (const char* const*)argv, read_image, exchange_slot);
}
