// Read a view uniform buffer, and refuse the things that are not one.
//
// The buffer here is built rather than captured, because a captured one cannot be committed and a
// test that needs game data does not run in CI. What it is built to satisfy are the same
// relationships the reader checks, so the two agree by construction on a good buffer and the
// interesting cases are the bad ones.
//
// Pass a directory as an argument and it additionally reads every `*_cb4096.bin` in it, which is
// how the reader gets checked against buffers the game actually bound. That path is for running by
// hand against an untracked capture directory; the registered test takes no argument.

#include <rescaleframe/ac7_view.h>

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <string>
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

void check_near(float actual, float expected, float tolerance, const char* message)
{
    const float difference = std::fabs(actual - expected);
    if (difference > tolerance) {
        std::fprintf(stderr, "%s (got %.5f, expected %.5f)\n", message, double(actual),
                     double(expected));
        passed = false;
    }
}

constexpr uint32_t kViewToTranslatedWorld = 0x0C0;
constexpr uint32_t kViewToClip = 0x180;
constexpr uint32_t kClipToView = 0x1C0;
constexpr uint32_t kViewForward = 0x300;
constexpr uint32_t kViewUp = 0x310;
constexpr uint32_t kViewRight = 0x320;
constexpr uint32_t kWorldCameraOrigin = 0x370;
constexpr uint32_t kPreViewTranslation = 0x3A0;
constexpr uint32_t kClipToPrevClip = 0x6E0;
constexpr uint32_t kTemporalAAJitter = 0x720;
constexpr uint32_t kViewRectMin = 0x7E0;
constexpr uint32_t kViewSize = 0x7F0;
constexpr uint32_t kBufferSize = 0x800;

struct Buffer {
    // Parentheses, not braces: braces here would build a two element vector holding 1024 and 0.
    std::vector<float> values = std::vector<float>(RSF_AC7_VIEW_BUFFER_BYTES / 4, 0.0f);

    void put(uint32_t offset, std::initializer_list<float> data)
    {
        uint32_t index = offset / 4;
        for (float value : data) {
            values[index++] = value;
        }
    }

    const void* bytes() const { return values.data(); }
};

// A view buffer that satisfies every relationship the reader tests. The camera basis is a real
// rotation rather than the identity, so a reader that transposed a matrix would be caught.
Buffer make_view(float horizontal_scale, float vertical_scale, float near_plane,
                 uint32_t view_width, uint32_t view_height, uint32_t buffer_width,
                 uint32_t buffer_height)
{
    Buffer buffer;

    const float right[3] = {0.0f, 1.0f, 0.0f};
    const float up[3] = {0.0f, 0.0f, 1.0f};
    const float forward[3] = {1.0f, 0.0f, 0.0f};

    // Rows of ViewToTranslatedWorld are the basis, in Unreal's order where Z is forward.
    buffer.put(kViewToTranslatedWorld,
               {right[0], right[1], right[2], 0.0f, up[0], up[1], up[2], 0.0f, forward[0],
                forward[1], forward[2], 0.0f, 0.0f, 0.0f, 0.0f, 1.0f});
    buffer.put(kViewRight, {right[0], right[1], right[2], 0.0f});
    buffer.put(kViewUp, {up[0], up[1], up[2], 0.0f});
    buffer.put(kViewForward, {forward[0], forward[1], forward[2], 0.0f});

    // Reversed Z with an infinite far plane, the form the game uses.
    buffer.put(kViewToClip, {horizontal_scale, 0.0f, 0.0f, 0.0f,
                             0.0f, vertical_scale, 0.0f, 0.0f,
                             0.0f, 0.0f, 0.0f, 1.0f,
                             0.0f, 0.0f, near_plane, 0.0f});
    buffer.put(kClipToView, {1.0f / horizontal_scale, 0.0f, 0.0f, 0.0f,
                             0.0f, 1.0f / vertical_scale, 0.0f, 0.0f,
                             0.0f, 0.0f, 0.0f, 1.0f / near_plane,
                             0.0f, 0.0f, 1.0f, 0.0f});

    const float camera[3] = {260968.45f, 107399.90f, 21228.20f};
    buffer.put(kWorldCameraOrigin, {camera[0], camera[1], camera[2], 0.0f});
    buffer.put(kPreViewTranslation, {-camera[0], -camera[1], -camera[2], 0.0f});

    // A small camera movement, the shape ClipToPrevClip actually takes.
    buffer.put(kClipToPrevClip, {1.0f, -0.00009f, 0.0f, 0.0f,
                                 0.00003f, 1.0f, 0.0f, 0.0f,
                                 -0.02808f, -0.20304f, 1.0f, 17.49332f,
                                 -0.00001f, 0.00003f, 0.0f, 1.0f});

    buffer.put(kViewRectMin, {0.0f, 0.0f, 0.0f, 0.0f});
    buffer.put(kViewSize, {float(view_width), float(view_height), 1.0f / float(view_width),
                           1.0f / float(view_height)});
    buffer.put(kBufferSize, {float(buffer_width), float(buffer_height),
                             1.0f / float(buffer_width), 1.0f / float(buffer_height)});
    return buffer;
}

