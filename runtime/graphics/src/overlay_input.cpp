// SPDX-License-Identifier: MIT

#include <rescaleframe/overlay_input.h>
#include "overlay_pointer_platform.h"
#include "overlay_visibility_platform.h"
#include "overlay_window_platform.h"

#include <windows.h>

#include <windowsx.h>

#include <MinHook.h>

#include <rescaleframe/log.h>

#include <atomic>
#include <climits>
#include <cstddef>
#include <initializer_list>
#include <mutex>

namespace {

constexpr size_t raw_mouse_bytes = offsetof(RAWINPUT, data) + sizeof(RAWMOUSE);
static_assert(sizeof(rsf_overlay_client_point) == 16 && RSF_OVERLAY_MOUSE_LEFT == 1);

// A cursor the user can see is not solved here, and this is the wrong module to solve it in.
//
// A game that plays with a mouse hides the system cursor once and keeps it hidden, usually with
// ShowCursor and often with ClipCursor holding it inside the client area, and some re-apply both
// every frame from their own input tick. Calling ShowCursor(TRUE) from here would fight that loop
// and lose, and the ShowCursor counter is process wide, so getting the count wrong leaves the game
// with a permanently visible cursor after the overlay closes.
//
// What would solve it, in rough order of how invasive each is: draw the pointer as part of the
// overlay from the position collected here, which needs a cursor in the egui side and no Win32
// calls at all; answer WM_SETCURSOR while the overlay is visible so the game's own handler never
// runs; hook ShowCursor, SetCursor and ClipCursor for the duration. The first is the one that does
// not depend on how the game manages its cursor, and it is the one to try first.

struct State {
    /* The user32 detours that make the real cursor usable while the panel is open.

       This is what SpecialK, ReShade and OptiScaler do, and it is why their panels take a mouse in
       a game that locks it (ReShade `input_windows.cpp`, SpecialK `input/cursor.cpp` and
       `input/raw_input.cpp`, read 26 September 2026). A game like AC7 warps the pointer to the
       centre every frame with SetCursorPos, keeps it inside the window with ClipCursor, hides it
       with ShowCursor and SetCursor, and steers from raw input. Following raw deltas around all of
       that, which this module did first, was chunky at best and unusable under a DPI scale. So
       while the panel is open: the warps are swallowed, the clip is lifted, the cursor is shown,
       the game is handed the position the cursor had when the panel opened, and the mouse fields of
       the raw input it reads are zeroed. The real cursor then moves freely and visibly, and the
       panel reads its absolute position each frame. Everything is put back on close. */
    struct CursorHooks {
        bool installed = false;
        BOOL(WINAPI* set_cursor_pos)(int, int) = nullptr;
        BOOL(WINAPI* clip_cursor)(const RECT*) = nullptr;
        BOOL(WINAPI* get_cursor_pos)(LPPOINT) = nullptr;
        int(WINAPI* show_cursor)(BOOL) = nullptr;
        HCURSOR(WINAPI* set_cursor)(HCURSOR) = nullptr;
        UINT(WINAPI* get_raw_input_data)(HRAWINPUT, UINT, LPVOID, PUINT, UINT) = nullptr;
        UINT(WINAPI* get_raw_input_buffer)(PRAWINPUT, PUINT, UINT) = nullptr;
        void* targets[7]{};
        size_t target_count = 0;
    } cursor;
    std::mutex cursor_guard;
    RECT last_clip{};
    bool have_clip = false;
    POINT frozen{};
    bool have_frozen = false;
    int cursor_shows = 0;
    std::atomic<unsigned long> swallowed_warps{0};
    std::atomic<unsigned long> swallowed_hides{0};
    /* Diagnostics only, counted while the overlay is visible so the hidden path stays free of them.
       They answer "did the panel's window receive pointer input and keep it from the game", which
       a screenshot cannot when the panel itself cannot be captured. */
    std::atomic<uint64_t> pointer_received{0}, pointer_consumed{0}, button_downs_consumed{0};
    std::atomic<uint64_t> raw_mouse_reports{0}, keys_consumed{0}, wheel_notches_consumed{0};
    std::atomic<uint64_t> frames_with_button{0}, frames_collected_visible{0};
    std::atomic<uint32_t> last_x{0}, last_y{0};
};

State& state()
{
    static State instance;
    return instance;
}

void log_to_rust(void*, const char* message) noexcept
{
    rsf_core_overlay_input_log(message);
}

uint32_t overlay_button_bit(uint32_t virtual_key)
{
    switch (virtual_key) {
    case VK_LBUTTON:
        return RSF_OVERLAY_MOUSE_LEFT;
    case VK_RBUTTON:
        return RSF_OVERLAY_MOUSE_RIGHT;
    case VK_MBUTTON:
        return RSF_OVERLAY_MOUSE_MIDDLE;
    default:
        // X buttons have no bit in the contract, so the overlay never sees them. They are still
        // tracked as held keys, because the game did see the press and needs the release.
        return 0;
    }
}

rsf_overlay_client_point client_point(HWND window, POINT point)
{
    RECT client{};
    if (!GetClientRect(window, &client) || client.right <= 0 || client.bottom <= 0) {
        return {point.x, point.y, 0, 0};
    }
    return {point.x, point.y, client.right, client.bottom};
}

void record_raw_mouse(State& self, HWND window, LPARAM lparam)
{
    // Use raw deltas only when the absolute cursor has not been freed.
    if (self.cursor.installed) return;
    RAWINPUT raw{};
    UINT size = sizeof(raw);
    const UINT read = GetRawInputData(reinterpret_cast<HRAWINPUT>(lparam), RID_INPUT, &raw, &size,
        sizeof(RAWINPUTHEADER));
    if (read == static_cast<UINT>(-1) || read < raw_mouse_bytes ||
        raw.header.dwType != RIM_TYPEMOUSE || (raw.data.mouse.usFlags & MOUSE_MOVE_ABSOLUTE) != 0) {
        return;
    }
    const POINT delta{raw.data.mouse.lLastX, raw.data.mouse.lLastY};
    if (delta.x == 0 && delta.y == 0) return;
    const auto facts = client_point(window, delta);
    rsf_core_overlay_pointer_raw(&facts);
}

void record_mouse_position(State& self, HWND window, LPARAM lparam)
{
    const POINT point{GET_X_LPARAM(lparam), GET_Y_LPARAM(lparam)};
    const bool cursor_freed = self.cursor.installed;
    const auto facts = cursor_freed ? rsf_overlay_client_point{point.x, point.y, 0, 0} :
        client_point(window, point);
    rsf_core_overlay_pointer_move(&facts, cursor_freed ? 1u : 0u);
}

void record_button(uint32_t virtual_key, bool down)
{
    const auto bit = overlay_button_bit(virtual_key);
    if (bit != 0) rsf_core_overlay_pointer_button(bit, down ? 1u : 0u);
}

uint32_t touch_flags(bool down, bool up)
{
    return (down ? uint32_t(RSF_TOUCH_DOWN) : 0u) | (up ? uint32_t(RSF_TOUCH_UP) : 0u);
}

void record_contact(HWND window, POINT point, uint32_t id, bool down, bool up)
{
    const auto facts = client_point(window, point);
    rsf_core_overlay_pointer_contact(&facts, id, touch_flags(down, up));
}

void seed_cursor_position(State& self)
{
    HWND window = static_cast<HWND>(rsf_core_overlay_input_window());
    POINT point{};
    // Call the genuine GetCursorPos so the sample is not the position frozen for the game.
    const bool have_point = self.cursor.installed ? self.cursor.get_cursor_pos(&point) != FALSE :
        GetCursorPos(&point) != FALSE;
    if (!window || !have_point || !ScreenToClient(window, &point)) return;
    const auto facts = client_point(window, point);
    rsf_core_overlay_pointer_seed(&facts);
}

// The detours. Each forwards to the genuine function through MinHook's trampoline unless the panel
// is open and the call is one the game makes to own the mouse.

BOOL WINAPI hooked_set_cursor_pos(int x, int y)
{
    State& self = state();
    if (rsf_core_overlay_visible()) {
        // The warp to the centre. Swallowed, and the game is told it worked.
        self.swallowed_warps.fetch_add(1, std::memory_order_relaxed);
        return TRUE;
    }
    return self.cursor.set_cursor_pos(x, y);
}

BOOL WINAPI hooked_clip_cursor(const RECT* rect)
{
    State& self = state();
    {
        std::lock_guard<std::mutex> lock(self.cursor_guard);
        if (rect) {
            self.last_clip = *rect;
            self.have_clip = true;
        } else {
            self.have_clip = false;
        }
    }
    if (rsf_core_overlay_visible()) {
        // Some games clip to a single pixel to keep the pointer still. Lifted for the panel; what
        // the game asked for is remembered and put back on close.
        return self.cursor.clip_cursor(nullptr);
    }
    return self.cursor.clip_cursor(rect);
}

BOOL WINAPI hooked_get_cursor_pos(LPPOINT point)
{
    State& self = state();
    if (point && rsf_core_overlay_visible()) {
        std::lock_guard<std::mutex> lock(self.cursor_guard);
        if (self.have_frozen) {
            // The game sees the pointer where it was when the panel opened, so nothing it derives
            // from the position moves while the panel is used.
            *point = self.frozen;
            return TRUE;
        }
    }
    return self.cursor.get_cursor_pos(point);
}

int WINAPI hooked_show_cursor(BOOL show)
{
    State& self = state();
    thread_local int game_count = 0;
    if (rsf_core_overlay_visible()) {
        // RAGE loops until ShowCursor(FALSE) returns negative (+263cf2a..34).
        // Querying the real counter with a show/hide pair leaves the overlay's
        // visible counter unchanged, so that loop never terminates. Keep the
        // game's logical counter separately and never touch user32 in this path.
        if (show) { if (game_count < INT_MAX) ++game_count; }
        else { if (game_count > INT_MIN) --game_count;
            self.swallowed_hides.fetch_add(1, std::memory_order_relaxed); }
        return game_count;
    }
    game_count = self.cursor.show_cursor(show);
    return game_count;
}

HCURSOR WINAPI hooked_set_cursor(HCURSOR handle)
{
    State& self = state();
    if (rsf_core_overlay_visible() && !handle) {
        // A null cursor is the other way a game hides it. The panel wants an arrow.
        return self.cursor.set_cursor(LoadCursorW(nullptr, MAKEINTRESOURCEW(32512)));
    }
    return self.cursor.set_cursor(handle);
}

// Zero the mouse in one raw input record, so a game steering from it holds still under the panel.
// Buttons too: a click on the panel must not fire the cannon.
void neutralise_raw_mouse(RAWINPUT* raw, size_t bytes)
{
    if (bytes >= raw_mouse_bytes && raw->header.dwType == RIM_TYPEMOUSE) {
        raw->data.mouse.lLastX = 0;
        raw->data.mouse.lLastY = 0;
        raw->data.mouse.usButtonFlags = 0;
        raw->data.mouse.usButtonData = 0;
    }
}

UINT WINAPI hooked_get_raw_input_data(HRAWINPUT handle, UINT command, LPVOID data, PUINT size,
                                      UINT header_size)
{
    State& self = state();
    const UINT result = self.cursor.get_raw_input_data(handle, command, data, size, header_size);
    if (result != static_cast<UINT>(-1) && data && command == RID_INPUT &&
        rsf_core_overlay_visible()) {
        neutralise_raw_mouse(static_cast<RAWINPUT*>(data), result);
    }
    return result;
}

UINT WINAPI hooked_get_raw_input_buffer(PRAWINPUT data, PUINT size, UINT header_size)
{
    State& self = state();
    const UINT result = self.cursor.get_raw_input_buffer(data, size, header_size);
    if (result == static_cast<UINT>(-1) || !data || result == 0 ||
        !rsf_core_overlay_visible()) {
        return result;
    }
    using QWORD = UINT64; // NEXTRAWINPUTBLOCK expects the name
    RAWINPUT* record = data;
    for (UINT index = 0; index < result; ++index) {
        neutralise_raw_mouse(record, record->header.dwSize);
        record = NEXTRAWINPUTBLOCK(record);
    }
    return result;
}

// Takes down every detour created so far, for both a failed install and a removal.
void unhook_cursor_targets(State& self)
{
    for (size_t index = 0; index < self.cursor.target_count; ++index) {
        MH_DisableHook(self.cursor.targets[index]);
        MH_RemoveHook(self.cursor.targets[index]);
    }
    self.cursor.target_count = 0;
}

bool install_cursor_hooks(State& self)
{
    HMODULE user32 = GetModuleHandleW(L"user32.dll");
    if (!user32) {
        rsf::say(log_to_rust, nullptr, "overlay input: user32 is not loaded, so the cursor stays the game's");
        return false;
    }
    const MH_STATUS initialised = MH_Initialize();
    if (initialised != MH_OK && initialised != MH_ERROR_ALREADY_INITIALIZED) {
        rsf::say(log_to_rust, nullptr, "overlay input: MinHook did not initialise (%d), so the cursor stays the game's",
            static_cast<int>(initialised));
        return false;
    }
    struct Hook {
        const char* name;
        void* detour;
        void** original;
    };
    const Hook hooks[] = {
        {"SetCursorPos", reinterpret_cast<void*>(&hooked_set_cursor_pos),
         reinterpret_cast<void**>(&self.cursor.set_cursor_pos)},
        {"ClipCursor", reinterpret_cast<void*>(&hooked_clip_cursor),
         reinterpret_cast<void**>(&self.cursor.clip_cursor)},
        {"GetCursorPos", reinterpret_cast<void*>(&hooked_get_cursor_pos),
         reinterpret_cast<void**>(&self.cursor.get_cursor_pos)},
        {"ShowCursor", reinterpret_cast<void*>(&hooked_show_cursor),
         reinterpret_cast<void**>(&self.cursor.show_cursor)},
        {"SetCursor", reinterpret_cast<void*>(&hooked_set_cursor),
         reinterpret_cast<void**>(&self.cursor.set_cursor)},
        {"GetRawInputData", reinterpret_cast<void*>(&hooked_get_raw_input_data),
         reinterpret_cast<void**>(&self.cursor.get_raw_input_data)},
        {"GetRawInputBuffer", reinterpret_cast<void*>(&hooked_get_raw_input_buffer),
         reinterpret_cast<void**>(&self.cursor.get_raw_input_buffer)},
    };
    self.cursor.target_count = 0;
    for (const Hook& hook : hooks) {
        void* target = reinterpret_cast<void*>(GetProcAddress(user32, hook.name));
        MH_STATUS status = target ? MH_CreateHook(target, hook.detour, hook.original)
                                  : MH_ERROR_FUNCTION_NOT_FOUND;
        if (status == MH_OK) {
            status = MH_EnableHook(target);
            if (status != MH_OK) {
                MH_RemoveHook(target);
            }
        }
        if (status != MH_OK) {
            rsf::say(log_to_rust, nullptr, "overlay input: %s could not be detoured (%d), so the cursor stays the game's",
                hook.name, static_cast<int>(status));
            unhook_cursor_targets(self);
            return false;
        }
        self.cursor.targets[self.cursor.target_count++] = target;
    }
    self.cursor.installed = true;
    rsf::say(log_to_rust, nullptr, "overlay input: cursor detours in place, %zu user32 functions", self.cursor.target_count);
    return true;
}

void remove_cursor_hooks(State& self)
{
    if (!self.cursor.installed) {
        return;
    }
    self.cursor.installed = false;
    unhook_cursor_targets(self);
}

// Take the cursor for the panel, or give it back. Always through the genuine functions.
void set_cursor_capture(State& self, bool on, bool restore_clip = true)
{
    if (!self.cursor.installed) {
        return;
    }
    if (on) {
        POINT point{};
        if (self.cursor.get_cursor_pos(&point)) {
            std::lock_guard<std::mutex> lock(self.cursor_guard);
            self.frozen = point;
            self.have_frozen = true;
        }
        self.cursor.clip_cursor(nullptr);
        int count = self.cursor.show_cursor(TRUE);
        int shows = 1;
        while (count < 0 && shows < 64) {
            count = self.cursor.show_cursor(TRUE);
            ++shows;
        }
        self.cursor_shows = shows;
        self.cursor.set_cursor(LoadCursorW(nullptr, MAKEINTRESOURCEW(32512)));
        self.swallowed_warps.store(0, std::memory_order_relaxed);
        self.swallowed_hides.store(0, std::memory_order_relaxed);
        rsf::say(log_to_rust, nullptr, "overlay input: cursor freed for the panel, shown after %d call%s", shows,
            shows == 1 ? "" : "s");
    } else {
        for (int index = 0; index < self.cursor_shows; ++index) {
            self.cursor.show_cursor(FALSE);
        }
        self.cursor_shows = 0;
        RECT clip{};
        bool restore = false;
        {
            std::lock_guard<std::mutex> lock(self.cursor_guard);
            self.have_frozen = false;
            restore = self.have_clip;
            clip = self.last_clip;
        }
        if (restore && restore_clip) {
            self.cursor.clip_cursor(&clip);
        }
        rsf::say(log_to_rust, nullptr, "overlay input: cursor returned to the game; swallowed %lu warps and %lu hides "
                  "while the panel was open",
            self.swallowed_warps.load(std::memory_order_relaxed),
            self.swallowed_hides.load(std::memory_order_relaxed));
    }
}

void apply_visibility_change(uint32_t visible, uint32_t route) noexcept try
{
    auto& self = state();
    rsf_core_overlay_pointer_clear();
    set_cursor_capture(self, visible != 0);
    if (visible) {
        seed_cursor_position(self);
    }
    // Announced because this is the one thing that happens on the message thread when the toggle
    // is pressed, and a crash on the first opened frame looks identical whether the window
    // procedure or the render thread died. The last line in the log says which.
    const char* source = route == RSF_VISIBILITY_WINDOW ? "window-message" :
        route == RSF_VISIBILITY_POLL ? "foreground-poll" : "external";
    rsf::say(log_to_rust, nullptr, "overlay input: toggle pressed, now %s (route=%s)",
        visible ? "visible" : "hidden", source);
} catch (...) { /* The Rust transition dispatcher may continue with queued changes. */ }

void apply_visibility(bool visible)
{
    rsf_core_overlay_visibility_set(visible ? 1u : 0u, GetTickCount64(), apply_visibility_change);
}

LRESULT chain_to_game(WNDPROC forward, HWND window, UINT message, WPARAM wparam, LPARAM lparam)
{
    if (forward) {
        return CallWindowProcW(forward, window, message, wparam, lparam);
    }
    // Install stores the original before swapping the procedure in, so this is not reachable
    // through the ordinary path. It stays because dropping a message the game expects is worse than
    // handing it to the default procedure, and calling through a null pointer ends the process with
    // nothing to read.
    return DefWindowProcW(window, message, wparam, lparam);
}

// The virtual key a mouse button message stands for, down, double-click and up alike.
uint32_t mouse_button_key(UINT message, WPARAM wparam)
{
    switch (message) {
    case WM_RBUTTONDOWN:
    case WM_RBUTTONDBLCLK:
    case WM_RBUTTONUP:
        return VK_RBUTTON;
    case WM_MBUTTONDOWN:
    case WM_MBUTTONDBLCLK:
    case WM_MBUTTONUP:
        return VK_MBUTTON;
    case WM_XBUTTONDOWN:
    case WM_XBUTTONDBLCLK:
    case WM_XBUTTONUP:
        return GET_XBUTTON_WPARAM(wparam) == XBUTTON2 ? VK_XBUTTON2 : VK_XBUTTON1;
    default:
        return VK_LBUTTON;
    }
}

LRESULT CALLBACK hooked_window_proc(HWND window, UINT message, WPARAM wparam, LPARAM lparam)
{
    State& self = state();
    // Acquire, to pair with the release store install makes before it swaps the procedure in. It
    // costs nothing on x64, and the alternative to reading a stale null here is chain_to_game
    // handing the game's own message to DefWindowProcW instead of to the game.
    const WNDPROC forward = reinterpret_cast<WNDPROC>(rsf_core_overlay_input_original());
    const bool visible = rsf_core_overlay_visible() != 0;
    const uint32_t toggle = rsf_core_overlay_toggle_key();

    // Windows can promote touch to mouse messages for a legacy window such as Unity's.
    // Their location must survive cursor polling, and a down/up between frames is still a tap.
    const auto extra = static_cast<ULONG_PTR>(GetMessageExtraInfo());
    if (visible && (extra & 0xffffff80u) == 0xff515780u &&
        (message == WM_MOUSEMOVE || message == WM_LBUTTONDOWN || message == WM_LBUTTONUP)) {
        self.pointer_received.fetch_add(1, std::memory_order_relaxed);
        self.pointer_consumed.fetch_add(1, std::memory_order_relaxed);
        POINT point{GET_X_LPARAM(lparam), GET_Y_LPARAM(lparam)};
        record_contact(window, point, 0, message == WM_LBUTTONDOWN, message == WM_LBUTTONUP);
        return 0;
    }

    switch (message) {
    case WM_POINTERDOWN:
    case WM_POINTERUPDATE:
    case WM_POINTERUP: {
        POINTER_INFO info{};
        const bool have_info = GetPointerInfo(GET_POINTERID_WPARAM(wparam), &info) != FALSE;
        const bool canceled = have_info && (info.pointerFlags & POINTER_FLAG_CANCELED) != 0;
        if (have_info && info.pointerType == PT_TOUCH) {
            const uint32_t flags = uint32_t(RSF_TOUCH_POINTER) |
                touch_flags(message == WM_POINTERDOWN, message == WM_POINTERUP) |
                (canceled ? uint32_t(RSF_TOUCH_CANCEL) : 0u);
            const auto desired = rsf_core_overlay_pointer_gesture(info.pointerId, info.ptPixelLocation.x,
                info.ptPixelLocation.y, flags, GetTickCount64(), rsf_core_overlay_visible());
            if (desired >= 0) apply_visibility(desired != 0);
        }
        if (canceled) rsf_core_overlay_pointer_capture_lost(info.pointerId);
        // Hidden: tracked above, and the game still receives every contact.
        if (!visible) break;
        self.pointer_received.fetch_add(1, std::memory_order_relaxed);
        if (have_info && (info.pointerType == PT_TOUCH || info.pointerType == PT_PEN)) {
            POINT point = info.ptPixelLocation;
            if (!canceled && ScreenToClient(window, &point))
                record_contact(window, point, info.pointerId, message == WM_POINTERDOWN,
                    message == WM_POINTERUP);
            self.pointer_consumed.fetch_add(1, std::memory_order_relaxed);
            return 0;
        }
        break;
    }
    case WM_POINTERCAPTURECHANGED:
        rsf_core_overlay_pointer_capture_lost(GET_POINTERID_WPARAM(wparam));
        if (visible) return 0;
        break;
    case WM_TOUCH: {
        TOUCHINPUT contacts[32]{};
        const UINT count = LOWORD(wparam);
        // Reading the contacts does not consume them: a hidden panel leaves the handle open for
        // the game, which closes it.
        if (count > 32 || !GetTouchInputInfo(reinterpret_cast<HTOUCHINPUT>(lparam), count, contacts, sizeof(TOUCHINPUT))) break;
        const auto now = GetTickCount64();
        int32_t desired = -1;
        for (UINT i = 0; i < count; ++i) {
            const POINT screen{TOUCH_COORD_TO_PIXEL(contacts[i].x), TOUCH_COORD_TO_PIXEL(contacts[i].y)};
            const auto flags = touch_flags((contacts[i].dwFlags & TOUCHEVENTF_DOWN) != 0,
                (contacts[i].dwFlags & TOUCHEVENTF_UP) != 0);
            const auto change = rsf_core_overlay_pointer_gesture(contacts[i].dwID, screen.x, screen.y,
                flags, now, rsf_core_overlay_visible());
            if (change >= 0) desired = change;
        }
        if (desired >= 0) apply_visibility(desired != 0);
        if (!visible) break;
        self.pointer_received.fetch_add(count, std::memory_order_relaxed);
        self.pointer_consumed.fetch_add(count, std::memory_order_relaxed);
        for (UINT i = 0; i < count; ++i) {
            POINT point{TOUCH_COORD_TO_PIXEL(contacts[i].x), TOUCH_COORD_TO_PIXEL(contacts[i].y)};
            if (ScreenToClient(window, &point))
                record_contact(window, point, contacts[i].dwID, (contacts[i].dwFlags & TOUCHEVENTF_DOWN) != 0,
                    (contacts[i].dwFlags & TOUCHEVENTF_UP) != 0);
        }
        CloseTouchInputHandle(reinterpret_cast<HTOUCHINPUT>(lparam));
        return 0;
    }
    case WM_KILLFOCUS:
        // Releases arrive at whatever took focus, not here, so anything still marked held would
        // stay held for the life of the process.
        rsf_core_overlay_input_clear_holds();
        rsf_core_overlay_pointer_focus_lost();
        break;

    case WM_ACTIVATEAPP:
        if (wparam == FALSE) {
            rsf_core_overlay_input_clear_holds();
            rsf_core_overlay_pointer_focus_lost();
        }
        break;

    case WM_KEYDOWN:
    case WM_SYSKEYDOWN: {
        const uint32_t key = static_cast<uint32_t>(wparam);
        if (key == toggle) {
            // Bit 30 is the previous key state. Holding the toggle down otherwise opens and closes
            // the overlay at the auto repeat rate.
            const uint32_t repeat = (lparam & 0x40000000) != 0 ? 1u : 0u;
            if (!rsf_core_overlay_toggle_press(repeat, GetTickCount64(), apply_visibility_change)) {
                return visible ? 0 : chain_to_game(forward, window, message, wparam, lparam);
            }
            if (visible) {
                return 0;
            }
            // Chained deliberately. While the overlay is hidden the game's input stream is
            // untouched, toggle included, so the game sees this press exactly as it would with
            // nothing installed. Marking it held means the release is forwarded too, even though
            // the overlay is open by then, and the game does not end up with a half pressed key.
            rsf_core_overlay_input_hold(key);
            return chain_to_game(forward, window, message, wparam, lparam);
        }
        if (visible) {
            // System keys other than the toggle keep going. They are window management rather than
            // gameplay, and swallowing WM_SYSKEYDOWN would take Alt+F4 with it: an overlay bug
            // should not be able to leave a user unable to close the game.
            if (message == WM_SYSKEYDOWN) {
                rsf_core_overlay_input_hold(key);
                return chain_to_game(forward, window, message, wparam, lparam);
            }
            self.keys_consumed.fetch_add(1, std::memory_order_relaxed);
            return 0;
        }
        rsf_core_overlay_input_hold(key);
        break;
    }

    case WM_KEYUP:
    case WM_SYSKEYUP: {
        const uint32_t key = static_cast<uint32_t>(wparam);
        if (key == toggle) rsf_core_overlay_toggle_release();
        const bool owed = rsf_core_overlay_input_release(key) != 0;
        // System keys chain in both directions, matching the WM_SYSKEYDOWN rule above. Chaining the
        // press and swallowing the release is the one combination that can leave the game holding
        // Alt, and it is reachable without the held table knowing: WM_KILLFOCUS forgets every hold,
        // so an Alt held across an alt-tab returns with no record of its press, and so does an Alt
        // held at the moment the runtime was injected.
        if (visible && !owed && message != WM_SYSKEYUP) {
            self.keys_consumed.fetch_add(1, std::memory_order_relaxed);
            return 0;
        }
        // The toggle does the same thing from the other direction: closing the overlay swallows the
        // toggle's press, and the matching release then arrives with the overlay hidden, where the
        // rule is to chain everything unchanged. A release of a key the game is not holding is a
        // no-op in an input handler that tracks key state, which is reasoning about the game rather
        // than something checked in one. The alternative is filtering the stream while the overlay
        // is hidden, which is the one thing this module promises not to do.
        break;
    }

    case WM_CHAR:
    case WM_DEADCHAR:
        // Consumed but not delivered: rsf_overlay_input has no text field. Letting them through
        // while the overlay has focus means typing into the overlay also types into the game.
        if (visible) {
            return 0;
        }
        break;

    case WM_MOUSEMOVE:
        if (visible) {
            self.pointer_received.fetch_add(1, std::memory_order_relaxed);
            self.pointer_consumed.fetch_add(1, std::memory_order_relaxed);
            record_mouse_position(self, window, lparam);
            return 0;
        }
        // Nothing is recorded while hidden, deliberately. Moves are the highest rate message the
        // window gets and the overlay seeds its position when it opens, so this path stays free of
        // both the lock and the work.
        break;

    case WM_LBUTTONDOWN:
    case WM_LBUTTONDBLCLK:
    case WM_RBUTTONDOWN:
    case WM_RBUTTONDBLCLK:
    case WM_MBUTTONDOWN:
    case WM_MBUTTONDBLCLK:
    case WM_XBUTTONDOWN:
    case WM_XBUTTONDBLCLK: {
        const uint32_t key = mouse_button_key(message, wparam);
        if (visible) {
            self.pointer_received.fetch_add(1, std::memory_order_relaxed);
            self.pointer_consumed.fetch_add(1, std::memory_order_relaxed);
            self.button_downs_consumed.fetch_add(1, std::memory_order_relaxed);
            // The position first: a click can arrive without a move before it, and the overlay
            // would otherwise apply it wherever the cursor was last seen.
            record_mouse_position(self, window, lparam);
            record_button(key, true);
            // The X button messages are documented as returning TRUE when handled.
            return (message == WM_XBUTTONDOWN || message == WM_XBUTTONDBLCLK) ? TRUE : 0;
        }
        rsf_core_overlay_input_hold(key);
        break;
    }

    case WM_LBUTTONUP:
    case WM_RBUTTONUP:
    case WM_MBUTTONUP:
    case WM_XBUTTONUP: {
        const uint32_t key = mouse_button_key(message, wparam);
        const bool owed = rsf_core_overlay_input_release(key) != 0;
        if (visible) {
            self.pointer_received.fetch_add(1, std::memory_order_relaxed);
            record_mouse_position(self, window, lparam);
            record_button(key, false);
            if (!owed) {
                self.pointer_consumed.fetch_add(1, std::memory_order_relaxed);
                return message == WM_XBUTTONUP ? TRUE : 0;
            }
        }
        break;
    }

    case WM_MOUSEWHEEL:
        if (visible) {
            // Notches, not pixels. WHEEL_DELTA is one detent and how far a detent scrolls is a
            // choice about the interface, which belongs on the egui side rather than here.
            // The position in this message is in screen coordinates, unlike every other mouse
            // message, so it is ignored rather than converted.
            const float notches = static_cast<float>(GET_WHEEL_DELTA_WPARAM(wparam)) / WHEEL_DELTA;
            rsf_core_overlay_pointer_scroll(notches);
            self.pointer_received.fetch_add(1, std::memory_order_relaxed);
            self.pointer_consumed.fetch_add(1, std::memory_order_relaxed);
            self.wheel_notches_consumed.fetch_add(1, std::memory_order_relaxed);
            return 0;
        }
        break;

    case WM_MOUSEHWHEEL:
        // Consumed with nowhere to put it: the contract has one scroll axis. Passing it through
        // while the overlay is open would scroll the game sideways under the menu.
        if (visible) {
            return 0;
        }
        break;

    case WM_INPUT:
        if (visible) {
            // Read before it is stopped. This is the movement the overlay's own pointer follows:
            // raw input is what the mouse reported, before the pointer was warped back to the
            // centre and before Windows coalesced anything, which is what makes it smooth where
            // following the window's own moves is chunky.
            record_raw_mouse(self, window, lparam);
            self.raw_mouse_reports.fetch_add(1, std::memory_order_relaxed);

            // Raw input is how a game reads mouse movement for aiming, so it has to stop here or
            // the overlay's clicks steer as well. The default procedure still runs: WM_INPUT is
            // documented as requiring it so the system can release the input data, and skipping
            // that leaks for as long as the overlay is open.
            //
            // This only covers raw input delivered as a message. A game reading its device through
            // DirectInput or XInput is not intercepted at all, and this cannot make it so.
            return DefWindowProcW(window, message, wparam, lparam);
        }
        break;

    default:
        break;
    }

    return chain_to_game(forward, window, message, wparam, lparam);
}

constexpr rsf_overlay_window_ops window_ops{
    []() noexcept {
        try { install_cursor_hooks(state()); } catch (...) {}
    },
    [](uint32_t visible, uint32_t window_alive) noexcept {
        auto& self = state();
        if (visible) {
            try { set_cursor_capture(self, false, window_alive != 0); } catch (...) {}
        }
        remove_cursor_hooks(self);
        std::lock_guard<std::mutex> cursor_lock(self.cursor_guard);
        self.have_clip = false;
    },
    [](rsf_overlay_input_log_fn log, void* user, const char* message) noexcept {
        try { log(user, message); } catch (...) {}
    },
};
static_assert(sizeof(rsf_overlay_window_ops) == 3 * sizeof(void*) && sizeof(rsf_overlay_input_options) == 32);
static_assert(RSF_OVERLAY_INPUT_ABI_VERSION == 1 && VK_INSERT == 0x2d &&
    RSF_OVERLAY_INPUT_OK == 0 && RSF_OVERLAY_INPUT_ERROR_INVALID_ARGUMENT == -1 &&
    RSF_OVERLAY_INPUT_ERROR_ABI_MISMATCH == -2 && RSF_OVERLAY_INPUT_ERROR_ALREADY_INSTALLED == -3 &&
    RSF_OVERLAY_INPUT_ERROR_NOT_INSTALLED == -4 && RSF_OVERLAY_INPUT_ERROR_SUBCLASS_FAILED == -5 &&
    RSF_OVERLAY_INPUT_ERROR_FOREIGN_SUBCLASS == -6);

} // namespace

