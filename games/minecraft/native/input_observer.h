// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <windows.h>
#include <atomic>

// The GLFW event owner only records dequeued tool pings. SDK calls belong to
// the next simulation's acquired token on the render thread. No input injection.
class MinecraftInputObserver {
    std::atomic<HWND> parent{nullptr},child{nullptr};
    std::atomic<UINT> message{0};
    std::atomic<bool> pending{false};
    HHOOK hook=nullptr;
    static LRESULT CALLBACK observe(int code,WPARAM removed,LPARAM data) {
        auto& self=instance();
        if(code>=0&&removed==PM_REMOVE) {
            const auto& event=*reinterpret_cast<const MSG*>(data);
            const auto expected=self.message.load(std::memory_order_acquire);
            if(expected&&event.message==expected&&
                (!event.hwnd||event.hwnd==self.parent.load()||event.hwnd==self.child.load()))self.pending.store(true,std::memory_order_release);
        }
        return CallNextHookEx(nullptr,code,removed,data);
    }
public:
    static MinecraftInputObserver& instance() {static MinecraftInputObserver observer;return observer;}
    bool install(HWND owner,HWND surface) {
        if(hook||GetWindowThreadProcessId(owner,nullptr)!=GetCurrentThreadId())return false;
        parent.store(owner);child.store(surface);pending.store(false);
        hook=SetWindowsHookExW(WH_GETMESSAGE,observe,nullptr,GetCurrentThreadId());
        return hook!=nullptr;
    }
    void set_message(UINT id) {message.store(id,std::memory_order_release);}
    UINT consume() {
        return pending.exchange(false,std::memory_order_acq_rel)?message.load(std::memory_order_acquire):0;
    }
    void close() {
        message.store(0);parent.store(nullptr);child.store(nullptr);
        if(hook) {UnhookWindowsHookEx(hook);hook=nullptr;}
        pending.store(false);
    }
};
