// SPDX-License-Identifier: MIT
#![cfg(all(windows, not(test)))]

mod abi;
mod assembly;
mod choice;
mod device;
mod dll;
mod dll_abi;
mod latency;
mod latency_abi;
mod leases;
mod policy;
mod provider;
mod session;
mod session_abi;
mod switching;