extern "C" rsf_overlay_input_result
rsf_overlay_input_install(void* window, const rsf_overlay_input_options* options)
{
    return rsf_core_overlay_input_install(window, options,
        reinterpret_cast<intptr_t>(&hooked_window_proc), &window_ops);
}

extern "C" rsf_overlay_input_result rsf_overlay_input_uninstall(void)
{
    return rsf_core_overlay_input_uninstall(reinterpret_cast<intptr_t>(&hooked_window_proc), &window_ops);
}

extern "C" uint32_t rsf_overlay_input_visible(void)
{
    return rsf_core_overlay_visible();
}

extern "C" void rsf_overlay_input_poll_toggle(void)
{
    const HWND window = static_cast<HWND>(rsf_core_overlay_input_window());
    if (!window) return;
    const HWND foreground = GetForegroundWindow();
    const bool focused = foreground && GetAncestor(window, GA_ROOT) == GetAncestor(foreground, GA_ROOT);
    const bool down = (GetAsyncKeyState(int(rsf_core_overlay_toggle_key())) & 0x8000) != 0;
    rsf_core_overlay_toggle_poll(focused ? 1u : 0u, down ? 1u : 0u, GetTickCount64(), apply_visibility_change);
    uint32_t buttons = 0;
    if (focused && rsf_core_overlay_visible()) {
        for (const int key : {VK_LBUTTON, VK_RBUTTON, VK_MBUTTON})
            if (GetAsyncKeyState(key) & 0x8000) buttons |= overlay_button_bit(uint32_t(key));
    }
    rsf_core_overlay_pointer_polled_buttons(buttons);
}

