// SPDX-License-Identifier: MIT
#[cfg(windows)]
mod temporal_shader_device;

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    match temporal_shader_device::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    eprintln!("The temporal shader device fixture requires Windows D3D11.");
    std::process::ExitCode::FAILURE
}
