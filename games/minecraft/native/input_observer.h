// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <windows.h>
#include <atomic>

// Report removed GLFW window messages. The core recognises pings and assigns frame identity.
class MinecraftInputObserver {
    std::atomic<HWND> parent{nullptr},child{nullptr};
    using Sink=void(*)(UINT);
    std::atomic<Sink> sink{nullptr};
    HHOOK hook=nullptr;
    static LRESULT CALLBACK observe(int code,WPARAM removed,LPARAM data) {
        auto& self=instance();
        if(code>=0&&removed==PM_REMOVE) {
            const auto& event=*reinterpret_cast<const MSG*>(data);
            if ((!event.hwnd||event.hwnd==self.parent.load()||event.hwnd==self.child.load()))
                if (const auto report=self.sink.load()) report(event.message);
        }
        return CallNextHookEx(nullptr,code,removed,data);
    }
public:
    static MinecraftInputObserver& instance() {static MinecraftInputObserver observer;return observer;}
    bool install(HWND owner,HWND surface,Sink report) {
        if(hook||GetWindowThreadProcessId(owner,nullptr)!=GetCurrentThreadId())return false;
        parent.store(owner);child.store(surface);sink.store(report);
        hook=SetWindowsHookExW(WH_GETMESSAGE,observe,nullptr,GetCurrentThreadId());
        return hook!=nullptr;
    }
    void close() {
        sink.store(nullptr);parent.store(nullptr);child.store(nullptr);
        if(hook) {UnhookWindowsHookEx(hook);hook=nullptr;}
    }
};