extern "C" void rsf_overlay_input_set_visible(uint32_t visible)
{
    // Allowed before install so a caller can set the flag in whatever order suits it. It does not
    // make the overlay draw on its own: rsf_overlay_input_collect reports a hidden idle state until
    // a window is subclassed, so a renderer testing its own drawing installs first or fills
    // rsf_overlay_input itself.
    apply_visibility(visible != 0);
}

extern "C" rsf_overlay_input_result rsf_overlay_input_collect(rsf_overlay_input* out,
                                                              uint32_t display_width,
                                                              uint32_t display_height,
                                                              float delta_seconds)
{
    if (!out || out->struct_size < sizeof(rsf_overlay_input)) {
        return RSF_OVERLAY_INPUT_ERROR_INVALID_ARGUMENT;
    }

    // The caller owns struct_size, so it is read above and never written here.
    out->display_width = display_width;
    out->display_height = display_height;
    // A negative or NaN delta reaches egui's animation clock, so it is clamped rather than passed
    // on. The comparison is written this way because NaN fails it and lands on zero.
    out->delta_seconds = delta_seconds > 0.0f ? delta_seconds : 0.0f;
    out->startup_hint_alpha = 0.0f; // The presentation host owns the hint timer.

    State& self = state();
    const HWND window = static_cast<HWND>(rsf_core_overlay_input_window());
    if (!window) {
        out->mouse_x = 0.0f;
        out->mouse_y = 0.0f;
        out->mouse_buttons = 0;
        out->scroll_delta = 0.0f;
        out->visible = 0;
        return RSF_OVERLAY_INPUT_ERROR_NOT_INSTALLED;
    }

    out->visible = rsf_core_overlay_visible();
    // Sample outside the accumulator lock. Rust checks for a newer contact
    // before applying this absolute cursor position.
    rsf_overlay_client_point cursor{};
    const rsf_overlay_client_point* sample = nullptr;
    if (out->visible && self.cursor.installed) {
        POINT point{};
        if (self.cursor.get_cursor_pos(&point) && ScreenToClient(window, &point)) {
            cursor = client_point(window, point);
            if (cursor.width > 0 && cursor.height > 0) sample = &cursor;
        }
    }
    rsf_core_overlay_pointer_collect(out, sample);
    if (out->visible) {
        self.frames_collected_visible.fetch_add(1, std::memory_order_relaxed);
        if (out->mouse_buttons) self.frames_with_button.fetch_add(1, std::memory_order_relaxed);
        self.last_x.store(static_cast<uint32_t>(out->mouse_x > 0.0f ? out->mouse_x : 0.0f), std::memory_order_relaxed);
        self.last_y.store(static_cast<uint32_t>(out->mouse_y > 0.0f ? out->mouse_y : 0.0f), std::memory_order_relaxed);
    }
    return RSF_OVERLAY_INPUT_OK;
}

