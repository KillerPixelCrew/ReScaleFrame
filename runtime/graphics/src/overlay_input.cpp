// SPDX-License-Identifier: GPL-3.0-only

#include <rescaleframe/overlay_input.h>

#include <windows.h>

#include <windowsx.h>

#include <MinHook.h>

#include <rescaleframe/log.h>

#include <algorithm>
#include <atomic>
#include <cmath>
#include <mutex>

namespace {

// One slot per virtual key code. A VK_ value is a byte, so every key and mouse button indexes the
// held table directly and the game's ordinary input path needs no lookup.
constexpr uint32_t held_table_size = 256;

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
    // Written once during install, read from the window procedure with no lock. A message can
    // arrive on the game's thread the instant the subclass takes effect, so these are in place
    // before the procedure is swapped in and are not cleared by uninstall.
    std::atomic<bool> installed{false};
    std::atomic<uint32_t> visible{0};
    /* When the visibility last actually changed, so two routes reporting one press do not undo
       each other. See apply_visibility. */
    std::atomic<unsigned long long> last_visibility_change{0};
    std::atomic<HWND> window{nullptr};
    std::atomic<WNDPROC> original{nullptr};
    std::atomic<uint32_t> toggle_key{VK_INSERT};

    // Serialises install against uninstall. The window procedure never takes this one, so the
    // SetWindowLongPtrW calls made under it cannot end up waiting on a message thread that is
    // waiting on us.
    std::mutex lifecycle;

    // The input the window procedure records and the render thread reads. Everything under this
    // lock is a handful of scalar stores, and nothing is called while holding it: this runs inside
    // the game's own message dispatch, so anything that could send a message or re-enter the
    // procedure would be re-entering the lock with it.
    std::mutex guard;
    /* The pointer, in the pixels of the image being presented rather than of the window's client
       area. The two differ under a DPI scale the process did not opt into: Windows then reports
       the client area and window messages in logical pixels while the back buffer is the physical
       size, 1067x600 against 1600x900 at 150 percent, and a pointer kept in client pixels could
       reach only two thirds of the panel. Raw mouse movement is in physical pixels already;
       window messages are scaled by the ratio on the way in. The display size is what the last
       collect passed in, zero until then, in which case client pixels are all there is. */
    float mouse_x = 0.0f;
    float mouse_y = 0.0f;
    float display_width = 0.0f;
    float display_height = 0.0f;

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
    /* The previous report's raw position, and whether there has been one. The overlay's pointer
       moves by the distance between reports rather than to the position in them, because a game
       that warps the pointer makes the position meaningless. See record_mouse_position. */
    float last_report_x = 0.0f;
    float last_report_y = 0.0f;
    bool have_last_report = false;
    /* Set once a raw mouse movement has been seen. Raw input is not affected by the game warping
       the pointer and is not coalesced, so once it arrives it drives the overlay's pointer and the
       window messages are ignored. See record_raw_mouse. */
    std::atomic<bool> have_raw_input{false};
    uint32_t buttons = 0;
    float scroll = 0.0f;

    // Keys and buttons the game has seen pressed and not yet released, indexed by virtual key.
    // Atomic rather than under the lock so that the hidden path, which is the game's ordinary input
    // path, takes no lock at all.
    std::atomic<uint8_t> game_holds[held_table_size] = {};

    // Only install and uninstall log, and both hold `lifecycle`. Nothing logs from the window
    // procedure: a line per message would put file I/O on the game's input path.
    rsf_overlay_input_log_fn log = nullptr;
    void* log_user = nullptr;
};

State& state()
{
    static State instance;
    return instance;
}

bool game_holds_key(const State& self, uint32_t key)
{
    return key < held_table_size && self.game_holds[key].load(std::memory_order_relaxed) != 0;
}

void set_game_hold(State& self, uint32_t key, bool held)
{
    if (key < held_table_size) {
        self.game_holds[key].store(held ? 1u : 0u, std::memory_order_relaxed);
    }
}

