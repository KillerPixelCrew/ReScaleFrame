#pragma once

// Runs only from the DirectInput8Create InitOnce callback, outside DllMain.
void thin_startup() noexcept;
