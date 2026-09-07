// SPDX-License-Identifier: GPL-3.0-only

#include <rescaleframe/overlay_input.h>

#include <windows.h>

#include <windowsx.h>

#include <atomic>
#include <cstdarg>
#include <cstdio>
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
    std::atomic<uint32_t> toggle_key{VK_F7};

    // Serialises install against uninstall. The window procedure never takes this one, so the
    // SetWindowLongPtrW calls made under it cannot end up waiting on a message thread that is
    // waiting on us.
    std::mutex lifecycle;

    // The input the window procedure records and the render thread reads. Everything under this
    // lock is a handful of scalar stores, and nothing is called while holding it: this runs inside
    // the game's own message dispatch, so anything that could send a message or re-enter the
    // procedure would be re-entering the lock with it.
    std::mutex guard;
    float mouse_x = 0.0f;
    float mouse_y = 0.0f;
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

// Both callers hold `lifecycle` across this, so a log sink that calls back into install or
// uninstall deadlocks on a non-recursive mutex. Nothing in the runtime does, and the other sinks in
// this directory log the same way, but it is a real constraint on what a sink may do.
void say(const State& self, const char* format, ...)
{
    if (!self.log) {
        return;
    }
    char message[512];
    va_list arguments;
    va_start(arguments, format);
    std::vsnprintf(message, sizeof(message), format, arguments);
    va_end(arguments);
    self.log(self.log_user, message);
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

void record_mouse_position(State& self, LPARAM lparam)
{
    // Client pixels, which is the coordinate space the overlay works in. See the assumption note in
    // rsf_overlay_input_collect: this module does not scale them.
    const float x = static_cast<float>(GET_X_LPARAM(lparam));
    const float y = static_cast<float>(GET_Y_LPARAM(lparam));
    std::lock_guard<std::mutex> lock(self.guard);
    self.mouse_x = x;
    self.mouse_y = y;
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
    if (!window || !GetCursorPos(&point) || !ScreenToClient(window, &point)) {
        return;
    }
    // Whatever the game left the cursor at. A game that hid and centred it reports the centre,
    // which is as good an answer as exists before the first move.
    std::lock_guard<std::mutex> lock(self.guard);
    self.mouse_x = static_cast<float>(point.x);
    self.mouse_y = static_cast<float>(point.y);
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
    if (visible) {
        seed_cursor_position(self);
    }
    // Announced because this is the one thing that happens on the message thread when the toggle
    // is pressed, and a crash on the first opened frame looks identical whether the window
    // procedure or the render thread died. The last line in the log says which.
    say(self, visible ? "overlay input: toggle pressed, now visible"
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
            record_mouse_position(self, lparam);
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
        uint32_t key = VK_LBUTTON;
        if (message == WM_RBUTTONDOWN || message == WM_RBUTTONDBLCLK) {
            key = VK_RBUTTON;
        } else if (message == WM_MBUTTONDOWN || message == WM_MBUTTONDBLCLK) {
            key = VK_MBUTTON;
        } else if (message == WM_XBUTTONDOWN || message == WM_XBUTTONDBLCLK) {
            key = GET_XBUTTON_WPARAM(wparam) == XBUTTON2 ? VK_XBUTTON2 : VK_XBUTTON1;
        }
        if (visible) {
            // The position first: a click can arrive without a move before it, and the overlay
            // would otherwise apply it wherever the cursor was last seen.
            record_mouse_position(self, lparam);
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
        uint32_t key = VK_LBUTTON;
        if (message == WM_RBUTTONUP) {
            key = VK_RBUTTON;
        } else if (message == WM_MBUTTONUP) {
            key = VK_MBUTTON;
        } else if (message == WM_XBUTTONUP) {
            key = GET_XBUTTON_WPARAM(wparam) == XBUTTON2 ? VK_XBUTTON2 : VK_XBUTTON1;
        }
        const bool owed = release_is_owed_to_game(self, key);
        if (visible) {
            record_mouse_position(self, lparam);
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

    const uint32_t toggle = options->toggle_virtual_key ? options->toggle_virtual_key : VK_F7;
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
        say(self, "overlay input: window is ANSI, subclassing it with the wide entry point "
                  "converts it to Unicode");
    }

    // The original goes in before the swap, not after. A message can reach the new procedure on the
    // game's thread the instant SetWindowLongPtrW returns, and the forward pointer has to already
    // be there when it does.
    const LONG_PTR existing = GetWindowLongPtrW(target, GWLP_WNDPROC);
    if (existing == 0) {
        say(self, "overlay input: could not read the window procedure, error %lu",
            static_cast<unsigned long>(GetLastError()));
        return RSF_OVERLAY_INPUT_ERROR_SUBCLASS_FAILED;
    }
    self.original.store(reinterpret_cast<WNDPROC>(existing), std::memory_order_release);
    self.installed.store(true, std::memory_order_release);

    say(self, "overlay input: subclassing window %p, toggle key 0x%02x", static_cast<void*>(target),
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
        say(self, "overlay input: subclass failed, error %lu",
            static_cast<unsigned long>(swap_error));
        return RSF_OVERLAY_INPUT_ERROR_SUBCLASS_FAILED;
    }
    if (previous != existing) {
        // Something replaced the procedure between the read and the swap. The value the swap
        // returned is the one that was actually displaced, so that is the chain to forward to.
        self.original.store(reinterpret_cast<WNDPROC>(previous), std::memory_order_release);
        say(self, "overlay input: window procedure changed during install, forwarding to the "
                  "displaced one");
    }
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
        say(self, "overlay input: window is gone, dropping the subclass without restoring");
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
        say(self, "overlay input: uninstall refused, the window procedure is not ours");
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
    self.installed.store(false, std::memory_order_release);
    self.visible.store(0, std::memory_order_release);
    self.window.store(nullptr, std::memory_order_relaxed);
    forget_game_holds(self);
    clear_transient_input(self);
    say(self, "overlay input: window procedure restored");
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
        // Mouse position is passed through in client pixels with no scaling, which assumes the
        // client area maps one to one onto the image being presented. That holds for a borderless
        // window at the desktop resolution and for exclusive fullscreen at the native mode, and it
        // is why the display size comes from the caller: this module has no view of the swap chain
        // and would otherwise be guessing at the same number the renderer already knows.
        //
        // It does not hold when DXGI stretches a smaller back buffer to the window, when the game
        // letterboxes to keep an aspect ratio, or under a per monitor DPI scale the process did not
        // opt into. In those cases clicks land offset from the cursor. Correcting it needs the
        // rectangle the back buffer occupies inside the client area, which is the renderer's
        // knowledge, not this module's. Untested against the game either way.
        std::lock_guard<std::mutex> lock(self.guard);
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
