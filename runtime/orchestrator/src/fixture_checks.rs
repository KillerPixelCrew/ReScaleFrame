// SPDX-License-Identifier: MIT
//! The failure-counting checks of the FSR4 state fixtures. Each fixture defines `with`, which
//! reaches its thread-local `Calls` and its `failures` count.

macro_rules! check {
    ($calls:expr, $value:expr) => {
        if !$value {
            eprintln!("{}: {}", line!(), stringify!($value));
            $calls.failures += 1;
        }
    };
}

macro_rules! expect {
    ($value:expr) => {{
        let passed = $value;
        if !passed {
            eprintln!("{}: {}", line!(), stringify!($value));
            with(|c| c.failures += 1);
        }
    }};
}