extern "C" rsf_overlay_input_result rsf_overlay_input_counters_get(rsf_overlay_input_counters* out)
{
    if (!out || out->struct_size < sizeof(*out)) return RSF_OVERLAY_INPUT_ERROR_INVALID_ARGUMENT;
    const State& self = state();
    out->installed = rsf_core_overlay_input_window() != nullptr ? 1u : 0u;
    out->visible = rsf_core_overlay_visible();
    out->cursor_freed = self.cursor.installed ? 1u : 0u;
    out->pointer_received = self.pointer_received.load(std::memory_order_relaxed);
    out->pointer_consumed = self.pointer_consumed.load(std::memory_order_relaxed);
    out->button_downs_consumed = self.button_downs_consumed.load(std::memory_order_relaxed);
    out->wheel_consumed = self.wheel_notches_consumed.load(std::memory_order_relaxed);
    out->raw_mouse_reports = self.raw_mouse_reports.load(std::memory_order_relaxed);
    out->keys_consumed = self.keys_consumed.load(std::memory_order_relaxed);
    out->frames_visible = self.frames_collected_visible.load(std::memory_order_relaxed);
    out->frames_with_button = self.frames_with_button.load(std::memory_order_relaxed);
    out->last_pointer_x = self.last_x.load(std::memory_order_relaxed);
    out->last_pointer_y = self.last_y.load(std::memory_order_relaxed);
    return RSF_OVERLAY_INPUT_OK;
}
