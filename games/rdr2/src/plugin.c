/* SPDX-License-Identifier: MIT */
/* The adapter's code is the rsf-rdr2 archive, which defines rsf_get_game_plugin_api (plugin.def
   exports it). The Visual Studio generator links a DLL only when its target compiles a source, so
   this unit carries the API header the export implements and nothing else. */
#include <rescaleframe/game_api.h>
