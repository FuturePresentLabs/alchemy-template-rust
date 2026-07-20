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
**ProCo RAT** as the demo pedal (Distortion / Filter / Volume). Clone it, build
it, flash it, then drop in your own `.pedal`.

## How it fits together

```
┌──────────────────────── firmware (Cortex-M7) ────────────────────────┐
│                                                                       │
│  C++  (Alchemy SDK)                     Rust  (pedal-dsp, no_std)      │
│  ┌───────────────────┐                  ┌──────────────────────────┐  │
│  │ pages, pot-catch, │  pk_set_control  │ pedalkernel CompiledPedal│  │
│  │ param-lock, CV,   │ ───────────────► │  (WDF audio engine)      │  │
│  │ presets, settings │                  │                          │  │
│  │ LED rings         │ pk_process_block │  reconstructed from a     │  │
│  │ audio callback    │ ◄──────────────► │  postcard blob baked in   │  │
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
git clone --recurse-submodules https://github.com/ajmwagar/alchemy-template-rust.git my-pedal
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
   point `PK_PEDAL` at another file). Then update the three knobs in
   [`src/pedal.cpp`](src/pedal.cpp) — the label strings passed to
   `pk::SetControl("…")` **must match** your `.pedal`'s `controls { }` block.

2. **Tune quality vs. CPU.** The demo builds at 1× oversampling with runtime
   Newton-Raphson (small image, mono-friendly). Trade image size / CPU for
   fidelity via the build tunables (Makefile vars or env, see
   [`dsp/build.rs`](dsp/build.rs)):

   ```sh
   make PK_OVERSAMPLING=4                            # less aliasing on hard clipping
   cd dsp && PK_K_TABLES=1 cargo build --release     # bake NR lookup tables (bigger, faster)
   ```

   Real-time headroom on the M7 depends on the circuit (nonlinear roots are the
   cost). If audio glitches, drop oversampling or simplify the pedal.

3. **The bridge.** Three C functions ([`src/pedalkernel_bridge.h`](src/pedalkernel_bridge.h)):

   ```c
   int32_t pk_init(float sample_rate, uint8_t* heap, size_t heap_len);
   void    pk_process_block(const float* in, float* out, size_t n);
   void    pk_set_control(const uint8_t* label, size_t label_len, float value);
   ```

   pedalkernel is `no_std + alloc`; `pk_init` takes a heap region — the template
   hands it 4 MB of SDRAM (costs nothing in the flashed image). Control writes
   run in the main loop and audio in the callback, mirroring the SDK's own split.

4. **Stereo.** The demo runs one mono processor and mirrors it to both outputs.
   For true stereo, instantiate two processors on the Rust side (one per channel).

### A note on pedalkernel's embedded (f32) build

pedalkernel runs `f64` on desktop and `f32` on the Cortex-M7. The `f32` path is
what this template compiles; it needs the small set of `f64 → crate::Wave`
fixes in the nonlinear device models that ship on pedalkernel `main`. If you
pin an older pedalkernel commit and the `dsp/` build fails with `f32`/`f64`
type errors, bump the submodule.

### Updating the vendored libraries

```sh
git -C lib/pedalkernel pull origin main
git add lib/pedalkernel && git commit -m "Bump pedalkernel"
```

The pinned libDaisy commit matches the one the Alchemy SDK vendors and tests
against; if you bump one, consider bumping the other to match.

## Licensing

The template scaffolding is MIT (see `LICENSE`), **but pedalkernel is
AGPL-3.0-or-later**. Linking `libpedal_dsp.a` into the firmware makes the
resulting binary a derivative work of pedalkernel — so a distributed build is
subject to the AGPL. Keep that in mind before shipping.
