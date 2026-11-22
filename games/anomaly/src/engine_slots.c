/* SPDX-License-Identifier: MIT */
#include <rescaleframe/foreign_guard.h>
#include "engine_slots.h"
#include <windows.h>

bool rsf_anomaly_compare_slot(void* address, void* expected, void* value, void** previous)
{
#ifdef _MSC_VER
    RSF_FOREIGN_TRY {
#endif
        *previous = InterlockedCompareExchangePointer((void* volatile*)address, value, expected);
        return true;
#ifdef _MSC_VER
    } RSF_FAULT_CATCH { return false; }
#endif
}
