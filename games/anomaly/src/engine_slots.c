/* SPDX-License-Identifier: MIT */
#include "engine_slots.h"
#include <windows.h>

bool rsf_anomaly_compare_slot(void* address, void* expected, void* value, void** previous)
{
#ifdef _MSC_VER
    __try {
#endif
        *previous = InterlockedCompareExchangePointer((void* volatile*)address, value, expected);
        return true;
#ifdef _MSC_VER
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
#endif
}
