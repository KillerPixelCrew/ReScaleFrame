/* SPDX-License-Identifier: MIT */
/* The guarded host log behind rsf_game_sys::host_log, bundled into every Rust archive that uses
   it (sdk/game/build.rs). */
#include <rescaleframe/foreign_guard.h>

bool rsf_game_log_guarded(rsf_game_log_fn log, void* user, const char* message)
{
    bool returned = true;
    if (!log) return true;
    RSF_FOREIGN_TRY { log(user, message); }
    RSF_FOREIGN_CATCH { returned = false; }
    return returned;
}