rsf_ac7_view read(const Buffer& buffer, rsf_ac7_view_result* result)
{
    rsf_ac7_view view{};
    view.struct_size = sizeof(view);
    *result = rsf_ac7_view_read(buffer.bytes(), RSF_AC7_VIEW_BUFFER_BYTES,
                                RSF_AC7_VIEW_ABI_VERSION, &view);
    return view;
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
    int found = 0, accepted = 0, main_views = 0;
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
        main_views += view.is_main_view;
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
                    view.is_main_view ? "main view" : "secondary view", jitter);
        check_inverse(view);
    }
    std::printf("\n%d buffers read, %d accepted as perspective views, %d of those the main view\n",
                found, accepted, main_views);
    return found == 0 ? 2 : (passed ? 0 : 1);
}


// Row-vector 4x4 algebra, the engine's convention, for building view buffers to check against.
struct Mat {
    double m[4][4];
};
Mat mul(const Mat& a, const Mat& b)
{
    Mat r{};
    for (int i = 0; i < 4; ++i)
        for (int j = 0; j < 4; ++j)
            for (int k = 0; k < 4; ++k)
                r.m[i][j] += a.m[i][k] * b.m[k][j];
    return r;
}
Mat inverse(Mat a)
{
    Mat r{};
    for (int i = 0; i < 4; ++i) r.m[i][i] = 1.0;
    for (int c = 0; c < 4; ++c) {
        int pivot = c;
        for (int i = c + 1; i < 4; ++i)
            if (std::fabs(a.m[i][c]) > std::fabs(a.m[pivot][c])) pivot = i;
        std::swap(a.m[c], a.m[pivot]);
        std::swap(r.m[c], r.m[pivot]);
        const double d = a.m[c][c];
        for (int j = 0; j < 4; ++j) { a.m[c][j] /= d; r.m[c][j] /= d; }
        for (int i = 0; i < 4; ++i) {
            if (i == c) continue;
            const double f = a.m[i][c];
            for (int j = 0; j < 4; ++j) { a.m[i][j] -= f * a.m[c][j]; r.m[i][j] -= f * r.m[c][j]; }
        }
    }
    return r;
}
void store(std::vector<unsigned char>& buffer, uint32_t offset, const Mat& a)
{
    float values[16];
    for (int i = 0; i < 16; ++i) values[i] = float(a.m[i / 4][i % 4]);
    std::memcpy(buffer.data() + offset, values, sizeof(values));
}
// A view buffer from a camera and projection, filled the way SceneView.cpp:2259 fills it.
std::vector<unsigned char> view_buffer(const Mat& translated_world_to_view, const Mat& projection,
                                       const double pre_view_translation[3], float jx, float jy)
{
    std::vector<unsigned char> buffer(RSF_AC7_VIEW_BUFFER_BYTES, 0);
    Mat translate{};
    for (int i = 0; i < 4; ++i) translate.m[i][i] = 1.0;
    for (int i = 0; i < 3; ++i) translate.m[3][i] = pre_view_translation[i];
    const Mat to_clip = mul(translated_world_to_view, projection);
    const Mat world_to_clip = mul(translate, to_clip);
    Mat screen{};
    screen.m[0][0] = 1.0; screen.m[1][1] = 1.0;
    screen.m[2][2] = projection.m[2][2]; screen.m[2][3] = 1.0; screen.m[3][2] = projection.m[3][2];
    Mat sv{};
    sv.m[0][0] = 2.0 / 1024.0; sv.m[1][1] = -2.0 / 576.0; sv.m[2][2] = 1.0;
    sv.m[3][0] = -1.0; sv.m[3][1] = 1.0; sv.m[3][3] = 1.0;
    const Mat screen_to_translated_world = mul(screen, inverse(to_clip));
    store(buffer, 0x000, to_clip);
    store(buffer, 0x040, world_to_clip);
    store(buffer, 0x080, translated_world_to_view);
    store(buffer, 0x180, projection);
    store(buffer, 0x1C0, inverse(projection));
    store(buffer, 0x200, inverse(to_clip));
    store(buffer, 0x240, mul(sv, screen_to_translated_world));
    store(buffer, 0x280, mul(screen, inverse(world_to_clip)));
    store(buffer, 0x2C0, screen_to_translated_world);
    const float jitter[4] = {jx, jy, 0.0f, 0.0f};
    std::memcpy(buffer.data() + 0x720, jitter, sizeof(jitter));
    return buffer;
}
} // namespace

