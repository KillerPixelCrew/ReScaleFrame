// SPDX-License-Identifier: MIT
// Native ABI smoke test and optional probe for untracked 4096-byte capture buffers.
// Synthetic camera and jitter cases run in games/ac7/src/view/tests.rs.
#include <rescaleframe/ac7_view.h>

#include <cmath>
#include <cstdint>
#include <cstdio>
#include <vector>

namespace {

bool passed = true;

void check(bool condition, const char* message)
{
    if (!condition) {
        std::fprintf(stderr, "%s\n", message);
        passed = false;
    }
}

void check_inverse(const rsf_ac7_view& view)
{
    for (int row = 0; row < 4; ++row) {
        for (int column = 0; column < 4; ++column) {
            double sum = 0.0;
            for (int k = 0; k < 4; ++k) {
                sum += double(view.clip_to_prev_clip[row * 4 + k]) *
                       view.prev_clip_to_clip[k * 4 + column];
            }
            const double expected = row == column ? 1.0 : 0.0;
            if (std::fabs(sum - expected) > 1e-4) {
                std::fprintf(stderr,
                             "prev_clip_to_clip is not the inverse at [%d][%d]: %.6f\n", row,
                             column, sum);
                passed = false;
            }
        }
    }
}

int read_directory(const char* directory)
{
    // Deliberately simple: the capture directory holds one flat set of files.
    int found = 0, accepted = 0, full_targets = 0;
    for (int index = 0; index < 100; ++index) {
        char path[1024];
        std::snprintf(path, sizeof(path), "%s\\capture%02d_cb4096.bin", directory, index);
        std::FILE* stream = std::fopen(path, "rb");
        if (!stream) {
            continue;
        }
        std::vector<unsigned char> data(RSF_AC7_VIEW_BUFFER_BYTES);
        const size_t got = std::fread(data.data(), 1, data.size(), stream);
        std::fclose(stream);
        if (got != data.size()) {
            continue;
        }
        ++found;
        rsf_ac7_view view{};
        view.struct_size = sizeof(view);
        const rsf_ac7_view_result result = rsf_ac7_view_read(
            data.data(), RSF_AC7_VIEW_BUFFER_BYTES, RSF_AC7_VIEW_ABI_VERSION, &view);
        if (result != RSF_AC7_VIEW_OK) {
            std::printf("capture%02d: refused, result %d\n", index, int(result));
            continue;
        }
        ++accepted;
        full_targets += view.fills_target;
        char jitter[128];
        if (view.has_jitter) {
            std::snprintf(jitter, sizeof(jitter), "jitter (%+.3f, %+.3f) px",
                          double(view.jitter_pixels[0]), double(view.jitter_pixels[1]));
            check(std::fabs(view.jitter_pixels[0]) <= 0.5f &&
                      std::fabs(view.jitter_pixels[1]) <= 0.5f,
                  "A jitter read from the game must stay inside half a pixel.");
        } else {
            std::snprintf(jitter, sizeof(jitter), "no jitter");
        }
        std::printf("capture%02d: %ux%u in %ux%u, fov %.1f deg, aspect %.4f, near %.2f, %s, %s\n",
                    index, view.view_width, view.view_height, view.buffer_width,
                    view.buffer_height, double(view.vertical_fov) * 57.29578,
                    double(view.aspect_ratio), double(view.near_plane),
                    view.fills_target ? "full target" : "partial target", jitter);
        check_inverse(view);
    }
    std::printf("\n%d buffers read, %d accepted as perspective views, %d of those full-target views\n",
                found, accepted, full_targets);
    return found == 0 ? 2 : (passed ? 0 : 1);
}

} // namespace

int main(int argc, char** argv)
{
    if (argc == 2) return read_directory(argv[1]);
    rsf_ac7_view view{};
    view.struct_size = sizeof(view);
    return rsf_ac7_view_read(nullptr, RSF_AC7_VIEW_BUFFER_BYTES, RSF_AC7_VIEW_ABI_VERSION,
                             &view) == RSF_AC7_VIEW_ERROR_INVALID_ARGUMENT ? 0 : 1;
}
