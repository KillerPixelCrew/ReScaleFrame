// SPDX-License-Identifier: MIT

#include <rescaleframe/overlay_input.h>
#include "overlay_cursor_capture.h"
#include "overlay_cursor_platform.h"
#include "overlay_input_route.h"
#include "overlay_pointer_platform.h"
#include "overlay_visibility_platform.h"
#include "overlay_window_platform.h"

#include <windows.h>

#include <windowsx.h>

#include <MinHook.h>

#include <rescaleframe/log.h>

#include <cstddef>
#include <initializer_list>

namespace {

constexpr size_t raw_mouse_bytes = offsetof(RAWINPUT, data) + sizeof(RAWMOUSE);
static_assert(sizeof(rsf_overlay_client_point) == 16 && RSF_OVERLAY_MOUSE_LEFT == 1);

void log_to_rust(void*, const char* message) noexcept
{
    rsf_core_overlay_input_log(message);
}

struct CursorCall {
    CursorCall() { rsf_core_overlay_cursor_enter(); }
    ~CursorCall() { rsf_core_overlay_cursor_leave(); }
};

template<class Fn, class... Args> auto cursor_call(uint32_t index, Fn entry, Args... args)
{
    const CursorCall lifetime;
    const auto original = reinterpret_cast<Fn>(rsf_core_overlay_cursor_forward(index));
    return (original ? original : entry)(args...);
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

void record_raw_mouse(HWND window, LPARAM lparam)
{
    // Use raw deltas only when the absolute cursor has not been freed.
    if (rsf_core_overlay_cursor_ready() != 0) return;
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

void record_mouse_position(HWND window, LPARAM lparam)
{
    const POINT point{GET_X_LPARAM(lparam), GET_Y_LPARAM(lparam)};
    const bool cursor_freed = rsf_core_overlay_cursor_ready() != 0;
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

void seed_cursor_position()
{
    HWND window = static_cast<HWND>(rsf_core_overlay_input_window());
    POINT point{};
    // Call the genuine GetCursorPos so the sample is not the position frozen for the game.
    const bool have_point = rsf_core_overlay_cursor_ready() != 0 ? cursor_call(RSF_CURSOR_GET_POS, &GetCursorPos, &point) != FALSE :
        GetCursorPos(&point) != FALSE;
    if (!window || !have_point || !ScreenToClient(window, &point)) return;
    const auto facts = client_point(window, point);
    rsf_core_overlay_pointer_seed(&facts);
}

// The detours. Each forwards to the genuine function through MinHook's trampoline unless the panel
// is open and the call is one the game makes to own the mouse.

BOOL WINAPI hooked_set_cursor_pos(int x, int y)
{
    if (rsf_core_overlay_cursor_swallow_warp()) return TRUE;
    return cursor_call(RSF_CURSOR_SET_POS, &SetCursorPos, x, y);
}

BOOL WINAPI hooked_clip_cursor(const RECT* rect)
{
    const auto captured = rsf_core_overlay_cursor_clip_request(rect);
    return cursor_call(RSF_CURSOR_CLIP, &ClipCursor, captured ? nullptr : rect);
}

BOOL WINAPI hooked_get_cursor_pos(LPPOINT point)
{
    if (rsf_core_overlay_cursor_frozen(point)) return TRUE;
    return cursor_call(RSF_CURSOR_GET_POS, &GetCursorPos, point);
}

int32_t show_cursor_native(int32_t show) noexcept try
{
    return cursor_call(RSF_CURSOR_SHOW, &ShowCursor, show);
} catch (...) { return 0; }

int WINAPI hooked_show_cursor(BOOL show)
{
    return rsf_core_overlay_cursor_show(show, show_cursor_native);
}

HCURSOR WINAPI hooked_set_cursor(HCURSOR handle)
{
    if (rsf_core_overlay_cursor_captured() && !handle) {
        // A null cursor is the other way a game hides it. The panel wants an arrow.
        return cursor_call(RSF_CURSOR_SET, &SetCursor, LoadCursorW(nullptr, MAKEINTRESOURCEW(32512)));
    }
    return cursor_call(RSF_CURSOR_SET, &SetCursor, handle);
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
    const UINT result = cursor_call(RSF_CURSOR_RAW_DATA, &GetRawInputData, handle, command, data, size, header_size);
    if (result != static_cast<UINT>(-1) && data && command == RID_INPUT &&
        rsf_core_overlay_cursor_captured()) {
        neutralise_raw_mouse(static_cast<RAWINPUT*>(data), result);
    }
    return result;
}

UINT WINAPI hooked_get_raw_input_buffer(PRAWINPUT data, PUINT size, UINT header_size)
{
    const UINT result = cursor_call(RSF_CURSOR_RAW_BUFFER, &GetRawInputBuffer, data, size, header_size);
    if (result == static_cast<UINT>(-1) || !data || result == 0 ||
        !rsf_core_overlay_cursor_captured()) {
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

constexpr rsf_overlay_cursor_ops cursor_ops{
    []() -> int32_t {
        const auto status = MH_Initialize();
        return status == MH_ERROR_ALREADY_INITIALIZED ? MH_OK : status;
    },
    [](void* target, void* detour, void** original) -> int32_t {
        return MH_CreateHook(target, detour, original);
    },
    [](void* target) -> int32_t {
        const auto status = MH_EnableHook(target);
        return status == MH_ERROR_ENABLED ? MH_OK : status;
    },
    [](void* target) -> int32_t {
        const auto status = MH_DisableHook(target);
        return status == MH_OK || status == MH_ERROR_DISABLED || status == MH_ERROR_NOT_CREATED;
    },
    [](void* target) -> int32_t {
        const auto status = MH_RemoveHook(target);
        return status == MH_OK || status == MH_ERROR_NOT_CREATED;
    },
};
static_assert(MH_OK == 0 && RSF_CURSOR_HOOK_COUNT == 7);
static_assert(sizeof(rsf_overlay_cursor_ops) == 5 * sizeof(void*));

void install_cursor_hooks()
{
    void* detours[RSF_CURSOR_HOOK_COUNT]{
        reinterpret_cast<void*>(&hooked_set_cursor_pos),
        reinterpret_cast<void*>(&hooked_clip_cursor),
        reinterpret_cast<void*>(&hooked_get_cursor_pos),
        reinterpret_cast<void*>(&hooked_show_cursor),
        reinterpret_cast<void*>(&hooked_set_cursor),
        reinterpret_cast<void*>(&hooked_get_raw_input_data),
        reinterpret_cast<void*>(&hooked_get_raw_input_buffer),
    };
    rsf_core_overlay_cursor_install(detours, &cursor_ops);
}

constexpr rsf_overlay_cursor_effects cursor_effects{
    [](POINT* point) noexcept -> int32_t {
        try { return cursor_call(RSF_CURSOR_GET_POS, &GetCursorPos, point); }
        catch (...) { return 0; }
    },
    [](const RECT* rect) noexcept {
        try { cursor_call(RSF_CURSOR_CLIP, &ClipCursor, rect); } catch (...) {}
    },
    show_cursor_native,
    []() noexcept {
        try { cursor_call(RSF_CURSOR_SET, &SetCursor, LoadCursorW(nullptr, MAKEINTRESOURCEW(32512))); }
        catch (...) {}
    },
};
static_assert(sizeof(POINT) == 8 && sizeof(RECT) == 16);
static_assert(sizeof(rsf_overlay_cursor_effects) == 4 * sizeof(void*));

void apply_visibility_change(uint32_t visible, uint32_t route) noexcept try
{
    rsf_core_overlay_pointer_clear();
    rsf_core_overlay_cursor_capture(visible, 1, &cursor_effects);
    if (visible) {
        seed_cursor_position();
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
    // Acquire, to pair with the release store install makes before it swaps the procedure in. It
    // costs nothing on x64, and the alternative to reading a stale null here is chain_to_game
    // handing the game's own message to DefWindowProcW instead of to the game.
    const WNDPROC forward = reinterpret_cast<WNDPROC>(rsf_core_overlay_input_original());
    const bool visible = rsf_core_overlay_visible() != 0;

    // Windows can promote touch to mouse messages for a legacy window such as Unity's.
    // Their location must survive cursor polling, and a down/up between frames is still a tap.
    const auto extra = static_cast<ULONG_PTR>(GetMessageExtraInfo());
    if (visible && (extra & 0xffffff80u) == 0xff515780u &&
        (message == WM_MOUSEMOVE || message == WM_LBUTTONDOWN || message == WM_LBUTTONUP)) {
        rsf_core_overlay_note_pointer(1, 1);
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
        const bool supported = have_info && (info.pointerType == PT_TOUCH || info.pointerType == PT_PEN);
        rsf_core_overlay_note_pointer(1, supported ? 1u : 0u);
        if (supported) {
            POINT point = info.ptPixelLocation;
            if (!canceled && ScreenToClient(window, &point))
                record_contact(window, point, info.pointerId, message == WM_POINTERDOWN,
                    message == WM_POINTERUP);
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
        rsf_core_overlay_note_pointer(count, count);
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
        rsf_core_overlay_route_focus_lost();
        break;

    case WM_ACTIVATEAPP:
        if (wparam == FALSE) {
            rsf_core_overlay_route_focus_lost();
        }
        break;

    case WM_KEYDOWN:
    case WM_SYSKEYDOWN:
    case WM_KEYUP:
    case WM_SYSKEYUP: {
        const uint32_t key = static_cast<uint32_t>(wparam);
        const uint32_t flags =
            ((message == WM_KEYDOWN || message == WM_SYSKEYDOWN) ? uint32_t(RSF_OVERLAY_KEY_DOWN) : 0u) |
            ((message == WM_SYSKEYDOWN || message == WM_SYSKEYUP) ? uint32_t(RSF_OVERLAY_KEY_SYSTEM) : 0u) |
            ((lparam & 0x40000000) != 0 ? uint32_t(RSF_OVERLAY_KEY_REPEAT) : 0u);
        if (rsf_core_overlay_route_key(key, flags, visible, apply_visibility_change)) return 0;
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
            rsf_core_overlay_note_pointer(1, 1);
            record_mouse_position(window, lparam);
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
        const bool consumed = rsf_core_overlay_route_button(key, 1, visible) != 0;
        if (visible) {
            // A click can arrive without a preceding move.
            record_mouse_position(window, lparam);
            record_button(key, true);
        }
        if (consumed) return (message == WM_XBUTTONDOWN || message == WM_XBUTTONDBLCLK) ? TRUE : 0;
        break;
    }

    case WM_LBUTTONUP:
    case WM_RBUTTONUP:
    case WM_MBUTTONUP:
    case WM_XBUTTONUP: {
        const uint32_t key = mouse_button_key(message, wparam);
        const bool consumed = rsf_core_overlay_route_button(key, 0, visible) != 0;
        if (visible) {
            record_mouse_position(window, lparam);
            record_button(key, false);
        }
        if (consumed) return message == WM_XBUTTONUP ? TRUE : 0;
        break;
    }

    case WM_MOUSEWHEEL:
        if (visible) {
            // Notches, not pixels. WHEEL_DELTA is one detent and how far a detent scrolls is a
            // choice about the interface, which belongs on the egui side rather than here.
            // The position in this message is in screen coordinates, unlike every other mouse
            // message, so it is ignored rather than converted.
            const float notches = static_cast<float>(GET_WHEEL_DELTA_WPARAM(wparam)) / WHEEL_DELTA;
            rsf_core_overlay_note_wheel(notches);
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
            record_raw_mouse(window, lparam);
            rsf_core_overlay_note_raw();

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
        try { install_cursor_hooks(); } catch (...) {}
    },
    [](uint32_t visible, uint32_t window_alive) noexcept -> int32_t {
        if (visible) rsf_core_overlay_cursor_capture(0, window_alive, &cursor_effects);
        const auto retired = rsf_core_overlay_cursor_retire(&cursor_ops);
        rsf_core_overlay_cursor_reset_capture();
        return retired;
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
    if (out->visible && rsf_core_overlay_cursor_ready() != 0) {
        POINT point{};
        if (cursor_call(RSF_CURSOR_GET_POS, &GetCursorPos, &point) && ScreenToClient(window, &point)) {
            cursor = client_point(window, point);
            if (cursor.width > 0 && cursor.height > 0) sample = &cursor;
        }
    }
    rsf_core_overlay_pointer_collect(out, sample);
    return RSF_OVERLAY_INPUT_OK;
}

extern "C" rsf_overlay_input_result rsf_overlay_input_counters_get(rsf_overlay_input_counters* out)
{
    return rsf_core_overlay_input_counters(out);
}
