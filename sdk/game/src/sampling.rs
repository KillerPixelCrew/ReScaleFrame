// SPDX-License-Identifier: MIT
// Both precisions preserve the arithmetic used by their projection convention.
macro_rules! halton {
    ($name:ident, $float:ty) => {
        pub fn $name(mut index: u32, base: u32) -> $float {
            if base < 2 {
                return 0.0;
            }
            let mut value = 0.0;
            let mut fraction = 1.0;
            while index != 0 {
                fraction /= base as $float;
                value += fraction * (index % base) as $float;
                index /= base;
            }
            value
        }
    };
}

halton!(halton_f32, f32);
halton!(halton_f64, f64);
