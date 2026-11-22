// SPDX-License-Identifier: MIT
#include <rescaleframe/render_links.h>
#include <cstdio>
#include <cstdlib>

namespace {
void require(bool value, int line)
{
    if (!value) { std::fprintf(stderr, "render link check failed at %d\n", line); std::exit(1); }
}
#define CHECK(value) require((value), __LINE__)
rsf_render_link output()
{
    return {sizeof(rsf_render_link), RSF_RENDER_LINK_ABI_VERSION, 0, 0, 0, 0, 0};
}
}

int main()
{
    rsf_render_links* links = nullptr;
    CHECK(rsf_render_links_create(42, 2, &links) == RSF_BACKEND_OK && links);
    auto ticket = output(), consumed = output();
    constexpr uint64_t source = UINT64_C(4294967297);
    auto bad = output();
    bad.struct_size -= 1;
    CHECK(rsf_render_links_lookup(links, 100, 8, &bad) == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    bad = output();
    ++bad.abi_version;
    CHECK(rsf_render_links_bind(links, 100, source, 8, &bad) == RSF_BACKEND_ERROR_ABI_MISMATCH);
    CHECK(rsf_render_links_release(links, &bad) == RSF_BACKEND_ERROR_ABI_MISMATCH);

    CHECK(rsf_render_links_bind(links, 100, source, 8, &ticket) == RSF_BACKEND_OK);
    CHECK(rsf_render_links_lookup(links, 100, 8, &consumed) == RSF_BACKEND_OK);
    CHECK(consumed.session_id == 42 && consumed.source_frame_id == source &&
        consumed.submission_id == ticket.submission_id);
    CHECK(rsf_render_links_release(links, &ticket) == RSF_BACKEND_OK);
    CHECK(rsf_render_links_destroy(links) == RSF_BACKEND_OK);
}
