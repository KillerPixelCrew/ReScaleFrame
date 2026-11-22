/* SPDX-License-Identifier: MIT */
extern int RSF_CAPTURE_FIXTURE(const char* directory);

int main(int argc, char** argv)
{
    return argc == 2 ? RSF_CAPTURE_FIXTURE(argv[1]) : 2;
}
