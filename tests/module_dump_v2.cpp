// Dump this test's own module and check the structured sidecar (schema 2) against facts the test
// knows independently: a polymorphic class and its vtable, a named worker thread and its start
// address, the exception directory, per-section hashes and a caller-supplied name list.

#include <rescaleframe/module_dump.h>

#include <windows.h>
#include <bcrypt.h>

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

// Outside the anonymous namespace so the recovered name is a plain one.
struct RsfDumpProbeBase {
    virtual ~RsfDumpProbeBase() = default;
    virtual int value() const { return 1; }
};

struct RsfDumpProbeDerived : RsfDumpProbeBase {
    int value() const override { return 2; }
};

namespace {

int passed = 1;

void check(bool condition, const char* message)
{
    if (!condition) {
        std::fprintf(stderr, "%s\n", message);
        passed = 0;
    }
}

bool read_file(const std::string& path, std::string* out)
{
    FILE* stream = std::fopen(path.c_str(), "rb");
    if (!stream) {
        return false;
    }
    std::vector<char> chunk(1 << 16);
    size_t got;
    out->clear();
    while ((got = std::fread(chunk.data(), 1, chunk.size(), stream)) > 0) {
        out->append(chunk.data(), got);
    }
    std::fclose(stream);
    return true;
}

// The sidecar is machine-read, so malformed JSON is a defect even when every field is present.
// This checks string and bracket structure, not the full grammar.
bool json_structure_ok(const std::string& text)
{
    std::vector<char> stack;
    bool in_string = false;
    for (size_t i = 0; i < text.size(); ++i) {
        const char c = text[i];
        if (in_string) {
            if (c == '\\') {
                ++i;
            } else if (c == '"') {
                in_string = false;
            } else if (static_cast<unsigned char>(c) < 0x20) {
                return false;
            }
            continue;
        }
        if (c == '"') {
            in_string = true;
        } else if (c == '{' || c == '[') {
            stack.push_back(c);
        } else if (c == '}' || c == ']') {
            if (stack.empty() || stack.back() != (c == '}' ? '{' : '[')) {
                return false;
            }
            stack.pop_back();
        }
    }
    return !in_string && stack.empty();
}

std::string hex(uint64_t value)
{
    char buffer[32];
    std::snprintf(buffer, sizeof(buffer), "0x%llx", static_cast<unsigned long long>(value));
    return buffer;
}

std::string sha256_hex(const unsigned char* data, size_t size)
{
    BCRYPT_ALG_HANDLE algorithm = nullptr;
    BCRYPT_HASH_HANDLE hash = nullptr;
    unsigned char digest[32] = {};
    if (BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM, nullptr, 0) < 0) {
        return {};
    }
    bool ok = BCryptCreateHash(algorithm, &hash, nullptr, 0, nullptr, 0, 0) >= 0;
    // BCrypt takes a 32-bit length.
    for (size_t offset = 0; ok && offset < size;) {
        const ULONG piece = static_cast<ULONG>(size - offset > (1u << 28) ? (1u << 28) : size - offset);
        ok = BCryptHashData(hash, const_cast<PUCHAR>(data + offset), piece, 0) >= 0;
        offset += piece;
    }
    ok = ok && BCryptFinishHash(hash, digest, sizeof(digest), 0) >= 0;
    if (hash) {
        BCryptDestroyHash(hash);
    }
    BCryptCloseAlgorithmProvider(algorithm, 0);
    if (!ok) {
        return {};
    }
    std::string text;
    for (unsigned char byte : digest) {
        char pair[3];
        std::snprintf(pair, sizeof(pair), "%02x", byte);
        text += pair;
    }
    return text;
}

HANDLE g_release;
const wchar_t* const kThreadName = L"rsf-dump-probe-worker";

DWORD WINAPI worker(LPVOID)
{
    WaitForSingleObject(g_release, 30000);
    return 0;
}

typedef HRESULT(WINAPI* set_thread_description_fn)(HANDLE, PCWSTR);

}  // namespace

