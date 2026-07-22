# alchemy-template-rust

A [Hermetic Modular Alchemy Lab](https://hermeticmodular.com/modules/alchemy-lab)
firmware template whose **DSP is a [pedalkernel](https://github.com/ajmwagar/pedalkernel)
WDF circuit written in Rust**, running on top of the full C++
[Alchemy SDK](https://github.com/hermetic-modular/alchemy-sdk).

It's the [`alchemy-template`](https://github.com/hermetic-modular/alchemy-template)
fork for people who want to author effects as `.pedal` netlists (op-amps,
diodes, tubes, transistors — real circuits, solved with Wave Digital Filters)
instead of hand-writing DSP. The C++ side keeps everything the SDK gives you —
pages, pot-catch, param-lock automation, CV routing, presets, settings, LED
rings — and the audio callback simply calls into pedalkernel. Ships with the
**ProCo RAT** as the demo pedal (Distortion / Filter / Volume), running in true
stereo (an independent instance per channel). Clone it, build it, flash it, then
drop in your own `.pedal`.

## How it fits together

```
┌──────────────────────── firmware (Cortex-M7) ────────────────────────┐
│                                                                       │
│  C++  (Alchemy SDK)                     Rust  (pedal-dsp, no_std)      │
│  ┌───────────────────┐                  ┌──────────────────────────┐  │
│  │ pages, pot-catch, │  pk_set_control  │ pedalkernel CompiledPedal│  │
│  │ param-lock, CV,   │ ───────────────► │  (WDF audio engine)      │  │
│  │ presets, settings │   process_block  │  1 instance per channel   │  │
│  │ LED rings         │ ◄──────────────► │  (stereo), from a         │  │
│  │ audio callback    │      _stereo      │  postcard blob baked in   │  │
│  └───────────────────┘                  │  by build.rs             │  │
│         main()                          └──────────────────────────┘  │
│                                            libpedal_dsp.a (linked)     │
└───────────────────────────────────────────────────────────────────────┘

   build time (host):  dsp/pedals/demo.pedal ──[pedalkernel compiler]──►
                       demo.postcard  ──include_bytes!──►  the firmware
```

The heavy compiler (DSL parse → WDF → serialized processor) runs **once at
build time on your machine**. The device only ever *deserializes* the result
and runs it per sample.

`build.rs` builds the compiler with pedalkernel's **`wave-f32`** feature so the
blob's scalars are serialized as `f32` — matching the device's `Wave`. postcard
isn't self-describing, so without this the f64 host blob would fail to
deserialize on the M7 (and the firmware would never start). Needs a pedalkernel
that has `wave-f32` ([ajmwagar/pedalkernel#221](https://github.com/ajmwagar/pedalkernel/pull/221)).

## What's inside

```
├── Makefile                 builds the Rust lib + firmware, links them
├── src/                     the C++ firmware — hardware, SDK surface, audio cb
│   ├── pedal.cpp                knobs, pages, CV, presets → pk_* bridge calls
│   ├── pedalkernel_bridge.h     the C ABI exported by the Rust lib
│   └── pedal_palette.h          LED ring colors
├── dsp/                     the Rust half → libpedal_dsp.a
│   ├── Cargo.toml               staticlib; deps on pedalkernel-rt (no_std)
│   ├── build.rs                 compiles a .pedal → postcard blob at build time
│   ├── src/lib.rs               the bridge: pk_init / pk_process_block / pk_set_control
│   ├── pedals/demo.pedal        the circuit (edit this / bring your own)
│   └── .cargo/config.toml       targets thumbv7em-none-eabihf, cortex-m7
└── lib/
    ├── alchemy-sdk/         Alchemy framework + board support   (submodule)
    ├── libDaisy/            Electrosmith Daisy library          (submodule)
    └── pedalkernel/         Rust WDF kernel                     (submodule)
```

## Requirements

- `git`, `make`
- `arm-none-eabi-gcc` **with newlib** — use the ARM cask, not the bare compiler
- a Rust toolchain (`rustup`) with the ARM bare-metal target
- `dfu-util` (to flash)

macOS (Homebrew):

```sh
brew install git make dfu-util
brew install --cask gcc-arm-embedded      # ships newlib; the `arm-none-eabi-gcc` formula does NOT
rustup target add thumbv7em-none-eabihf
```

Ubuntu / Debian:

```sh
sudo apt install git make gcc-arm-none-eabi libnewlib-arm-none-eabi dfu-util
rustup target add thumbv7em-none-eabihf
```

## Getting started

```sh
git clone --recurse-submodules git@github.com:FuturePresentLabs/alchemy-template-rust.git my-pedal
cd my-pedal

make libdaisy    # build libDaisy once after cloning
make             # build the Rust DSP lib + firmware → build/pedal.bin
```

`make` builds `dsp/` (cross-compiles pedalkernel + compiles `demo.pedal` into a
blob) and then the C++ firmware, and links them.

## Flashing

The Alchemy Lab runs a custom bootloader (`DaisyBootloader-AlchemyLabV2`) that
serves DFU over the front-panel USB-C port. Connect that port, then put the
module in update mode: during the ~2 s window after power-on — the LED rings
spin a warm-white comet — press or hold **B3.** The rings switch to a slow
breathe. Then:

```sh
make program-dfu
```

You can also use the [Hermetic Modular Web Programmer](https://hermeticmodular.com/program).

## Make it yours

1. **Swap the pedal.** Replace `dsp/pedals/demo.pedal` with your circuit (or
   point `PK_PEDAL` at another file), rebuild, and flash. **No firmware edits
   needed** — the control wiring is dynamic: at boot the firmware queries
   `pk_num_controls()` and binds one pot per control, in declared order, naming
   each knob from `pk_control_label()` and driving it with
   `pk_set_control_by_index()`. (A pedal with more than the six physical pots
   gets its first six on knobs; the rest keep their compiled defaults.)

2. **Tune quality vs. CPU vs. image size.** The demo builds at 1× oversampling
   with runtime Newton-Raphson — small image (SRAM ~83%), but the NR solve is
   iterative, so two channels of a hard nonlinear circuit is the tight case.
   Baking **K-tables** swaps the per-sample solve for a lookup (cheap,
   constant-time — the comfortable choice for stereo) at the cost of a bigger
   baked blob. Tunables are Makefile vars (forwarded to
   [`dsp/build.rs`](dsp/build.rs)):

   ```sh
   make PK_OVERSAMPLING=4      # less aliasing on hard clipping (multiplies CPU)
   make PK_K_TABLES=1          # bake NR lookup tables — faster/smoother per sample
   ```

   For the RAT the K-table blob is ~68 KB. It's `include_bytes!`'d into the
   image, so it lands in the 480 KB SRAM app: the build goes from ~83% to ~96%
   SRAM. Both channels **share that one blob** (deserialized into independent
   state on the SDRAM heap), so stereo does not double it — but ~96% leaves
   little room for a bigger circuit or higher oversampling. If audio glitches,
   drop oversampling or simplify the pedal.

   **Need headroom?** The blob doesn't have to live in SRAM. With 16 MB of QSPI
   flash you can place `PEDAL_BLOB` in a QSPI-mapped (XIP) section instead — the
   deserialize reads it once at init and SRAM drops back to ~83%. That needs a
   custom linker section for the blob plus a separate step to program it to QSPI
   (it isn't part of the SRAM `.bin` that `make program-dfu` writes), so it's a
   hardware-validated add-on rather than a flag — open an issue if you want it.

   (The Rust lib is built `opt-level = "z"` for size — pedalkernel-rt's WDF
   engine is large and every device model is reachable via deserialization, so
   the linker can't drop unused ones. `opt-level = 3` is faster but overflowed
   SRAM here; use it only with a smaller circuit.)

3. **The bridge.** Three C functions ([`src/pedalkernel_bridge.h`](src/pedalkernel_bridge.h)):

   ```c
   int32_t pk_init(float sample_rate, uint8_t* heap, size_t heap_len);
   void    pk_process_block_stereo(const float* inL, const float* inR,
                                   float* outL, float* outR, size_t n);
   void    pk_process_block(const float* in, float* out, size_t n);   // mono, ch 0
   size_t  pk_num_controls(void);
   size_t  pk_control_label(size_t idx, uint8_t* buf, size_t buf_len);
   void    pk_set_control_by_index(size_t idx, float value);          // all channels
   void    pk_set_control(const uint8_t* label, size_t label_len, float value);
   ```

   pedalkernel is `no_std + alloc`; `pk_init` takes a heap region — the template
   hands it 8 MB of SDRAM (costs nothing in the flashed image). Control writes
   run in the main loop and audio in the callback, mirroring the SDK's own split.

4. **Stereo (and beyond).** Left and right run through independent instances of
   the same pedal: the audio callback calls `pk_process_block_stereo()`, and a
   control write updates every channel. The count is `NUM_CHANNELS` in
   [`dsp/src/lib.rs`](dsp/src/lib.rs) — the instances share the one baked blob
   but keep their own audio state on the heap, so raising it costs SDRAM, not
   flash.

### Updating the vendored libraries

```sh
git -C lib/pedalkernel pull origin main
git add lib/pedalkernel && git commit -m "Bump pedalkernel"
```

The pinned libDaisy commit matches the one the Alchemy SDK vendors and tests
against; if you bump one, consider bumping the other to match.

## Licensing

The template scaffolding in this repo is MIT (see `LICENSE`).

The DSP it links — **pedalkernel — is AGPLv3** (`AGPL-3.0-or-later`), and this
project is meant to support the community. If you're a hobbyist, tinkerer,
researcher, or small maker, you're free to use, modify, build, and share it —
just keep it under the AGPL, which means anything you distribute stays open:
publish the complete corresponding source (your `.pedal` circuits and any
changes) to whoever you distribute to.

The one commercial line: pedalkernel's LICENSE adds a Section 7 condition —
incorporating the kernel/runtime into hardware products *by any entity with
annual revenue exceeding $1M USD* (or one majority-owned by such an entity)
requires a separate commercial license from **Future Present Labs LLC**. That's
the only case that steps outside the AGPL; everyone under that threshold is
covered by it.

For that case, pedalkernel is dual-licensed — a closed-source commercial
license (kernel + runtime, hardware rights, prebuilt host bindings, and support)
is available from **Future Present Labs**; contact **info@fpl.dev**.

The authoritative terms are pedalkernel's
[LICENSE](https://github.com/ajmwagar/pedalkernel/blob/main/LICENSE) — this
summary is just a pointer.
