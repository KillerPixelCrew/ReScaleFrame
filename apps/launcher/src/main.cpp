#include <rescaleframe/version.h>

#include <iostream>
#include <string_view>

int main(int argc, char* argv[])
{
    if (argc == 2 && std::string_view(argv[1]) == "--version") {
        std::cout << "ReScaleFrame " << RSF_VERSION_STRING << '\n';
        return 0;
    }
    if (argc > 2 || (argc == 2 && std::string_view(argv[1]) != "--help")) {
        std::cerr << "Unknown option. Use --help.\n";
        return 2;
    }
    std::cout << "ReScaleFrame " << RSF_VERSION_STRING << "\n"
                 "A framework for adding upscaling and frame generation to games.\n\n"
                 "Initial scaffold. Game launching and injection are not implemented.\n"
                 "Options: --help, --version\n";
    return 0;
}
