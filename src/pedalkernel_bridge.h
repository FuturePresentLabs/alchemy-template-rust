/**
 * pedalkernel_bridge.h — C ABI exported by the Rust `pedal-dsp` static library
 * (see dsp/src/lib.rs). The firmware links `libpedal_dsp.a` and drives a
 * pedalkernel WDF processor through these three calls.
 *
 * Concurrency: `pk_process_block` runs in the audio interrupt; `pk_set_control`
 * runs in the main control loop. That split mirrors the C++ DSP templates
 * (audio in the callback, coefficient updates in `main`) — a control write only
 * mutates existing processor fields in place, so a preempted update costs at
 * worst one glitched sample.
 */

#pragma once

#include <cstddef>
#include <cstdint>
#include <cstring>

extern "C" {

/**
 * Initialize the allocator + load the baked pedal, primed for `sample_rate` Hz.
 * `heap`/`heap_len` describe a caller-owned region (put it in SDRAM). Call once,
 * after hw.Init() (SDRAM ready) and before StartAudio. Returns 0 on success.
 */
int32_t pk_init(float sample_rate, uint8_t* heap, size_t heap_len);

/** Process one mono block, `in` → `out`, `n` frames. Call from the audio callback. */
void pk_process_block(const float* in, float* out, size_t n);

/** Set a normalized control (0..1) by its `.pedal` label. Call from the control loop. */
void pk_set_control(const uint8_t* label, size_t label_len, float value);

} // extern "C"

namespace pk {

/** Convenience wrapper: `pk::SetControl("Distortion", 0.5f)`. */
inline void SetControl(const char* label, float value)
{
    pk_set_control(reinterpret_cast<const uint8_t*>(label), std::strlen(label), value);
}

} // namespace pk
