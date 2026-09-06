/* SPDX-License-Identifier: GPL-3.0-only */
/* The research proxy's DLSS path, kept behind four functions so the proxy itself stays readable.

   This is not a public interface. It exists inside the loader because the loader is what gets into
   the game today, and every part of it belongs elsewhere: see the comment at the top of
   dlss_bridge.c and the ownership rules in AGENTS.md. */

#ifndef RSF_DLSS_BRIDGE_H
#define RSF_DLSS_BRIDGE_H

typedef void (*rsf_bridge_log_fn)(void* user, const char* message);

/* Acquire the game's device, start the backend, and begin watching frames.

   Returns non-zero when everything came up. Returning zero is a normal outcome early in startup,
   when the game has not created a device yet, so a caller may try again rather than give up. */
int rsf_bridge_start(const char* streamline_directory, unsigned long output_width,
                     unsigned long output_height, unsigned long quality, rsf_bridge_log_fn log,
                     void* log_user);

/* Write the counters to the log. Worth doing on a key press: a run that produced no image should
   be able to say which step it stopped at, and every counter here has been the answer at least
   once. */
void rsf_bridge_report(void);

/* Ask for the next evaluated frame's output to be written out. */
void rsf_bridge_request_dump(const char* prefix);

int rsf_bridge_running(void);

#endif /* RSF_DLSS_BRIDGE_H */
