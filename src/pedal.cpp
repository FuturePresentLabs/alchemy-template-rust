/**
 * pedal.cpp — Alchemy Lab firmware that runs a pedalkernel WDF circuit.
 *
 * The whole Alchemy SDK surface is here (a control page, pot-catch, param-lock
 * automation, per-knob CV, presets, settings, LED rings). The DSP is the Rust
 * `pedal-dsp` static library, which runs a pedalkernel `CompiledPedal` compiled
 * from `dsp/pedals/demo.pedal` (the ProCo RAT, by default) at build time.
 *
 * The control wiring is DYNAMIC: at boot we ask the Rust side how many controls
 * the loaded pedal exposes (pk_num_controls), read their labels
 * (pk_control_label), and bind one physical pot per control, in order. Swap the
 * `.pedal` and the knobs follow — no edits here (up to kNumPots controls).
 *
 * Control flow:
 *   - main()            wires hardware, hands Rust its heap, self-wires knobs.
 *   - AudioCallback()   audio IRQ → pk_process_block() (mono, mirrored to L/R).
 *   - UpdateControls()  control loop → pk_set_control_by_index() per knob.
 */

#include "daisy_seed.h"
#include "alchemy/hw/alchemy_lab.h"
#include "alchemy/surface/control_loop.h"
#include "alchemy/surface/cv_matrix.h"
#include "alchemy/surface/page.h"
#include "alchemy/surface/pager.h"
#include "alchemy/surface/param_lock.h"
#include "alchemy/surface/presets.h"
#include "alchemy/surface/settings.h"
#include "alchemy/surface/virtual_knob.h"

#include "pedalkernel_bridge.h"
#include "pedal_palette.h"

using namespace alchemy;

/* ── Rust heap ───────────────────────────────────────────────────────────────
 * pedalkernel is `no_std + alloc`; it needs an allocator. We hand it a slab of
 * SDRAM (64 MB on the Daisy) at pk_init(). `.sdram_bss` is uninitialized, so
 * this costs nothing in the flashed image. 4 MB is ample for one pedal. */
static constexpr size_t kPkHeapBytes = 4 * 1024 * 1024;
static uint8_t DSY_SDRAM_BSS pk_heap[kPkHeapBytes];

/* ── Controls (populated at boot from the loaded pedal) ──────────────────────
 * One physical pot per pedal control, in declared order, up to kNumPots. */
static VirtualKnob knobs[kNumPots];
static char        knob_names[kNumPots][24]; // persistent label storage for name_
static uint8_t     g_num_controls = 0;

static Page            main_page(0);
static AlchemyLab      hw;
static ControlLoop     loop(hw);
static Pager           pager(hw.buttons[0], 1, kNumPots);
static ParamLock<kNumPots> locks(hw.buttons[0], pager);
static Presets         presets(hw.seed.qspi);
static Settings        settings(hw, &pager);
static CvMatrix        cv_matrix(kNumCvInputs);

/* Push the summed CV+knob values into the pedal each control frame, by index. */
static void UpdateControls()
{
    for (uint8_t i = 0; i < g_num_controls; ++i)
        pk_set_control_by_index(i, knobs[i].Value());
}

/* Mono guitar pedal: run the left input through pedalkernel, mirror to both
 * outputs. For true stereo you'd run two processor instances — see README. */
static void AudioCallback(daisy::AudioHandle::InputBuffer  in,
                          daisy::AudioHandle::OutputBuffer out,
                          size_t                           size)
{
    pk_process_block(in[0], out[0], size);
    for (size_t i = 0; i < size; ++i)
        out[1][i] = out[0][i];
}

int main()
{
    hw.Init();

    /* Hand pedalkernel its SDRAM heap and load the baked pedal. On failure
     * (bad blob / OOM) we idle rather than start audio with no processor. */
    if (pk_init(hw.SampleRate(), pk_heap, sizeof(pk_heap)) != 0)
        for (;;) {}

    /* Discover the pedal's controls and bind one pot per control, in order.
     * A pedal with more than kNumPots controls gets its first kNumPots on
     * knobs; the rest keep their compiled defaults. */
    g_num_controls = static_cast<uint8_t>(pk_num_controls());
    if (g_num_controls > kNumPots)
        g_num_controls = kNumPots;

    for (uint8_t i = 0; i < g_num_controls; ++i)
    {
        pk::ControlLabel(i, knob_names[i], sizeof(knob_names[i]));
        knobs[i] = VirtualKnob(i, knob_names[i])
                       .Linear(0.f, 1.f)
                       .Ring(Level(kRingPalette[i % kRingPaletteLen], FillAnim::Pulse));
        main_page.Add(knobs[i]);
        cv_matrix.Jack(i).To(knobs[i]);
    }

    /* Opt into default settings gestures and preset management. */
    settings.UseBrightness();
    settings.UsePresets(presets);

    presets.Manage(pager);
    presets.Manage(locks);
    presets.Manage(settings);
    presets.Init();
    presets.BootLoad();

    UpdateControls();
    hw.StartAudio(AudioCallback);

    /* Canonical control-rate frame. */
    loop.Use(pager)
        .Use(locks)
        .Use(settings)
        .Use(cv_matrix)
        .Use(main_page)
        .OnFrame(UpdateControls);

    for (;;) loop.Tick();
}
