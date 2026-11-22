// SPDX-License-Identifier: MIT
use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=entries.h");
    let header = include_str!("entries.h");
    let (_, declaration) = header
        .split_once("struct rsf_vk_ngx_entries {")
        .expect("NGX entry table declaration");
    let (fields, _) = declaration.split_once("};").expect("NGX entry table end");
    let mut bindings = String::from("#[repr(C)]\nstruct NgxEntries {\n");
    for field in fields
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let name = field
            .strip_prefix("void *")
            .and_then(|field| field.strip_suffix(';'))
            .expect("NGX table fields must be opaque pointers");
        bindings.push_str(&format!("    {name}: *mut c_void,\n"));
    }
    bindings.push_str("}\n");
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
    fs::write(output.join("entries.rs"), bindings).expect("write NGX entry bindings");
}
