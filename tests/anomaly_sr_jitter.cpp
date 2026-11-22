/* SPDX-License-Identifier: MIT */
extern "C" int rsf_anomaly_sr_jitter_fixture(const char*) noexcept;

int main(int argc, char** argv) {
    return rsf_anomaly_sr_jitter_fixture(argc > 1 ? argv[1] : nullptr);
}