int main(int argc, char* argv[])
{
    if (argc != 2) {
        std::fprintf(stderr, "usage: module_dump_v2 <output directory>\n");
        return 2;
    }
    const std::string directory = argv[1];

    const unsigned char* module = reinterpret_cast<const unsigned char*>(GetModuleHandleW(nullptr));

    // Keep the polymorphic object alive and observable so the vtable is not discarded.
    RsfDumpProbeBase* object = new RsfDumpProbeDerived();
    volatile int sink = object->value();
    (void)sink;
    const uint64_t vtable_rva =
        static_cast<uint64_t>(*reinterpret_cast<const unsigned char* const*>(object) - module);

    g_release = CreateEventW(nullptr, TRUE, FALSE, nullptr);
    HANDLE thread = CreateThread(nullptr, 0, worker, nullptr, 0, nullptr);
    check(thread != nullptr, "The worker thread must start.");
    HMODULE kernel = GetModuleHandleW(L"kernel32.dll");
    set_thread_description_fn set_description =
        kernel ? reinterpret_cast<set_thread_description_fn>(
                     reinterpret_cast<void*>(GetProcAddress(kernel, "SetThreadDescription")))
               : nullptr;
    const bool can_name = set_description != nullptr;
    if (thread && can_name) {
        set_description(thread, kThreadName);
    }
    const uint64_t worker_rva =
        static_cast<uint64_t>(reinterpret_cast<const unsigned char*>(&worker) - module);
    Sleep(50);

    const std::string names_path = directory + "\\v2-names.tsv";
    FILE* names = std::fopen(names_path.c_str(), "wb");
    check(names != nullptr, "The name list must be writable.");
    if (names) {
        std::fprintf(names, "# engine names\n0x%llx\tProbe_Worker\n%llu\tProbe_Decimal \"quoted\"\nnot-a-line\n",
                     static_cast<unsigned long long>(worker_rva),
                     static_cast<unsigned long long>(vtable_rva));
        std::fclose(names);
    }

    rsf_dump_options options;
    std::memset(&options, 0, sizeof(options));
    options.struct_size = sizeof(options);
    options.abi_version = RSF_MODULE_DUMP_ABI_VERSION;
    options.output_directory_utf8 = argv[1];
    options.output_name_utf8 = "v2";
    options.entropy_threshold = 7.0;
    options.name_list_path_utf8 = names_path.c_str();
    options.rtti_budget_ms = 20000;

    rsf_dump_report report;
    std::memset(&report, 0, sizeof(report));
    report.struct_size = sizeof(report);
    const rsf_dump_result result = rsf_dump_module(nullptr, &options, &report);
    check(result == RSF_DUMP_OK, "The dump must succeed.");

    // A caller built against the schema 1 structures must still be accepted.
    {
        rsf_dump_options old_options = options;
        old_options.struct_size = static_cast<uint32_t>(RSF_DUMP_OPTIONS_V1_SIZE);
        old_options.name_list_path_utf8 = nullptr;
        old_options.output_name_utf8 = "v1caller";
        rsf_dump_report old_report;
        std::memset(&old_report, 0, sizeof(old_report));
        old_report.struct_size = static_cast<uint32_t>(RSF_DUMP_REPORT_V1_SIZE);
        check(rsf_dump_module(nullptr, &old_options, &old_report) == RSF_DUMP_OK,
              "A schema 1 sized options and report structure must be accepted.");
        check(old_report.struct_size == RSF_DUMP_REPORT_V1_SIZE && old_report.sections_written > 0,
              "A schema 1 report must be filled without overrunning it.");
    }

    SetEvent(g_release);
    if (thread) {
        WaitForSingleObject(thread, 5000);
        CloseHandle(thread);
    }
    delete object;
    if (result != RSF_DUMP_OK) {
        return 1;
    }

    std::string sidecar;
    std::string dump;
    check(read_file(directory + "\\v2.dump.json", &sidecar), "The sidecar must exist.");
    check(read_file(directory + "\\v2.dump", &dump), "The dump must exist.");
    check(json_structure_ok(sidecar), "The sidecar must be structurally valid JSON.");

    check(sidecar.find("\"schema_version\": 2") != std::string::npos,
          "The sidecar must carry its schema version.");
    check(sidecar.find("\"abi_version\": 1") != std::string::npos &&
              sidecar.find("\"code_entropy\"") != std::string::npos &&
              sidecar.find("\"resolves_into\"") != std::string::npos,
          "The schema 1 fields must remain.");
    check(sidecar.find("\"timing_ms\"") != std::string::npos, "Timing must be reported.");

    // Threads.
    const std::string description = std::string("\"description\": \"rsf-dump-probe-worker\"");
    if (can_name) {
        const size_t at = sidecar.find(description);
        check(at != std::string::npos, "The named worker thread must be listed.");
        if (at != std::string::npos) {
            const size_t begin = sidecar.rfind("{\"tid\"", at);
            const size_t end = sidecar.find('}', at);
            check(begin != std::string::npos && end != std::string::npos, "The thread object is malformed.");
            if (begin != std::string::npos && end != std::string::npos) {
                const std::string entry = sidecar.substr(begin, end - begin);
                check(entry.find("\"start_rva\": \"" + hex(worker_rva) + "\"") != std::string::npos,
                      "The worker thread's start address must be its entry point's RVA.");
                check(entry.find("\"dumping\": false") != std::string::npos,
                      "The worker is not the dumping thread.");
                check(entry.find("\"priority\"") != std::string::npos, "Priority must be listed.");
            }
        }
    }
    check(sidecar.find("\"dumping\": true") != std::string::npos,
          "Exactly the dumping thread must be marked.");

    // RTTI is MSVC x64 specific. A mingw build uses Itanium RTTI and legitimately finds none.
#ifdef _MSC_VER
    {
        const size_t at = sidecar.find("\"name\": \"RsfDumpProbeDerived\"");
        check(at != std::string::npos, "The polymorphic class must be found by its RTTI.");
        if (at != std::string::npos) {
            const size_t begin = sidecar.rfind("{\"vtable_rva\"", at);
            const size_t end = sidecar.find("\"declared_bases\"", at);
            check(begin != std::string::npos && end != std::string::npos, "The class entry is malformed.");
            if (begin != std::string::npos && end != std::string::npos) {
                const std::string entry = sidecar.substr(begin, end - begin);
                check(entry.find("{\"vtable_rva\": \"" + hex(vtable_rva) + "\"") == 0,
                      "The class must carry the vtable RVA read from the live object.");
                check(entry.find("\"decorated\": \".?AURsfDumpProbeDerived@") != std::string::npos ||
                          entry.find("\"decorated\": \".?AVRsfDumpProbeDerived@") != std::string::npos,
                      "The decorated name must be recorded.");
                check(entry.find("\"offset\": 0, \"cd_offset\": 0") != std::string::npos,
                      "A single-inheritance class has zero offsets.");
                check(entry.find("\"name\": \"RsfDumpProbeBase\"") != std::string::npos,
                      "The base class must come from the hierarchy descriptor.");
            }
        }
        check(report.rtti_classes > 0, "The report must count recovered classes.");
    }
#else
    check(sidecar.find("\"rtti\": {") != std::string::npos, "The rtti section must exist.");
#endif
    check(sidecar.find("\"rtti\": {\"classes\": [") != std::string::npos &&
              sidecar.find("\"budget_ms\"") != std::string::npos,
          "The rtti section must report its budget and elapsed time.");

    // Function starts.
    {
        const size_t at = sidecar.find("\"functions\": {\"entries\": [");
        check(at != std::string::npos, "The functions section must exist.");
        if (at != std::string::npos) {
            const size_t count_at = sidecar.find("\"count\": ", at);
            check(count_at != std::string::npos &&
                      std::strtoul(sidecar.c_str() + count_at + 9, nullptr, 10) > 0,
                  "An x64 executable must list function starts.");
            check(sidecar.find("\n    [", at) != std::string::npos, "Function entries must be listed.");
        }
        check(report.function_starts > 0, "The report must count function starts.");
    }

    // Names.
    check(sidecar.find("\"rva\": \"" + hex(worker_rva) + "\", \"name\": \"Probe_Worker\"") !=
              std::string::npos,
          "The hex-addressed name must be merged.");
    check(sidecar.find("\"name\": \"Probe_Decimal \\\"quoted\\\"\"") != std::string::npos,
          "The decimal-addressed name must be merged and escaped.");
    check(sidecar.find("\"skipped_lines\": 1") != std::string::npos, "The malformed line must be counted.");

    // Section hashes against an independent implementation, over the bytes the dump holds.
    if (dump.size() > sizeof(IMAGE_DOS_HEADER)) {
        const unsigned char* bytes = reinterpret_cast<const unsigned char*>(dump.data());
        const IMAGE_NT_HEADERS64* headers = reinterpret_cast<const IMAGE_NT_HEADERS64*>(
            bytes + reinterpret_cast<const IMAGE_DOS_HEADER*>(bytes)->e_lfanew);
        const IMAGE_SECTION_HEADER* sections = IMAGE_FIRST_SECTION(headers);
        size_t cursor = sidecar.find("\"sections\": [");
        int hashed = 0;
        for (WORD index = 0; index < headers->FileHeader.NumberOfSections; ++index) {
            const DWORD size = sections[index].Misc.VirtualSize;
            if (size == 0) {
                continue;
            }
            cursor = sidecar.find("\"sha256\": \"", cursor);
            check(cursor != std::string::npos, "Every non-empty section must carry a hash.");
            if (cursor == std::string::npos) {
                break;
            }
            cursor += 11;
            const std::string recorded = sidecar.substr(cursor, 64);
            check(sections[index].VirtualAddress + static_cast<size_t>(size) <= dump.size(),
                  "The dump must cover the section.");
            check(recorded == sha256_hex(bytes + sections[index].VirtualAddress, size),
                  "The recorded section hash must match the section bytes in the dump.");
            ++hashed;
        }
        check(hashed > 0, "At least one section hash must be verified.");
        check(sidecar.find("\"entropy\": ", sidecar.find("\"sections\": [")) != std::string::npos,
              "Sections must carry sampled entropy.");
    }

    return passed ? 0 : 1;
}