int main(int argc, char* argv[])
{
    if (argc == 2) {
        return read_directory(argv[1]);
    }

    const Buffer good = make_view(1.63185f, 2.90107f, 1.0f, 2048, 1152, 2048, 1152);

    rsf_ac7_view_result result = RSF_AC7_VIEW_OK;
    rsf_ac7_view view = read(good, &result);
    check(result == RSF_AC7_VIEW_OK, "A well formed view buffer must be accepted.");

    check_near(view.aspect_ratio, 16.0f / 9.0f, 1e-3f, "The aspect ratio comes from the projection.");
    check_near(view.vertical_fov, 2.0f * std::atan(1.0f / 2.90107f), 1e-5f,
               "The vertical field of view comes from the projection.");
    check_near(view.near_plane, 1.0f, 1e-5f, "The near plane is the matrix element, not a guess.");
    check_near(view.camera_position[0], 260968.45f, 1.0f, "The camera position is read.");
    check_near(view.camera_forward[0], 1.0f, 1e-5f, "Forward is Unreal's view space Z.");
    check_near(view.camera_up[2], 1.0f, 1e-5f, "Up is Unreal's view space Y.");
    check_near(view.camera_right[1], 1.0f, 1e-5f, "Right is Unreal's view space X.");
    check(view.view_width == 2048 && view.view_height == 1152, "The view size is read.");
    check(view.is_main_view == 1u, "A view filling its target is the main view.");
    check_near(view.clip_to_prev_clip[11], 17.49332f, 1e-4f, "ClipToPrevClip is read as stored.");
    check_inverse(view);
    check(view.has_jitter == 0u, "A buffer without a jitter must not claim one.");

    // The jitter as the running game produced it, from capture10: the engine writes the same clip
    // space offset into TemporalAAJitter and into two elements of the projection.
    Buffer jittered = good;
    const float clip_x = -0.00042f, clip_y = 0.00027f;
    jittered.put(kTemporalAAJitter, {clip_x, clip_y, 0.000168f, 0.00025f});
    jittered.put(kViewToClip + 8 * 4, {clip_x, clip_y});
    view = read(jittered, &result);
    check(result == RSF_AC7_VIEW_OK, "A jittered view buffer must be accepted.");
    check(view.has_jitter == 1u, "A jittered projection must be reported as such.");

    // Clip space divided by the view rect, not the buffer. Those differ once the render scale
    // moves, and using the buffer would scale every offset without ever looking wrong.
    check_near(view.jitter_pixels[0], clip_x * 2048.0f * 0.5f, 1e-4f,
               "The horizontal jitter comes back in pixels.");
    check_near(view.jitter_pixels[1], clip_y * 1152.0f * -0.5f, 1e-4f,
               "The vertical jitter comes back in pixels, with the engine's sign flip.");
    check(std::fabs(view.jitter_pixels[0]) <= 0.5f && std::fabs(view.jitter_pixels[1]) <= 0.5f,
          "A real jitter stays inside half a pixel.");
    check_near(view.previous_jitter_pixels[0], 0.000168f * 2048.0f * 0.5f, 1e-4f,
               "The previous frame's jitter sits beside the current one.");

    // 4.18 keeps no un-jittered projection, so the two elements it wrote have to come back out.
    check_near(view.view_to_clip[8], clip_x, 1e-6f, "The projection keeps the jitter as stored.");
    check_near(view.view_to_clip_no_jitter[8], 0.0f, 1e-6f,
               "The un-jittered projection has the offset removed.");
    check_near(view.view_to_clip_no_jitter[9], 0.0f, 1e-6f,
               "Both elements of the offset are removed.");
    check_near(view.view_to_clip_no_jitter[0], view.view_to_clip[0], 1e-6f,
               "Removing the jitter must leave the rest of the projection alone.");

    // A view that does not fill its target is one of the smaller ones the engine also renders, and
    // its camera describes something the player is not looking through.
    const Buffer secondary = make_view(1.63185f, 2.90107f, 1.0f, 1016, 1016, 2048, 1152);
    view = read(secondary, &result);
    check(result == RSF_AC7_VIEW_OK, "A secondary view is still a view buffer.");
    check(view.is_main_view == 0u, "A view smaller than its target is not the main view.");

    // The interface renders through an orthographic view, which has no perspective divide. Built
    // as a consistent projection and its true inverse, taken from a captured interface view:
    // zeroing one element of a perspective matrix would fail the inverse check first and be
    // refused as not a view buffer at all, which is a different answer.
    Buffer orthographic = good;
    orthographic.put(kViewToClip, {1.0f / 1024.0f, 0.0f, 0.0f, 0.0f,
                                   0.0f, -1.0f / 576.0f, 0.0f, 0.0f,
                                   0.0f, 0.0f, -1.0f / 200.0f, 0.0f,
                                   -1.0f, 1.0f, 0.5f, 1.0f});
    orthographic.put(kClipToView, {1024.0f, 0.0f, 0.0f, 0.0f,
                                   0.0f, -576.0f, 0.0f, 0.0f,
                                   0.0f, 0.0f, -200.0f, 0.0f,
                                   1024.0f, 576.0f, 100.0f, 1.0f});
    view = read(orthographic, &result);
    check(result == RSF_AC7_VIEW_ERROR_NOT_PERSPECTIVE,
          "An orthographic view must be reported as such, not parsed as a camera.");

    // Everything below is not a view buffer, and must be refused rather than parsed.
    rsf_ac7_view target{};
    target.struct_size = sizeof(target);

    check(rsf_ac7_view_read(good.bytes(), RSF_AC7_VIEW_BUFFER_BYTES,
                            RSF_AC7_VIEW_ABI_VERSION + 1u, &target) ==
              RSF_AC7_VIEW_ERROR_ABI_MISMATCH,
          "An incompatible ABI must be rejected.");
    check(rsf_ac7_view_read(nullptr, RSF_AC7_VIEW_BUFFER_BYTES, RSF_AC7_VIEW_ABI_VERSION,
                            &target) == RSF_AC7_VIEW_ERROR_INVALID_ARGUMENT,
          "A missing buffer must be rejected.");
    check(rsf_ac7_view_read(good.bytes(), 2640, RSF_AC7_VIEW_ABI_VERSION, &target) ==
              RSF_AC7_VIEW_ERROR_NOT_A_VIEW_BUFFER,
          "A buffer of the wrong size cannot be the view buffer.");

    // A buffer of plausible floats rather than zeros: at runtime the candidates are other
    // constant buffers, not empty memory.
    Buffer noise;
    for (size_t index = 0; index < noise.values.size(); ++index) {
        noise.values[index] = static_cast<float>((index * 37 % 211)) * 0.01f - 1.0f;
    }
    check(rsf_ac7_view_read(noise.bytes(), RSF_AC7_VIEW_BUFFER_BYTES, RSF_AC7_VIEW_ABI_VERSION,
                            &target) == RSF_AC7_VIEW_ERROR_NOT_A_VIEW_BUFFER,
          "A constant buffer that is not the view buffer must be refused.");

    // One field wrong is enough. This is the case that matters: a buffer that looks right except
    // for the part being relied on.
    Buffer broken = good;
    broken.put(kViewUp, {0.5f, 0.5f, 0.5f, 0.0f});
    check(rsf_ac7_view_read(broken.bytes(), RSF_AC7_VIEW_BUFFER_BYTES, RSF_AC7_VIEW_ABI_VERSION,
                            &target) == RSF_AC7_VIEW_ERROR_NOT_A_VIEW_BUFFER,
          "A basis vector that is not a unit vector must be refused.");

    Buffer wrong_translation = good;
    wrong_translation.put(kPreViewTranslation, {1.0f, 2.0f, 3.0f, 0.0f});
    check(rsf_ac7_view_read(wrong_translation.bytes(), RSF_AC7_VIEW_BUFFER_BYTES,
                            RSF_AC7_VIEW_ABI_VERSION, &target) ==
              RSF_AC7_VIEW_ERROR_NOT_A_VIEW_BUFFER,
          "A translation that is not the negated camera position must be refused.");

    Buffer wrong_size = good;
    wrong_size.put(kBufferSize, {2048.0f, 1152.0f, 0.5f, 0.5f});
    check(rsf_ac7_view_read(wrong_size.bytes(), RSF_AC7_VIEW_BUFFER_BYTES,
                            RSF_AC7_VIEW_ABI_VERSION, &target) ==
              RSF_AC7_VIEW_ERROR_NOT_A_VIEW_BUFFER,
          "A size not paired with its reciprocal must be refused.");

    // Removing the jitter from a view buffer gives the buffer of the same camera without it, in
    // every field the jitter went into.
    {
        Mat camera{};
        const double angle = 0.4;
        camera.m[0][0] = std::cos(angle); camera.m[0][2] = std::sin(angle);
        camera.m[1][1] = 1.0;
        camera.m[2][0] = -std::sin(angle); camera.m[2][2] = std::cos(angle);
        camera.m[3][0] = 120.0; camera.m[3][1] = -40.0; camera.m[3][2] = 900.0; camera.m[3][3] = 1.0;
        Mat projection{};
        projection.m[0][0] = 1.2; projection.m[1][1] = 2.1;
        projection.m[2][3] = 1.0; projection.m[3][2] = 10.0;
        const double translation[3] = {-5000.0, 250.0, 1200.0};
        const float jx = 0.00031f;
        const float jy = -0.00052f;
        Mat jittered_projection = projection;
        jittered_projection.m[2][0] += jx;
        jittered_projection.m[2][1] += jy;
        const std::vector<unsigned char> jittered_buffer =
            view_buffer(camera, jittered_projection, translation, jx, jy);
        const std::vector<unsigned char> expected =
            view_buffer(camera, projection, translation, 0.0f, 0.0f);
        std::vector<unsigned char> removed(RSF_AC7_VIEW_BUFFER_BYTES, 0);
        check(rsf_ac7_view_remove_jitter(jittered_buffer.data(), removed.data(),
                                         RSF_AC7_VIEW_BUFFER_BYTES) == 1,
              "A jittered perspective view must have its jitter removed.");
        const uint32_t fields[] = {0x000, 0x040, 0x180, 0x1C0, 0x200, 0x240, 0x280, 0x2C0};
        const char* names[] = {"TranslatedWorldToClip", "WorldToClip", "ViewToClip", "ClipToView",
                               "ClipToTranslatedWorld", "SVPositionToTranslatedWorld",
                               "ScreenToWorld", "ScreenToTranslatedWorld"};
        for (size_t f = 0; f < sizeof(fields) / sizeof(fields[0]); ++f) {
            float got[16];
            float want[16];
            std::memcpy(got, removed.data() + fields[f], sizeof(got));
            std::memcpy(want, expected.data() + fields[f], sizeof(want));
            double worst = 0.0;
            for (int i = 0; i < 16; ++i) {
                const double scale = std::fabs(want[i]) > 1.0 ? std::fabs(want[i]) : 1.0;
                worst = std::max(worst, std::fabs(double(got[i]) - double(want[i])) / scale);
            }
            if (worst > 1e-4) {
                std::fprintf(stderr, "%s differs by %g after removal\n", names[f], worst);
                passed = false;
            }
        }
        float jitter_after[2];
        std::memcpy(jitter_after, removed.data() + 0x720, sizeof(jitter_after));
        check(jitter_after[0] == 0.0f && jitter_after[1] == 0.0f,
              "The jitter field itself must read zero after removal.");
        std::vector<unsigned char> untouched(RSF_AC7_VIEW_BUFFER_BYTES, 0);
        check(rsf_ac7_view_remove_jitter(expected.data(), untouched.data(),
                                         RSF_AC7_VIEW_BUFFER_BYTES) == 0 &&
                  untouched == expected,
              "A view without jitter must be copied unchanged.");
    }

    return passed ? 0 : 1;
}
