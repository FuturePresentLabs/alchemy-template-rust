//! Build step: compile a `.pedal` circuit into a `CompiledPedal` and serialize
//! it to a postcard blob that `src/lib.rs` bakes into the firmware with
//! `include_bytes!`. The heavy std-only compiler (`pedalkernel`) runs here, on
//! the host, at build time; the device only ever *deserializes* the result.
//!
//! Tunable without editing this file (set in the environment / Makefile):
//!   PK_PEDAL         path to the .pedal source      (default: pedals/demo.pedal)
//!   PK_SAMPLE_RATE   Hz, must match the Daisy audio (default: 48000)
//!   PK_OVERSAMPLING  1 | 2 | 4                      (default: 1)
//!   PK_K_TABLES      0 | 1  bake NR lookup tables?  (default: 0)
//!
//! Notes on the defaults (a template that has to *fit and run* out of the box):
//!   * Oversampling 1x is the lightest on the M7. Bump to 2/4 for less aliasing
//!     on hard-clipping circuits if you have the CPU headroom.
//!   * K-tables off keeps the baked blob small (tables are large 2-D sweeps) and
//!     the app image inside SRAM; nonlinear roots fall back to runtime
//!     Newton-Raphson. Turn on (PK_K_TABLES=1) to trade image size for lower
//!     per-sample CPU if your circuit fits.

use std::path::PathBuf;

use pedalkernel::compiler::{compile_pedal_cached, CompileOptions};
use pedalkernel::oversampling::OversamplingFactor;

fn env_or(key: &str, default: &str) -> String {
    println!("cargo:rerun-if-env-changed={key}");
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    let pedal_rel = env_or("PK_PEDAL", "pedals/demo.pedal");
    let pedal_path = manifest.join(&pedal_rel);
    println!("cargo:rerun-if-changed={}", pedal_path.display());
    println!("cargo:rerun-if-changed=build.rs");

    let sample_rate: f64 = env_or("PK_SAMPLE_RATE", "48000")
        .parse()
        .expect("PK_SAMPLE_RATE must be a number");

    let oversampling = match env_or("PK_OVERSAMPLING", "1").as_str() {
        "1" => OversamplingFactor::X1,
        "2" => OversamplingFactor::X2,
        "4" => OversamplingFactor::X4,
        other => panic!("PK_OVERSAMPLING must be 1, 2, or 4 (got {other})"),
    };

    let skip_k_tables = env_or("PK_K_TABLES", "0") != "1";

    let source = std::fs::read_to_string(&pedal_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", pedal_path.display()));

    let options = CompileOptions {
        oversampling,
        skip_k_tables,
        ..CompileOptions::default()
    };

    // Writes `<out_dir>/demo.postcard` (and a `.hash` for its own caching).
    // `src/lib.rs` includes exactly that filename, so keep the stem "demo".
    let blob = compile_pedal_cached(
        &source,
        "demo",
        "demo",
        sample_rate,
        &options,
        &out_dir,
    )
    .unwrap_or_else(|e| panic!("compiling {} failed: {e}", pedal_path.display()));

    println!(
        "cargo:warning=pedal-dsp: compiled {} → {} bytes (sr={sample_rate}, os={}x, k_tables={})",
        pedal_rel,
        blob.len(),
        oversampling as usize,
        !skip_k_tables,
    );
}
