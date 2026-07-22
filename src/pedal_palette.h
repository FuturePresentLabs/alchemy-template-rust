/**
 * pedal_palette.h — LED ring colors, one per knob slot.
 *
 * Purely cosmetic. Since the knobs are wired to whatever controls the loaded
 * `.pedal` exposes (in order), colors are indexed by slot rather than named.
 */

#pragma once

#include "alchemy/led/panel.h"

// One color per physical pot (up to kNumPots). Cycled by control index.
constexpr alchemy::LedPanel::Rgb kRingPalette[] = {
    {0xFF, 0x30, 0x00}, // hot red/orange
    {0x00, 0xC0, 0xFF}, // cyan
    {0xFF, 0xB0, 0x20}, // amber
    {0x40, 0xFF, 0x60}, // green
    {0xC0, 0x40, 0xFF}, // violet
    {0xFF, 0x40, 0x90}, // pink
};
constexpr size_t kRingPaletteLen = sizeof(kRingPalette) / sizeof(kRingPalette[0]);
