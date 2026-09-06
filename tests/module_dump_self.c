/* Dump this test's own main module and verify the result, which exercises the whole reader,
   writer and import walker without a game and without any protected binary involved. */

#include <rescaleframe/module_dump.h>

#include <windows.h>

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int passed = 1;

static void check(int condition, const char* message)
{
    if (!condition) {
        fprintf(stderr, "%s\n", message);
        passed = 0;
    }
}

static char* read_file(const char* path, size_t* size)
{
    FILE* stream = fopen(path, "rb");
    if (!stream) {
        return NULL;
    }
    fseek(stream, 0, SEEK_END);
    const long length = ftell(stream);
    fseek(stream, 0, SEEK_SET);
    if (length <= 0) {
        fclose(stream);
        return NULL;
    }
    char* data = (char*)malloc((size_t)length + 1);
    if (!data) {
        fclose(stream);
        return NULL;
    }
    *size = fread(data, 1, (size_t)length, stream);
    data[*size] = '\0';
    fclose(stream);
    return data;
}

int main(int argc, char* argv[])
{
    if (argc != 2) {
        fprintf(stderr, "usage: module_dump_self <output directory>\n");
        return 2;
    }

    rsf_dump_options options;
    memset(&options, 0, sizeof(options));
    options.struct_size = sizeof(options);
    options.abi_version = RSF_MODULE_DUMP_ABI_VERSION;
    options.output_directory_utf8 = argv[1];
    options.output_name_utf8 = "self";
    options.entropy_threshold = 7.0;
    options.require_decrypted = 0;

    rsf_dump_report report;
    memset(&report, 0, sizeof(report));
    report.struct_size = sizeof(report);

    check(rsf_dump_module(NULL, &options, NULL) == RSF_DUMP_OK,
          "A dump with no report structure must succeed.");

    options.abi_version = RSF_MODULE_DUMP_ABI_VERSION + 1u;
    check(rsf_dump_module(NULL, &options, &report) == RSF_DUMP_ERROR_ABI_MISMATCH,
          "An incompatible ABI must be rejected.");
    options.abi_version = RSF_MODULE_DUMP_ABI_VERSION;

    options.struct_size = 0;
    check(rsf_dump_module(NULL, &options, &report) == RSF_DUMP_ERROR_INVALID_ARGUMENT,
          "A short options structure must be rejected.");
    options.struct_size = sizeof(options);

    check(rsf_dump_module(NULL, NULL, &report) == RSF_DUMP_ERROR_INVALID_ARGUMENT,
          "Missing options must be rejected.");

    const rsf_dump_result result = rsf_dump_module(NULL, &options, &report);
    check(result == RSF_DUMP_OK, "Dumping this process's own module must succeed.");
    if (result != RSF_DUMP_OK) {
        return 1;
    }

    /* An ordinary unprotected executable must measure well below the ciphertext threshold. */
    check(report.code_entropy > 0.0 && report.code_entropy < 7.0,
          "This test's own code section must not look like ciphertext.");
    check(report.sections_written > 0, "At least one section must be written.");
    check(report.modules_listed > 0, "The loaded module list must not be empty.");
    check(report.imports_described > 0, "This test imports functions, so imports must be listed.");
    check(report.load_base != 0, "The load base must be recorded.");
    /* An ordinary executable calls through its import table, so the reference scan must find
       some. This is the same check that distinguishes a decrypted capture from ciphertext. */
    check(report.iat_references > 0, "Calls through the import table must be found in code.");

    char dump_path[1024];
    char sidecar_path[1024];
    snprintf(dump_path, sizeof(dump_path), "%s\\self.dump", argv[1]);
    snprintf(sidecar_path, sizeof(sidecar_path), "%s\\self.dump.json", argv[1]);

    size_t dump_size = 0;
    char* dump = read_file(dump_path, &dump_size);
    check(dump != NULL, "The dump file must exist.");
    if (dump) {
        check(dump[0] == 'M' && dump[1] == 'Z', "The dump must start with a PE signature.");

        const IMAGE_DOS_HEADER* dos = (const IMAGE_DOS_HEADER*)dump;
        const IMAGE_NT_HEADERS64* headers =
            (const IMAGE_NT_HEADERS64*)(dump + dos->e_lfanew);
        check(headers->Signature == IMAGE_NT_SIGNATURE, "The dump must carry NT headers.");
        check(headers->FileHeader.NumberOfSections == report.sections_written,
              "Every section in the header must have been written.");

        /* The defining property of this dump format: file offset equals virtual address. */
        const IMAGE_SECTION_HEADER* sections = IMAGE_FIRST_SECTION(headers);
        int aligned = 1;
        for (WORD index = 0; index < headers->FileHeader.NumberOfSections; ++index) {
            if (sections[index].PointerToRawData != sections[index].VirtualAddress) {
                aligned = 0;
            }
        }
        check(aligned, "Each section's raw offset must equal its virtual address.");

        /* Compare against the live module, which is the thing the dump claims to represent. */
        const unsigned char* live = (const unsigned char*)GetModuleHandleW(NULL);
        const IMAGE_DOS_HEADER* live_dos = (const IMAGE_DOS_HEADER*)live;
        const IMAGE_NT_HEADERS64* live_headers =
            (const IMAGE_NT_HEADERS64*)(live + live_dos->e_lfanew);
        check(headers->OptionalHeader.AddressOfEntryPoint ==
                  live_headers->OptionalHeader.AddressOfEntryPoint,
              "The entry point must be preserved.");
        check(headers->OptionalHeader.SizeOfImage == live_headers->OptionalHeader.SizeOfImage,
              "The image size must be preserved.");

        check(headers->OptionalHeader.FileAlignment == headers->OptionalHeader.SectionAlignment,
              "File alignment must equal section alignment for a one to one layout.");
        check(headers->OptionalHeader.ImageBase == (ULONGLONG)(uintptr_t)live,
              "The recorded image base must be where the module is actually loaded.");
        check(headers->OptionalHeader.CheckSum == 0, "The stale checksum must be cleared.");
        check((headers->OptionalHeader.DllCharacteristics &
               IMAGE_DLLCHARACTERISTICS_DYNAMIC_BASE) == 0,
              "Dynamic base must be cleared so the recorded base is honoured.");
        check(headers->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_SECURITY]
                      .VirtualAddress == 0,
              "The certificate directory is a file offset and must be cleared.");

        /* The bytes at the entry point must match memory. This is what proves the mapping, as
           opposed to merely proving the headers describe one. */
        const DWORD entry = headers->OptionalHeader.AddressOfEntryPoint;
        check(entry + 64 < dump_size, "The dump must extend past its own entry point.");
        if (entry + 64 < dump_size) {
            check(memcmp(dump + entry, live + entry, 64) == 0,
                  "Bytes at the entry point must match the live module.");
        }
        free(dump);
    }

    size_t sidecar_size = 0;
    char* sidecar = read_file(sidecar_path, &sidecar_size);
    check(sidecar != NULL, "The side-car file must exist.");
    if (sidecar) {
        check(strstr(sidecar, "\"tool\": \"rescaleframe-module-dump\"") != NULL,
              "The side-car must identify the tool that wrote it.");
        check(strstr(sidecar, "\"load_base\"") != NULL, "The side-car must record the load base.");
        check(strstr(sidecar, "\"preferred_base\"") != NULL,
              "The side-car must record the preferred base.");
        check(strstr(sidecar, "\"code_entropy\"") != NULL,
              "The side-car must record the measured entropy.");
        /* The import walker is the part the Ghidra ingest depends on, so require a known entry
           rather than merely a non-empty list. */
        check(strstr(sidecar, "GetProcAddress") != NULL,
              "The import map must resolve a known KERNEL32 import.");
        check(strstr(sidecar, "\"resolves_into\"") != NULL,
              "At least one import must be attributed to a loaded module.");
        free(sidecar);
    }

    return passed ? 0 : 1;
}
