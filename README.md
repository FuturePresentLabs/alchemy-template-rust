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

   The Rust lib is built `opt-level = "z"` (size). The BOOT_SRAM app runs
   entirely from 480 KB of SRAM and pedalkernel-rt's WDF engine is large — every
   device model is reachable via deserialization, so the linker can't drop the
   unused ones. The RAT demo lands at ~409 KB (SRAM 83%). Switching the Rust
   profile to `opt-level = 3` is faster but overflowed SRAM here; do it only with
   a smaller circuit, and watch the `--print-memory-usage` output at link time.

3. **The bridge.** Three C functions ([`src/pedalkernel_bridge.h`](src/pedalkernel_bridge.h)):

   ```c
   int32_t pk_init(float sample_rate, uint8_t* heap, size_t heap_len);
   void    pk_process_block(const float* in, float* out, size_t n);
   size_t  pk_num_controls(void);
   size_t  pk_control_label(size_t idx, uint8_t* buf, size_t buf_len);
   void    pk_set_control_by_index(size_t idx, float value);
   void    pk_set_control(const uint8_t* label, size_t label_len, float value);
   ```

   pedalkernel is `no_std + alloc`; `pk_init` takes a heap region — the template
   hands it 4 MB of SDRAM (costs nothing in the flashed image). Control writes
   run in the main loop and audio in the callback, mirroring the SDK's own split.

4. **Stereo.** The demo runs one mono processor and mirrors it to both outputs.
   For true stereo, instantiate two processors on the Rust side (one per channel).

### Updating the vendored libraries

```sh
git -C lib/pedalkernel pull origin main
git add lib/pedalkernel && git commit -m "Bump pedalkernel"
```

The pinned libDaisy commit matches the one the Alchemy SDK vendors and tests
against; if you bump one, consider bumping the other to match.

## Licensing

The template scaffolding in this repo is MIT (see `LICENSE`).

The DSP it links — **pedalkernel — is AGPLv3** (`AGPL-3.0-or-later`). Linking
`libpedal_dsp.a` into the firmware makes the resulting binary a combined work
governed by the AGPL: if you distribute the firmware (or a device running it),
you must make the complete corresponding source available to recipients under
the AGPL, including your `.pedal` circuits and any modifications.

Two things matter before you ship a product:

- **Hardware incorporation needs a commercial license.** pedalkernel's LICENSE
  adds a Section 7 condition — incorporating the kernel/runtime into a
  *qualifying hardware product* requires a separate commercial license from
  **Future Present Labs LLC**, on top of (or instead of) the AGPL. Selling an
  Alchemy Lab loaded with this firmware is exactly that case.
- **A commercial license is available.** pedalkernel is dual-licensed. A
  closed-source commercial license — kernel + runtime, hardware product rights,
  prebuilt host bindings, and support — is offered through **Puget Audio**;
  contact **info@puget.audio** for terms and pricing. It lifts the AGPL's
  copyleft and source-disclosure obligations for your product.

The authoritative terms are pedalkernel's
[LICENSE](https://github.com/ajmwagar/pedalkernel/blob/main/LICENSE) and its
[commercial licensing](https://github.com/ajmwagar/pedalkernel#commercial-licenses-and-bindings)
section — this summary is just a pointer.
