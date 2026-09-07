/* SPDX-License-Identifier: GPL-3.0-only */
/* A vendor that is not here still has to answer.
 *
 * The whole point of the contract is that the orchestrator asks all three vendors the same question
 * and gets an answer from each, including the ones that were not compiled in and the ones whose
 * runtime is missing. Those are different answers and a user chasing a missing feature needs to know
 * which: one is a fact about how this was built, the other about the machine it is running on.
 *
 * So this test runs in both configurations and asserts the part that must hold in both. Whichever
 * way this checkout was built, one of the two branches is being exercised here, and CI builds it
 * both ways for that reason.
 */
#include <rescaleframe/fsr_backend.h>

#include <cstdio>
#include <cstring>

namespace {

bool passed = true;

void check(bool condition, const char* what)
{
    if (!condition) {
        std::fprintf(stderr, "FAIL: %s\n", what);
        passed = false;
    }
}

void stage(const char* what) { std::fprintf(stderr, "[stage] %s\n", what); }

void collect(void* user, const char* message)
{
    (void)user;
    std::fprintf(stderr, "  %s\n", message);
}

} // namespace

int main()
{
    stage("the provider exists whether or not the SDK does");
    const rsf_sr_provider* sr = rsf_fsr_sr_provider();
    const rsf_fg_provider* fg = rsf_fsr_fg_provider();
    check(sr != nullptr && fg != nullptr,
          "Both providers must exist. A caller asks every vendor the same question, and a vendor "
          "that is not compiled in must answer rather than fail to link.");
    if (!sr || !fg) {
        return 1;
    }
    check(sr->struct_size == sizeof(rsf_sr_provider) && fg->struct_size == sizeof(rsf_fg_provider),
          "And must be built against this version of the contract.");
    check(sr->probe != nullptr && sr->open != nullptr && sr->evaluate != nullptr,
          "Every entry point must be present, because the ones that cannot work return a reason "
          "and a null pointer is not a reason.");
    check(fg->create_swapchain != nullptr && fg->tag_frame != nullptr,
          "Including the generation ones.");

    rsf_backend_probe_desc desc{};
    desc.struct_size = sizeof(desc);
    desc.abi_version = RSF_BACKEND_ABI_VERSION;
    desc.game_api = RSF_API_D3D11;
    desc.log = collect;

    rsf_backend_caps caps{};
    caps.struct_size = sizeof(caps);

    stage("probing");
    const rsf_backend_result result = sr->probe(&desc, &caps);
    check(result == RSF_BACKEND_OK || result == RSF_BACKEND_ERROR_NOT_COMPILED,
          "A probe either answers or says it was not built in. Anything else means it tried to do "
          "something, and a probe must not: it is asked of every vendor on a path that has not "
          "committed to any of them.");
    check(caps.vendor == RSF_VENDOR_AMD, "The vendor must be named either way.");
    check(caps.name != nullptr, "And so must the backend, so a report can say which one refused.");

    if (result == RSF_BACKEND_ERROR_NOT_COMPILED) {
        stage("built without the headers");
        check(caps.available == 0, "It must not claim to be available.");
        check(caps.refusal_utf8 != nullptr && std::strlen(caps.refusal_utf8) > 0,
              "And must say why in words, because 'unavailable' sends a user looking for a driver "
              "problem that is really a build option.");
        check(caps.sr_apis == 0 && caps.fg_apis == 0,
              "A backend that is not there offers nothing, or negotiation will choose it.");

        rsf_sr_open_desc open{};
        open.struct_size = sizeof(open);
        open.abi_version = RSF_BACKEND_ABI_VERSION;
        void* session = nullptr;
        check(sr->open(&open, &session) == RSF_BACKEND_ERROR_NOT_COMPILED,
              "Opening must give the same answer as probing rather than a different failure that "
              "sends the reader somewhere else.");
        check(fg->tag_frame(nullptr, nullptr, nullptr) == RSF_BACKEND_ERROR_NOT_COMPILED,
              "And so must every other entry point.");
    } else {
        stage("built with the headers");
        check(caps.available == 1, "It must say so.");
        check((caps.sr_apis & RSF_API_D3D12) != 0 && (caps.fg_apis & RSF_API_D3D12) != 0,
              "FidelityFX is D3D12 on both sides, which is why choosing it for reconstruction "
              "means the presentation bridge exists even with generation off.");
        check((caps.sr_apis & RSF_API_D3D11) == 0,
              "And offers no D3D11 path, which is the fact that decides the route.");
        check(caps.max_generated_frames == 1,
              "One generated frame between each pair, so a multiplier above two is capped.");
        check(caps.sr_fg_share_session == 0,
              "No shared session, so choosing it for generation leaves reconstruction free to be "
              "another vendor. This is what makes it the first generator worth wiring up.");
        check(caps.fg_requires_latency_markers == 0,
              "And it needs no latency markers, so a frame whose identifier is ambiguous can still "
              "be interpolated around.");
        check(caps.fg_owns_swapchain == 1, "It replaces the presentation chain, as all three do.");
    }

    stage("arguments are checked");
    {
        check(sr->probe(nullptr, &caps) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "A null probe.");
        check(sr->probe(&desc, nullptr) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "A null caps.");
        rsf_backend_probe_desc wrong = desc;
        wrong.abi_version = RSF_BACKEND_ABI_VERSION + 1u;
        check(sr->probe(&wrong, &caps) == RSF_BACKEND_ERROR_ABI_MISMATCH,
              "An ABI mismatch must be refused before anything is filled in.");
        rsf_backend_caps short_caps{};
        short_caps.struct_size = 4;
        check(sr->probe(&desc, &short_caps) == RSF_BACKEND_ERROR_INVALID_ARGUMENT,
              "A short caps structure.");
    }

    std::fprintf(stderr, "%s\n", passed ? "fsr_backend: all checks passed" : "fsr_backend: FAILED");
    return passed ? 0 : 1;
}
