// Generates the cxx bridge headers consumed by shell/bridge/. [ADR-004]
// Two-reviewer rule applies to all changes in this crate. [ADR-027]
fn main() {
    cxx_build::bridge("src/lib.rs")
        .flag_if_supported("-std=c++20")
        .compile("ffi-bridge");
    println!("cargo:rerun-if-changed=src/lib.rs");
}
