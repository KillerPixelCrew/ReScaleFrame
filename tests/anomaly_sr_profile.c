/* SPDX-License-Identifier: MIT */
/* Launcher for the Rust profile contract (fixture_profile/contract.rs), which reads the native, geometry, SR and
   motion records from their Rust owners. */
int rsf_test_anomaly_profile_contract(void);

int main(void) { return rsf_test_anomaly_profile_contract(); }