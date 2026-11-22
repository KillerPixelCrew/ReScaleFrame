// SPDX-License-Identifier: MIT
#![cfg(all(windows, not(test)))]

mod abi;

mod assembly;

mod choice;

mod leases;

mod dll;
mod dll_abi;
mod policy;
mod session;
mod session_abi;
