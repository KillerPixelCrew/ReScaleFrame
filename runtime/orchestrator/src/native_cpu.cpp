// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/native_cpu.h>
#include <array>
#include <mutex>
namespace {
std::mutex guard;
std::array<rsf_native_cpu_frame, 128> frames{};
uint64_t current_session = 0, last_begin = 0;
rsf_game_cpu_event_fn consumer = nullptr;
void* consumer_user = nullptr;
}
extern "C" RSF_RUNTIME_API int rsf_native_cpu_event(const rsf_game_cpu_event* event)
{
    if (!event || event->struct_size < sizeof(*event) || event->stage > RSF_GAME_CPU_PACING ||
        !event->session_id || !event->source_frame_id || !event->timestamp_qpc || !event->qpc_frequency) return 0;
    rsf_game_cpu_event_fn sink = nullptr; void* user = nullptr;
    {
        std::lock_guard<std::mutex> lock(guard);
        if (!current_session && event->stage == RSF_GAME_CPU_FRAME_BEGIN) current_session = event->session_id;
        if (current_session != event->session_id) return 0;
        auto& frame = frames[event->source_frame_id % frames.size()];
        if (event->stage == RSF_GAME_CPU_FRAME_BEGIN) {
            if (event->source_frame_id <= last_begin || (frame.source_frame_id && !frame.ended)) return 0;
            frame = {}; frame.struct_size = sizeof(frame); frame.session_id = event->session_id;
            frame.source_frame_id = event->source_frame_id; frame.qpc_frequency = event->qpc_frequency;
            last_begin = event->source_frame_id;
        } else {
            if (frame.source_frame_id != event->source_frame_id || frame.session_id != event->session_id ||
                frame.ended) return 0;
            if (frame.failed) {
                if (event->stage == RSF_GAME_CPU_FRAME_END) frame.ended = 1;
                return 0;
            }
        }
        if (event->stage == RSF_GAME_CPU_PACING) {
            if (frame.stage_mask != 1u || frame.pacing_qpc || frame.qpc_frequency != event->qpc_frequency ||
                event->timestamp_qpc < frame.timestamps_qpc[0]) return 0;
            frame.pacing_qpc = event->timestamp_qpc;
            sink = consumer; user = consumer_user;
        } else if (event->stage == RSF_GAME_CPU_INPUT_EVENT) {
            if (event->input_kind & ~7u || (!event->input_kind && event->message_id < 0xc000u) ||
                event->qpc_frequency != frame.qpc_frequency || event->timestamp_qpc < frame.timestamps_qpc[0]) return 0;
            for (uint32_t i = 0; i < 3; ++i) if (event->input_kind & (1u << i)) {
                frame.input_mask |= 1u << i;
                ++frame.input_events[i]; frame.input_qpc[i] = event->timestamp_qpc;
            }
            sink = consumer; user = consumer_user;
        } else {
        const uint32_t expected = (1u << event->stage) - 1u;
        // Idle/loading frames may close before input or simulation; they remain explicit partial
        // CPU records. They cannot be promoted to a complete render or latency frame.
        const bool partial_end = event->stage == RSF_GAME_CPU_FRAME_END &&
            (frame.stage_mask == 1u || frame.stage_mask == 3u);
        uint64_t previous = 0;
        for (const auto stamp : frame.timestamps_qpc) if (stamp > previous) previous = stamp;
        if (frame.qpc_frequency != event->qpc_frequency || event->timestamp_qpc < previous ||
            (!partial_end && frame.stage_mask != expected)) {
            frame.failed = 1;
            if (event->stage == RSF_GAME_CPU_FRAME_END) frame.ended = 1;
            return 0;
        }
        frame.timestamps_qpc[event->stage] = event->timestamp_qpc;
        frame.stage_mask |= 1u << event->stage;
        frame.ended = event->stage == RSF_GAME_CPU_FRAME_END;
        sink = consumer; user = consumer_user;
        }
    }
    if (sink) sink(user, event);
    return 1;
}
extern "C" RSF_RUNTIME_API int rsf_native_cpu_read(uint64_t session, uint64_t source, rsf_native_cpu_frame* frame)
{
    if (!frame || frame->struct_size < sizeof(*frame) || !source) return 0;
    std::lock_guard<std::mutex> lock(guard);
    const auto& record = frames[source % frames.size()];
    if (record.session_id != session || record.source_frame_id != source) return 0;
    *frame = record; return 1;
}
extern "C" RSF_RUNTIME_API void rsf_native_cpu_set_sink(rsf_game_cpu_event_fn sink, void* user)
{ std::lock_guard<std::mutex> lock(guard); consumer = sink; consumer_user = user; }
extern "C" RSF_RUNTIME_API void rsf_native_cpu_release(void)
{
    std::lock_guard<std::mutex> lock(guard);
    frames = {}; current_session = last_begin = 0; consumer = nullptr; consumer_user = nullptr;
}
