//! pedal-dsp — the Rust ⇆ C++ bridge.
//!
//! This crate cross-compiles to `libpedal_dsp.a` and exposes a tiny C ABI that
//! the C++ Alchemy SDK firmware links and drives:
//!
//! ```c
//! int32_t pk_init(float sample_rate, uint8_t* heap, size_t heap_len);
//! void    pk_process_block_stereo(const float* inL, const float* inR,
//!                                 float* outL, float* outR, size_t n);
//! void    pk_set_control_by_index(size_t idx, float value);
//! ```
//!
//! Inside it runs one pedalkernel [`CompiledPedal`] per channel — the `no_std`
//! WDF audio engine — reconstructed from a postcard blob that `build.rs`
//! compiled from `pedals/demo.pedal` at build time. Stereo is two instances of
//! the same circuit: they share the one baked blob (deserialized once each) but
//! keep independent audio state. The C++ side keeps the whole SDK (pages,
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

/// Independent processor instances (audio channels). The demo runs the same
/// pedal on left and right; bump this and the extra channels come for free
/// (each deserializes the same blob into its own state — code is shared, state
/// lives on the SDRAM heap).
const NUM_CHANNELS: usize = 2;

/// The live processors, one per channel.
///
/// `pk_process_block*` runs in the audio interrupt; `pk_set_control*` runs in
/// the main control loop. That split mirrors the C++ template itself (DSP in the
/// callback, control updates in `main`): a control write only mutates existing
/// stage fields in place, and the allocator is critical-section protected, so a
/// preempted update costs at worst one glitched sample. This holds as long as
/// `process` stays allocation-free — which it must be for real-time audio.
static mut PROC: [Option<CompiledPedal>; NUM_CHANNELS] = [const { None }; NUM_CHANNELS];

#[inline]
fn procs() -> &'static mut [Option<CompiledPedal>; NUM_CHANNELS] {
    // SAFETY: see `PROC` — bare-metal single-core; the documented race between
    // the audio IRQ and the control loop is benign (in-place field writes).
    unsafe { &mut *addr_of_mut!(PROC) }
}

#[inline]
fn chan_mut(ch: usize) -> Option<&'static mut CompiledPedal> {
    procs().get_mut(ch).and_then(|c| c.as_mut())
}

/// Process one channel's block in place: `input` → `output`, `n` frames.
/// Passes through unchanged if that channel isn't initialized.
///
/// # Safety
/// `input`/`output` must each point to `n` valid, non-overlapping `f32`s.
#[inline]
unsafe fn process_one(ch: usize, input: *const f32, output: *mut f32, n: usize) {
    let Some(proc) = chan_mut(ch) else {
        // SAFETY: caller contract — `n` valid f32s at both pointers.
        unsafe { core::ptr::copy_nonoverlapping(input, output, n) };
        return;
    };
    for i in 0..n {
        // SAFETY: `i < n`, buffers are valid and non-overlapping.
        unsafe { *output.add(i) = proc.process(*input.add(i)) };
    }
}

/// Initialize the allocator, deserialize one pedal per channel, and prime them
/// for `sample_rate` Hz.
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

    // One processor per channel from the same (read-only) baked blob.
    for ch in 0..NUM_CHANNELS {
        match postcard::from_bytes::<CompiledPedal>(PEDAL_BLOB) {
            Ok(mut proc) => {
                proc.set_sample_rate(sample_rate);
                proc.reset();
                // SAFETY: runs before audio; no concurrent access yet.
                procs()[ch] = Some(proc);
            }
            Err(_) => return -2,
        }
    }
    0
}

/// Process a stereo block: left through channel 0, right through channel 1.
/// Runs in the audio IRQ. Each channel passes through until [`pk_init`] succeeds.
///
/// # Safety
/// Each pointer must reference `n` valid `f32`s; inputs/outputs non-overlapping.
#[no_mangle]
pub unsafe extern "C" fn pk_process_block_stereo(
    in_l: *const f32,
    in_r: *const f32,
    out_l: *mut f32,
    out_r: *mut f32,
    n: usize,
) {
    // SAFETY: caller contract on the four buffers; the two channels are
    // processed sequentially so their `&mut` borrows never overlap.
    unsafe {
        process_one(0, in_l, out_l, n);
        process_one(1, in_r, out_r, n);
    }
}

/// Process one mono block through channel 0. Runs in the audio IRQ.
///
/// # Safety
/// `input`/`output` must each point to `n` valid, non-overlapping `f32`s.
#[no_mangle]
pub unsafe extern "C" fn pk_process_block(input: *const f32, output: *mut f32, n: usize) {
    // SAFETY: caller contract — `n` valid f32s at both pointers.
    unsafe { process_one(0, input, output, n) };
}

/// Set a normalized control (`0.0..=1.0`) by the label it was given in the
/// `.pedal`'s `controls { }` block, on every channel. Called from the main
/// control loop. No-op before [`pk_init`] or for an unknown label.
///
/// # Safety
/// `label` must point to `label_len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn pk_set_control(label: *const u8, label_len: usize, value: f32) {
    // SAFETY: caller contract — `label_len` readable bytes at `label`.
    let bytes = unsafe { core::slice::from_raw_parts(label, label_len) };
    if let Ok(name) = str::from_utf8(bytes) {
        for proc in procs().iter_mut().flatten() {
            proc.set_control_immediate(name, value);
        }
    }
}

// ── Control introspection ────────────────────────────────────────────────────
// The C++ side uses these to discover the loaded pedal's controls at boot and
// wire knobs to them by index — so swapping `.pedal` needs no firmware edits.
// The channels all run the same circuit, so channel 0 answers for the set.

/// Number of controls the loaded pedal exposes (the `.pedal`'s `controls { }`
/// block), in a stable order. 0 before [`pk_init`].
#[no_mangle]
pub extern "C" fn pk_num_controls() -> usize {
    chan_mut(0).map_or(0, |p| p.controls.len())
}

/// Copy control `idx`'s label into `buf` (up to `buf_len` bytes; **not**
/// NUL-terminated — the caller adds the terminator). Returns the label's full
/// byte length regardless of truncation (like `snprintf`); 0 if `idx` is out of
/// range or before [`pk_init`].
///
/// # Safety
/// `buf` must point to `buf_len` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn pk_control_label(idx: usize, buf: *mut u8, buf_len: usize) -> usize {
    let Some(proc) = chan_mut(0) else { return 0 };
    let Some(ctl) = proc.controls.get(idx) else { return 0 };
    let bytes = ctl.label.as_bytes();
    let n = bytes.len().min(buf_len);
    if !buf.is_null() && n > 0 {
        // SAFETY: `buf` has `buf_len >= n` writable bytes; source is disjoint.
        unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, n) };
    }
    bytes.len()
}

/// Set control `idx` (as ordered by [`pk_num_controls`]) to a normalized
/// `0.0..=1.0` value on every channel. Cheaper than the label form (no string
/// match) and smoothed by the processor. Called from the main control loop;
/// no-op for an out-of-range index or before [`pk_init`].
#[no_mangle]
pub extern "C" fn pk_set_control_by_index(idx: usize, value: f32) {
    for proc in procs().iter_mut().flatten() {
        proc.set_control_indexed(idx, value);
    }
}
