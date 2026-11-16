// SPDX-License-Identifier: GPL-3.0-only
#pragma once
// Synchronous current-thread receipt, not GPU quiescence. Provider writes are
// isolated to private output; any native writes are ordered on the immediate
// context. Unknown exceptions return false. The host must also verify device life.
bool rsf_native_sr_native_destinations_ordered();
