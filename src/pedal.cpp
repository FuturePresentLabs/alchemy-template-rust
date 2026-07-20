/**
 * pedal.cpp — Alchemy Lab firmware that runs a pedalkernel WDF circuit.
 *
 * The whole Alchemy SDK surface is here (a control page, pot-catch, param-lock
 * automation, per-knob CV, presets, settings, LED rings) — exactly as in the
 * C++ template. The difference is the DSP: instead of a hand-written biquad,
 * the audio callback calls into the Rust `pedal-dsp` static library, which runs
 * a pedalkernel `CompiledPedal` compiled from `dsp/pedals/demo.pedal`
 * (the ProCo RAT, by default) at build time.
 *
 * Control flow:
 *   - main()            wires hardware + SDK surfaces, hands Rust its heap.
 *   - AudioCallback()   audio IRQ → pk_process_block() (mono, mirrored to L/R).
 *   - UpdateControls()  control loop → pk_set_control() per knob, by label.
 *
 * The three knobs map to the .pedal's `controls { }` labels: Distortion,
 * Filter, Volume. Swap the pedal (edit dsp/pedals/demo.pedal or set PK_PEDAL)
 * and update the knob names/labels below to match its controls.
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
 * this costs nothing in the flashed image. 4 MB is ample for one pedal; a demo
 * blob deserializes into a few KB. Bump it if you bake in K-tables or big
 * circuits. */
static constexpr size_t kPkHeapBytes = 4 * 1024 * 1024;
static uint8_t DSY_SDRAM_BSS pk_heap[kPkHeapBytes];

/* ── Controls ────────────────────────────────────────────────────────────────
 * Normalized 0..1 knobs; the Rust side maps position → circuit values per the
 * .pedal's `controls { }` ranges. Names here are just for the display/CV; the
 * *labels* passed to pk::SetControl() must match the .pedal exactly. */
static VirtualKnob k_distortion = VirtualKnob(0, "Distortion")
    .Linear(0.f, 1.f)
    .Ring(Level(kPalette.distortion, FillAnim::Pulse));

static VirtualKnob k_filter = VirtualKnob(1, "Filter")
    .Linear(0.f, 1.f)
    .Ring(Level(kPalette.filter, FillAnim::Ripple));

static VirtualKnob k_volume = VirtualKnob(2, "Volume")
    .Linear(0.f, 1.f)
    .Ring(Level(kPalette.volume, FillAnim::Pulse));

static Page main_page = Page(0).Knobs(k_distortion, k_filter, k_volume);

/* ── SDK surfaces ─────────────────────────────────────────────────────────── */
static AlchemyLab              hw;
static ControlLoop             loop(hw);
static Pager                   pager(hw.buttons[0], 1, kNumPots);
static ParamLock<kNumPots>     locks(hw.buttons[0], pager);
static Presets                 presets(hw.seed.qspi);
static Settings                settings(hw, &pager);
static CvMatrix                cv_matrix(kNumCvInputs);

/* Push the summed CV+knob values into the Rust pedal each control frame.
 * The label strings must match the .pedal's `controls { }` declarations. */
static void UpdateControls()
{
    pk::SetControl("Distortion", k_distortion.Value());
    pk::SetControl("Filter",     k_filter.Value());
    pk::SetControl("Volume",     k_volume.Value());
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

    /* CV routing — one jack per control, static layout. */
    cv_matrix.Jack(0).To(k_distortion);
    cv_matrix.Jack(1).To(k_filter);
    cv_matrix.Jack(2).To(k_volume);

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
