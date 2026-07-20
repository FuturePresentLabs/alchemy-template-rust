//! pedal-dsp — the Rust ⇆ C++ bridge.
//!
//! This crate cross-compiles to `libpedal_dsp.a` and exposes a tiny C ABI that
//! the C++ Alchemy SDK firmware links and drives:
//!
//! ```c
//! int32_t pk_init(float sample_rate, uint8_t* heap, size_t heap_len);
//! void    pk_process_block(const float* in, float* out, size_t n);
//! void    pk_set_control(const uint8_t* label, size_t label_len, float value);
//! ```
//!
//! Inside it runs a pedalkernel [`CompiledPedal`] — the `no_std` WDF audio
//! engine — reconstructed from a postcard blob that `build.rs` compiled from
//! `pedals/demo.pedal` at build time. The C++ side keeps the whole SDK (pages,
//! pot-catch, CV, presets, LEDs); this side is just the DSP.

#![no_std]

extern crate alloc;

use core::ptr::addr_of_mut;
use core::str;

use embedded_alloc::Heap;
use pedalkernel_rt::processor::CompiledPedal;
use pedalkernel_rt::PedalProcessor;

// `#[panic_handler]` for bare metal (halts). Linked for its side effect only.
use panic_halt as _;
// Pulls in cortex-m's `critical-section` impl that `embedded-alloc` requires.
use cortex_m as _;

/// Global allocator. `pedalkernel-rt` is `no_std + alloc`; the backing memory
/// region is handed to us at [`pk_init`] time so the C++ side can put it in
/// SDRAM (64 MB) rather than scarce internal SRAM.
#[global_allocator]
static HEAP: Heap = Heap::empty();

/// The compiled pedal, serialized by `build.rs` and baked into the image.
static PEDAL_BLOB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/demo.postcard"));

/// The live processor.
///
/// `pk_process_block` runs in the audio interrupt; `pk_set_control` runs in the
/// main control loop. That split mirrors the C++ template itself (DSP in the
/// callback, control updates in `main`): a control write only mutates existing
/// stage fields in place, and the allocator is critical-section protected, so a
/// preempted update costs at worst one glitched sample. This holds as long as
/// `process` stays allocation-free — which it must be for real-time audio.
static mut PROC: Option<CompiledPedal> = None;

#[inline]
fn proc_mut() -> Option<&'static mut CompiledPedal> {
    // SAFETY: see `PROC` — bare-metal single-core; the documented race between
    // the audio IRQ and the control loop is benign (in-place field writes).
    unsafe { (*addr_of_mut!(PROC)).as_mut() }
}

/// Initialize the allocator, deserialize the baked pedal, and prime it for
/// `sample_rate` Hz.
///
/// `heap`/`heap_len` describe a caller-owned memory region (put it in SDRAM).
/// Call exactly once, after the hardware and SDRAM are initialized and before
/// audio starts. Returns 0 on success, negative on error.
///
/// # Safety
/// `heap` must point to `heap_len` bytes of valid, exclusively-owned,
/// writable memory that outlives all later calls.
#[no_mangle]
pub unsafe extern "C" fn pk_init(sample_rate: f32, heap: *mut u8, heap_len: usize) -> i32 {
    if heap.is_null() || heap_len == 0 {
        return -1;
    }
    // SAFETY: single init before audio starts; caller guarantees the region.
    unsafe { HEAP.init(heap as usize, heap_len) };

    match postcard::from_bytes::<CompiledPedal>(PEDAL_BLOB) {
        Ok(mut proc) => {
            proc.set_sample_rate(sample_rate);
            proc.reset();
            // SAFETY: runs before audio; no concurrent access yet.
            unsafe { PROC = Some(proc) };
            0
        }
        Err(_) => -2,
    }
}

/// Process one mono block, `input` → `output`, `n` frames. Runs in the audio
/// IRQ. Passes through unchanged until [`pk_init`] has succeeded.
///
/// # Safety
/// `input` and `output` must each point to `n` valid, non-overlapping `f32`s.
#[no_mangle]
pub unsafe extern "C" fn pk_process_block(input: *const f32, output: *mut f32, n: usize) {
    let Some(proc) = proc_mut() else {
        // SAFETY: caller contract — `n` valid f32s at both pointers.
        unsafe { core::ptr::copy_nonoverlapping(input, output, n) };
        return;
    };
    for i in 0..n {
        // SAFETY: `i < n`, buffers are valid and non-overlapping.
        unsafe { *output.add(i) = proc.process(*input.add(i)) };
    }
}

/// Set a normalized control (`0.0..=1.0`) by the label it was given in the
/// `.pedal`'s `controls { }` block (e.g. `"Distortion"`, `"Filter"`,
/// `"Volume"`). Called from the main control loop. No-op before [`pk_init`] or
/// for an unknown label.
///
/// # Safety
/// `label` must point to `label_len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn pk_set_control(label: *const u8, label_len: usize, value: f32) {
    let Some(proc) = proc_mut() else { return };
    // SAFETY: caller contract — `label_len` readable bytes at `label`.
    let bytes = unsafe { core::slice::from_raw_parts(label, label_len) };
    if let Ok(name) = str::from_utf8(bytes) {
        proc.set_control_immediate(name, value);
    }
}
