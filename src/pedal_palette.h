/**
 * pedal_palette.h — LED ring colors for the three demo controls.
 *
 * Purely cosmetic and optional — it just keeps the colors out of the main
 * flow of pedal.cpp. One `Rgb` per knob.
 */

#pragma once

#include "alchemy/led/panel.h"

struct PedalPalette
{
    alchemy::LedPanel::Rgb distortion, filter, volume;
};

// RAT-ish: hot distortion, cool filter sweep, warm output level.
constexpr PedalPalette kPalette = {
    {0xFF, 0x30, 0x00}, // Distortion — hot red/orange
    {0x00, 0xC0, 0xFF}, // Filter     — cyan
    {0xFF, 0xB0, 0x20}, // Volume     — amber
};