void forget_game_holds(State& self)
{
    for (uint32_t index = 0; index < held_table_size; ++index) {
        self.game_holds[index].store(0u, std::memory_order_relaxed);
    }
}

// Buttons and wheel are dropped on a visibility change and on focus loss. A button the overlay
// believes is down while nobody is pressing it drags a slider on its own.
void clear_transient_input(State& self)
{
    std::lock_guard<std::mutex> lock(self.guard);
    self.buttons = 0;
    self.scroll = 0.0f;
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

/* Move the pointer by a raw mouse movement.

   This is the good path, and the one an overlay wants in a game that locks the mouse. A game that
   warps the pointer every frame is steering from raw input, so `WM_INPUT` is already being
   delivered to this window and arrives here through the subclass without registering anything of
   our own. Registering would be worse than useless: raw input registration is per process and per
   device class, so ours would replace the game's and take its steering with it.

   Raw movement is what the mouse reported, before the pointer was clipped, warped or coalesced.
   Window moves are coalesced, which is why following them looked chunky: with the game warping
   every frame, the real movement and the warp back often arrive as one message and cancel.

   The caller stops the message after this, which it already did before the overlay read anything
   from it: while the panel is open, raw movement must not also steer the aircraft. */
void record_raw_mouse(State& self, HWND window, LPARAM lparam)
{
    // With the cursor freed by the detours, the panel reads the real pointer each frame and raw
    // deltas would count every movement twice. This path is the fallback for when they are not.
    if (self.cursor.installed) {
        return;
    }
    // One read into a RAWINPUT-sized record: a mouse report always fits, and a larger one is refused.
    RAWINPUT raw{};
    UINT size = sizeof(raw);
    const UINT read = GetRawInputData(reinterpret_cast<HRAWINPUT>(lparam), RID_INPUT, &raw, &size,
                                      sizeof(RAWINPUTHEADER));
    if (read == static_cast<UINT>(-1) || read < sizeof(RAWINPUTHEADER) ||
        raw.header.dwType != RIM_TYPEMOUSE) {
        return;
    }
    // Absolute devices, a tablet or some remote desktops, report a position rather than a movement
    // and are left to the window messages, which are correct for them.
    if ((raw.data.mouse.usFlags & MOUSE_MOVE_ABSOLUTE) != 0) {
        return;
    }
    const long dx = raw.data.mouse.lLastX;
    const long dy = raw.data.mouse.lLastY;
    if (dx == 0 && dy == 0) {
        return;
    }

    RECT client{};
    const bool have_client = GetClientRect(window, &client) && client.right > 0 && client.bottom > 0;
    const float width = have_client ? static_cast<float>(client.right) : 0.0f;
    const float height = have_client ? static_cast<float>(client.bottom) : 0.0f;

    std::lock_guard<std::mutex> lock(self.guard);
    self.have_raw_input.store(true, std::memory_order_release);
    self.mouse_x += static_cast<float>(dx);
    self.mouse_y += static_cast<float>(dy);
    // Raw movement is physical, so the bound is the presented image where that is known.
    const float bound_x = self.display_width > 0.0f ? self.display_width : width;
    const float bound_y = self.display_height > 0.0f ? self.display_height : height;
    if (bound_x > 0.0f && bound_y > 0.0f) {
        self.mouse_x = std::min(std::max(self.mouse_x, 0.0f), bound_x - 1.0f);
        self.mouse_y = std::min(std::max(self.mouse_y, 0.0f), bound_y - 1.0f);
    }
}

/* Follow the pointer by how far it moved, not by where it is.

   Ace Combat 7 is played with a pad and steers with a locked mouse: it warps the pointer back to
   the middle of the window every frame, so its absolute position is the centre no matter how the
   mouse is moved, and a panel that trusted that position had a cursor pinned there.

   What survives the warp is the distance between two reports, so the overlay keeps a pointer of its
   own and moves it by that distance. The warp itself is the one report that must not count: it is
   the game putting the pointer back, not the user moving it, and adding it would cancel the motion
   that preceded it exactly. It is recognised by landing on the centre, which is where a warp goes
   and where a hand almost never lands on the exact pixel. Losing one report when it does costs a
   pixel of travel.

   A game that does not warp never produces that report and the same arithmetic follows the pointer
   normally, so this costs nothing where it is not needed. */
void record_mouse_position(State& self, HWND window, LPARAM lparam)
{
    // Client pixels, which is the coordinate space the overlay works in. See the assumption note in
    // rsf_overlay_input_collect: this module does not scale them.
    const float x = static_cast<float>(GET_X_LPARAM(lparam));
    const float y = static_cast<float>(GET_Y_LPARAM(lparam));

    // The freed cursor is polled each frame instead. See the detours.
    if (self.cursor.installed) {
        return;
    }
    // Raw input has taken over, and following both would count every movement twice.
    if (self.have_raw_input.load(std::memory_order_acquire)) {
        return;
    }

    RECT client{};
    if (!GetClientRect(window, &client) || client.right <= 0 || client.bottom <= 0) {
        std::lock_guard<std::mutex> lock(self.guard);
        self.mouse_x = x;
        self.mouse_y = y;
        return;
    }
    const float width = static_cast<float>(client.right);
    const float height = static_cast<float>(client.bottom);
    const float centre_x = std::floor(width * 0.5f);
    const float centre_y = std::floor(height * 0.5f);

    std::lock_guard<std::mutex> lock(self.guard);
    // Client pixels to presented pixels. One where the two agree, which they do in a DPI-aware
    // process and in exclusive fullscreen.
    const float scale_x = self.display_width > 0.0f ? self.display_width / width : 1.0f;
    const float scale_y = self.display_height > 0.0f ? self.display_height / height : 1.0f;
    const float bound_x = self.display_width > 0.0f ? self.display_width : width;
    const float bound_y = self.display_height > 0.0f ? self.display_height : height;
    const bool is_warp = x == centre_x && y == centre_y;
    if (!self.have_last_report) {
        self.have_last_report = true;
    } else if (!is_warp) {
        self.mouse_x += (x - self.last_report_x) * scale_x;
        self.mouse_y += (y - self.last_report_y) * scale_y;
        // Kept inside the window, or the pointer wanders off and takes several sweeps to come back.
        self.mouse_x = std::min(std::max(self.mouse_x, 0.0f), bound_x - 1.0f);
        self.mouse_y = std::min(std::max(self.mouse_y, 0.0f), bound_y - 1.0f);
    }
    self.last_report_x = x;
    self.last_report_y = y;
}

void record_button(State& self, uint32_t virtual_key, bool down)
{
    const uint32_t bit = overlay_button_bit(virtual_key);
    if (bit == 0) {
        return;
    }
    std::lock_guard<std::mutex> lock(self.guard);
    if (down) {
        self.buttons |= bit;
    } else {
        self.buttons &= ~bit;
    }
}

// Where the cursor is when the overlay opens, so the first frame does not place it at the origin
// and wait for a move. Win32 calls only, no lock held.
void seed_cursor_position(State& self)
{
    HWND window = self.window.load(std::memory_order_relaxed);
    POINT point{};
    // The genuine GetCursorPos where it is detoured: the detour would answer with the frozen
    // position, which is exactly the one the panel must not start from.
    const bool have_point = self.cursor.installed ? self.cursor.get_cursor_pos(&point) != FALSE
                                                  : GetCursorPos(&point) != FALSE;
    if (!window || !have_point || !ScreenToClient(window, &point)) {
        return;
    }
    // Whatever the game left the cursor at. A game that hid and centred it reports the centre,
    // which is as good an answer as exists before the first move. Scaled from client pixels to
    // presented pixels like a window message.
    RECT client{};
    const bool have_client = GetClientRect(window, &client) && client.right > 0 && client.bottom > 0;
    std::lock_guard<std::mutex> lock(self.guard);
    const float scale_x = have_client && self.display_width > 0.0f
                              ? self.display_width / static_cast<float>(client.right)
                              : 1.0f;
    const float scale_y = have_client && self.display_height > 0.0f
                              ? self.display_height / static_cast<float>(client.bottom)
                              : 1.0f;
    self.mouse_x = static_cast<float>(point.x) * scale_x;
    self.mouse_y = static_cast<float>(point.y) * scale_y;
    /* Forget the previous report, so the first move after opening is measured from the report that
       follows rather than from wherever the pointer was when the panel was last closed. */
    self.have_last_report = false;
}

// The detours. Each forwards to the genuine function through MinHook's trampoline unless the panel
// is open and the call is one the game makes to own the mouse.

BOOL WINAPI hooked_set_cursor_pos(int x, int y)
{
    State& self = state();
    if (self.visible.load(std::memory_order_acquire)) {
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
    if (self.visible.load(std::memory_order_acquire)) {
        // Some games clip to a single pixel to keep the pointer still. Lifted for the panel; what
        // the game asked for is remembered and put back on close.
        return self.cursor.clip_cursor(nullptr);
    }
    return self.cursor.clip_cursor(rect);
}

BOOL WINAPI hooked_get_cursor_pos(LPPOINT point)
{
    State& self = state();
    if (point && self.visible.load(std::memory_order_acquire)) {
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
    if (!show && self.visible.load(std::memory_order_acquire)) {
        // The game hiding the cursor, often every frame. Not applied; the count it would have seen
        // is reported so its own bookkeeping stays consistent.
        self.swallowed_hides.fetch_add(1, std::memory_order_relaxed);
        const int count = self.cursor.show_cursor(TRUE);
        self.cursor.show_cursor(FALSE);
        return count - 1;
    }
    return self.cursor.show_cursor(show);
}

HCURSOR WINAPI hooked_set_cursor(HCURSOR handle)
{
    State& self = state();
    if (self.visible.load(std::memory_order_acquire) && !handle) {
        // A null cursor is the other way a game hides it. The panel wants an arrow.
        return self.cursor.set_cursor(LoadCursorW(nullptr, MAKEINTRESOURCEW(32512)));
    }
    return self.cursor.set_cursor(handle);
}

// Zero the mouse in one raw input record, so a game steering from it holds still under the panel.
// Buttons too: a click on the panel must not fire the cannon.
void neutralise_raw_mouse(RAWINPUT* raw, size_t bytes)
{
    if (bytes >= sizeof(RAWINPUTHEADER) + sizeof(RAWMOUSE) && raw->header.dwType == RIM_TYPEMOUSE) {
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
        self.visible.load(std::memory_order_acquire)) {
        neutralise_raw_mouse(static_cast<RAWINPUT*>(data), result);
    }
    return result;
}

UINT WINAPI hooked_get_raw_input_buffer(PRAWINPUT data, PUINT size, UINT header_size)
{
    State& self = state();
    const UINT result = self.cursor.get_raw_input_buffer(data, size, header_size);
    if (result == static_cast<UINT>(-1) || !data || result == 0 ||
        !self.visible.load(std::memory_order_acquire)) {
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
        rsf::say(self.log, self.log_user, "overlay input: user32 is not loaded, so the cursor stays the game's");
        return false;
    }
    const MH_STATUS initialised = MH_Initialize();
    if (initialised != MH_OK && initialised != MH_ERROR_ALREADY_INITIALIZED) {
        rsf::say(self.log, self.log_user, "overlay input: MinHook did not initialise (%d), so the cursor stays the game's",
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
            rsf::say(self.log, self.log_user, "overlay input: %s could not be detoured (%d), so the cursor stays the game's",
                hook.name, static_cast<int>(status));
            unhook_cursor_targets(self);
            return false;
        }
        self.cursor.targets[self.cursor.target_count++] = target;
    }
    self.cursor.installed = true;
    rsf::say(self.log, self.log_user, "overlay input: cursor detours in place, %zu user32 functions", self.cursor.target_count);
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
void set_cursor_capture(State& self, bool on)
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
        rsf::say(self.log, self.log_user, "overlay input: cursor freed for the panel, shown after %d call%s", shows,
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
        if (restore) {
            self.cursor.clip_cursor(&clip);
        }
        rsf::say(self.log, self.log_user, "overlay input: cursor returned to the game; swallowed %lu warps and %lu hides "
                  "while the panel was open",
            self.swallowed_warps.load(std::memory_order_relaxed),
            self.swallowed_hides.load(std::memory_order_relaxed));
    }
}

void apply_visibility(State& self, bool visible)
{
    /* One press, one change.

       The toggle key reaches this from two directions: the window procedure, which sees the key as
       a message and can swallow it, and a hotkey worker polling the key state, which exists because
       a run turned up where no message ever arrived. Both fire on the same press. The procedure
       opens the panel and the poll, asking for the opposite of what it now sees, closes it again,
       which looks exactly like the panel flashing for one frame and vanishing.

       Ignoring a change that lands within a few frames of the last one costs nothing a hand can
       notice and makes either path work alone or together. */
    const ULONGLONG now = GetTickCount64();
    const ULONGLONG previous_change = self.last_visibility_change.load(std::memory_order_acquire);
    if (previous_change != 0 && now - previous_change < 250) {
        return;
    }

    const uint32_t previous = self.visible.exchange(visible ? 1u : 0u, std::memory_order_acq_rel);
    if ((previous != 0) == visible) {
        return;
    }
    self.last_visibility_change.store(now, std::memory_order_release);
    clear_transient_input(self);
    set_cursor_capture(self, visible);
    if (visible) {
        seed_cursor_position(self);
    }
    // Announced because this is the one thing that happens on the message thread when the toggle
    // is pressed, and a crash on the first opened frame looks identical whether the window
    // procedure or the render thread died. The last line in the log says which.
    rsf::say(self.log, self.log_user, visible ? "overlay input: toggle pressed, now visible"
                      : "overlay input: toggle pressed, now hidden");
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

// A release of something the game already saw pressed is forwarded even while the overlay is
// visible. Without this, holding the throttle key, opening the overlay and letting go leaves the
// game holding it forever, and the aircraft flies away under the menu.
bool release_is_owed_to_game(State& self, uint32_t virtual_key)
{
    if (!game_holds_key(self, virtual_key)) {
        return false;
    }
    set_game_hold(self, virtual_key, false);
    return true;
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
    const WNDPROC forward = self.original.load(std::memory_order_acquire);
    const bool visible = self.visible.load(std::memory_order_acquire) != 0;
    const uint32_t toggle = self.toggle_key.load(std::memory_order_relaxed);

    switch (message) {
    case WM_KILLFOCUS:
        // Releases arrive at whatever took focus, not here, so anything still marked held would
        // stay held for the life of the process.
        forget_game_holds(self);
        clear_transient_input(self);
        break;

    case WM_ACTIVATEAPP:
        if (wparam == FALSE) {
            forget_game_holds(self);
            clear_transient_input(self);
        }
        break;

    case WM_KEYDOWN:
    case WM_SYSKEYDOWN: {
        const uint32_t key = static_cast<uint32_t>(wparam);
        if (key == toggle) {
            // Bit 30 is the previous key state. Holding the toggle down otherwise opens and closes
            // the overlay at the auto repeat rate.
            const bool repeat = (lparam & 0x40000000) != 0;
            if (repeat) {
                return visible ? 0 : chain_to_game(forward, window, message, wparam, lparam);
            }
            if (visible) {
                apply_visibility(self, false);
                return 0;
            }
            apply_visibility(self, true);
            // Chained deliberately. While the overlay is hidden the game's input stream is
            // untouched, toggle included, so the game sees this press exactly as it would with
            // nothing installed. Marking it held means the release is forwarded too, even though
            // the overlay is open by then, and the game does not end up with a half pressed key.
            set_game_hold(self, key, true);
            return chain_to_game(forward, window, message, wparam, lparam);
        }
        if (visible) {
            // System keys other than the toggle keep going. They are window management rather than
            // gameplay, and swallowing WM_SYSKEYDOWN would take Alt+F4 with it: an overlay bug
            // should not be able to leave a user unable to close the game.
            if (message == WM_SYSKEYDOWN) {
                set_game_hold(self, key, true);
                return chain_to_game(forward, window, message, wparam, lparam);
            }
            return 0;
        }
        set_game_hold(self, key, true);
        break;
    }

    case WM_KEYUP:
    case WM_SYSKEYUP: {
        const uint32_t key = static_cast<uint32_t>(wparam);
        const bool owed = release_is_owed_to_game(self, key);
        // System keys chain in both directions, matching the WM_SYSKEYDOWN rule above. Chaining the
        // press and swallowing the release is the one combination that can leave the game holding
        // Alt, and it is reachable without the held table knowing: WM_KILLFOCUS forgets every hold,
        // so an Alt held across an alt-tab returns with no record of its press, and so does an Alt
        // held at the moment the runtime was injected.
        if (visible && !owed && message != WM_SYSKEYUP) {
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
            // The position first: a click can arrive without a move before it, and the overlay
            // would otherwise apply it wherever the cursor was last seen.
            record_mouse_position(self, window, lparam);
            record_button(self, key, true);
            // The X button messages are documented as returning TRUE when handled.
            return (message == WM_XBUTTONDOWN || message == WM_XBUTTONDBLCLK) ? TRUE : 0;
        }
        set_game_hold(self, key, true);
        break;
    }

    case WM_LBUTTONUP:
    case WM_RBUTTONUP:
    case WM_MBUTTONUP:
    case WM_XBUTTONUP: {
        const uint32_t key = mouse_button_key(message, wparam);
        const bool owed = release_is_owed_to_game(self, key);
        if (visible) {
            record_mouse_position(self, window, lparam);
            record_button(self, key, false);
            if (!owed) {
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
            {
                const float notches =
                    static_cast<float>(GET_WHEEL_DELTA_WPARAM(wparam)) / WHEEL_DELTA;
                std::lock_guard<std::mutex> lock(self.guard);
                self.scroll += notches;
            }
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

} // namespace

extern "C" rsf_overlay_input_result
rsf_overlay_input_install(void* window, const rsf_overlay_input_options* options)
{
    if (!window || !options || options->struct_size < sizeof(rsf_overlay_input_options)) {
        return RSF_OVERLAY_INPUT_ERROR_INVALID_ARGUMENT;
    }
    if (options->abi_version != RSF_OVERLAY_INPUT_ABI_VERSION) {
        return RSF_OVERLAY_INPUT_ERROR_ABI_MISMATCH;
    }

    HWND target = static_cast<HWND>(window);
    if (!IsWindow(target)) {
        return RSF_OVERLAY_INPUT_ERROR_INVALID_ARGUMENT;
    }

    State& self = state();
    std::lock_guard<std::mutex> lock(self.lifecycle);
    if (self.installed.load(std::memory_order_acquire)) {
        return RSF_OVERLAY_INPUT_ERROR_ALREADY_INSTALLED;
    }

    self.log = options->log;
    self.log_user = options->log_user;

    const uint32_t toggle = options->toggle_virtual_key ? options->toggle_virtual_key : VK_INSERT;
    self.toggle_key.store(toggle, std::memory_order_relaxed);
    self.window.store(target, std::memory_order_relaxed);
    self.visible.store(0, std::memory_order_relaxed);
    forget_game_holds(self);
    {
        std::lock_guard<std::mutex> input_lock(self.guard);
        self.mouse_x = 0.0f;
        self.mouse_y = 0.0f;
        self.buttons = 0;
        self.scroll = 0.0f;
    }

    if (!IsWindowUnicode(target)) {
        // Subclassing with the wide entry point converts an ANSI window to Unicode, which changes
        // how the game's own procedure receives WM_CHAR. Whether any game this project targets
        // registers an ANSI window has not been checked, and this path has never been exercised, so
        // this line is the only warning anyone gets. The fix, if such a game turns up, is the A
        // variants of the read, the swap and the forward.
        rsf::say(self.log, self.log_user, "overlay input: window is ANSI, subclassing it with the wide entry point "
                  "converts it to Unicode");
    }

    // The original goes in before the swap, not after. A message can reach the new procedure on the
    // game's thread the instant SetWindowLongPtrW returns, and the forward pointer has to already
    // be there when it does.
    const LONG_PTR existing = GetWindowLongPtrW(target, GWLP_WNDPROC);
    if (existing == 0) {
        rsf::say(self.log, self.log_user, "overlay input: could not read the window procedure, error %lu",
            static_cast<unsigned long>(GetLastError()));
        return RSF_OVERLAY_INPUT_ERROR_SUBCLASS_FAILED;
    }
    self.original.store(reinterpret_cast<WNDPROC>(existing), std::memory_order_release);
    self.installed.store(true, std::memory_order_release);

    rsf::say(self.log, self.log_user, "overlay input: subclassing window %p, toggle key 0x%02x", static_cast<void*>(target),
        static_cast<unsigned>(toggle));

    SetLastError(0);
    const LONG_PTR previous =
        SetWindowLongPtrW(target, GWLP_WNDPROC, reinterpret_cast<LONG_PTR>(&hooked_window_proc));
    // Read once, before anything else runs. A displaced procedure of zero is only a failure if the
    // last error says so, and the rollback below sits between the test and the report.
    const DWORD swap_error = GetLastError();
    if (previous == 0 && swap_error != 0) {
        self.installed.store(false, std::memory_order_release);
        self.original.store(nullptr, std::memory_order_release);
        self.window.store(nullptr, std::memory_order_relaxed);
        rsf::say(self.log, self.log_user, "overlay input: subclass failed, error %lu",
            static_cast<unsigned long>(swap_error));
        return RSF_OVERLAY_INPUT_ERROR_SUBCLASS_FAILED;
    }
    if (previous != existing) {
        // Something replaced the procedure between the read and the swap. The value the swap
        // returned is the one that was actually displaced, so that is the chain to forward to.
        self.original.store(reinterpret_cast<WNDPROC>(previous), std::memory_order_release);
        rsf::say(self.log, self.log_user, "overlay input: window procedure changed during install, forwarding to the "
                  "displaced one");
    }
    // The cursor detours are not a reason to fail: without them the message path still works, as
    // far as it ever did, and the log has said which it is.
    install_cursor_hooks(self);
    return RSF_OVERLAY_INPUT_OK;
}

extern "C" rsf_overlay_input_result rsf_overlay_input_uninstall(void)
{
    State& self = state();
    std::lock_guard<std::mutex> lock(self.lifecycle);
    if (!self.installed.load(std::memory_order_acquire)) {
        return RSF_OVERLAY_INPUT_ERROR_NOT_INSTALLED;
    }

    HWND target = self.window.load(std::memory_order_relaxed);
    if (!target || !IsWindow(target)) {
        // The game closed its window while we were installed. There is nothing to restore, and
        // touching a dead handle would be worse than leaving it.
        rsf::say(self.log, self.log_user, "overlay input: window is gone, dropping the subclass without restoring");
        self.installed.store(false, std::memory_order_release);
        self.visible.store(0, std::memory_order_release);
        self.window.store(nullptr, std::memory_order_relaxed);
        return RSF_OVERLAY_INPUT_OK;
    }

    const LONG_PTR current = GetWindowLongPtrW(target, GWLP_WNDPROC);
    if (current != reinterpret_cast<LONG_PTR>(&hooked_window_proc)) {
        // Somebody subclassed after us and their procedure now holds ours as its forward. Writing
        // our saved original back would erase their hook, which is a fault in their code that they
        // cannot see and we caused. Staying installed is the lesser damage.
        rsf::say(self.log, self.log_user, "overlay input: uninstall refused, the window procedure is not ours");
        return RSF_OVERLAY_INPUT_ERROR_FOREIGN_SUBCLASS;
    }

    // Restore first, then stop consuming. In between, a message already inside the procedure still
    // forwards correctly: `original` is deliberately left in place, exactly as the vtable hooks in
    // this directory keep theirs, because clearing it turns that race from a stale hook into a call
    // through a null pointer. This cannot make an in flight message finish first.
    //
    // One thing it does not put back: if the window was ANSI, install converted it to Unicode and
    // restoring through the wide entry point leaves it converted for the rest of the process. Only
    // the A variants named in the install log would undo that, and they are untested.
    SetWindowLongPtrW(target, GWLP_WNDPROC, reinterpret_cast<LONG_PTR>(
                                                self.original.load(std::memory_order_acquire)));
    if (self.visible.load(std::memory_order_acquire)) {
        set_cursor_capture(self, false);
    }
    remove_cursor_hooks(self);
    self.installed.store(false, std::memory_order_release);
    self.visible.store(0, std::memory_order_release);
    self.window.store(nullptr, std::memory_order_relaxed);
    forget_game_holds(self);
    clear_transient_input(self);
    rsf::say(self.log, self.log_user, "overlay input: window procedure restored");
    return RSF_OVERLAY_INPUT_OK;
}

extern "C" uint32_t rsf_overlay_input_visible(void)
{
    return state().visible.load(std::memory_order_acquire);
}

extern "C" void rsf_overlay_input_set_visible(uint32_t visible)
{
    // Allowed before install so a caller can set the flag in whatever order suits it. It does not
    // make the overlay draw on its own: rsf_overlay_input_collect reports a hidden idle state until
    // a window is subclassed, so a renderer testing its own drawing installs first or fills
    // rsf_overlay_input itself.
    apply_visibility(state(), visible != 0);
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
    if (!self.installed.load(std::memory_order_acquire)) {
        out->mouse_x = 0.0f;
        out->mouse_y = 0.0f;
        out->mouse_buttons = 0;
        out->scroll_delta = 0.0f;
        out->visible = 0;
        return RSF_OVERLAY_INPUT_ERROR_NOT_INSTALLED;
    }

    out->visible = self.visible.load(std::memory_order_acquire);
    {
        // The pointer is kept in the pixels of the presented image, which is why the display size
        // comes from the caller: this module has no view of the swap chain and would otherwise be
        // guessing at the same number the renderer already knows. Window messages arrive in client
        // pixels and are scaled by the ratio of the two on the way in, which is what a DPI scale
        // the process did not opt into needs (AC7 on a 150 percent desktop: a 1067x600 client area
        // presenting 1600x900). A game that letterboxes inside its client area would still need the
        // rectangle the back buffer occupies, which nothing here knows.
        std::lock_guard<std::mutex> lock(self.guard);
        // Remembered for the message handlers, which scale client pixels to this. The pointer is
        // already in presented pixels, so it passes through.
        self.display_width = static_cast<float>(display_width);
        self.display_height = static_cast<float>(display_height);
        if (out->visible && self.cursor.installed) {
            // The real cursor, freed by the detours: read where it is, every frame, through the
            // genuine function, and scaled from client pixels to presented pixels. Nothing here
            // sends a message, so the lock is safe to hold.
            HWND window = self.window.load(std::memory_order_relaxed);
            POINT point{};
            RECT client{};
            if (window && self.cursor.get_cursor_pos(&point) && ScreenToClient(window, &point) &&
                GetClientRect(window, &client) && client.right > 0 && client.bottom > 0) {
                const float scale_x = self.display_width > 0.0f
                                          ? self.display_width / static_cast<float>(client.right)
                                          : 1.0f;
                const float scale_y = self.display_height > 0.0f
                                          ? self.display_height / static_cast<float>(client.bottom)
                                          : 1.0f;
                self.mouse_x = static_cast<float>(point.x) * scale_x;
                self.mouse_y = static_cast<float>(point.y) * scale_y;
            }
        }
        out->mouse_x = self.mouse_x;
        out->mouse_y = self.mouse_y;
        out->mouse_buttons = self.buttons;
        // Taken, not read. The wheel accumulates between frames and each notch has to reach the
        // overlay exactly once.
        out->scroll_delta = self.scroll;
        self.scroll = 0.0f;
    }
    return RSF_OVERLAY_INPUT_OK;
}
