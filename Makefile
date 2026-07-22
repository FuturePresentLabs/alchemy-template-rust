# =============================================================================
# alchemy-template-rust — Daisy-bootloader firmware whose DSP is a pedalkernel
# WDF circuit, cross-compiled from Rust and linked into the C++ Alchemy SDK.
#
#   make libdaisy       — build lib/libDaisy once after cloning
#   make                — build the Rust DSP lib + firmware (BOARD=v2 by default)
#   make program-dfu    — flash over USB (module in DFU mode first; see README)
#   make clean          — remove the build tree (C++ and Rust)
#
# Requires a Rust toolchain with the ARM target:
#   rustup target add thumbv7em-none-eabihf
#
# The Rust half lives in dsp/ and builds to libpedal_dsp.a; the pedal circuit
# it runs is dsp/pedals/demo.pedal, compiled into the firmware at build time.
# Tunables (also settable in dsp/build.rs' env vars) are forwarded to cargo:
#   PK_SAMPLE_RATE (48000)  PK_OVERSAMPLING (1)  PK_PEDAL (pedals/demo.pedal)
# =============================================================================

TARGET = pedal

# Alchemy Lab board revision: v1 | v2
BOARD ?= v2
ifeq ($(filter $(BOARD),v1 v2),)
$(error BOARD must be 'v1' or 'v2' (got '$(BOARD)'))
endif

ALCHEMY_DIR  = lib/alchemy-sdk
LIBDAISY_DIR = lib/libDaisy

# ── Rust DSP static library ─────────────────────────────────────────────────
RUST_DIR     = dsp
RUST_TARGET  = thumbv7em-none-eabihf
RUST_LIB_DIR = $(RUST_DIR)/target/$(RUST_TARGET)/release
RUST_LIB     = $(RUST_LIB_DIR)/libpedal_dsp.a

# Pedal-compile tunables, forwarded to dsp/build.rs (see its header comment).
PK_SAMPLE_RATE  ?= 48000
PK_OVERSAMPLING ?= 1
PK_PEDAL        ?= pedals/demo.pedal

# ── App sources — yours to edit ─────────────────────────────────────────────
CPP_SOURCES = \
    src/pedal.cpp

# ── Alchemy SDK, compiled straight from the submodule ───────────────────────
CPP_SOURCES += $(sort $(shell find $(ALCHEMY_DIR)/framework/src -name '*.cpp'))
CPP_SOURCES += $(sort $(wildcard $(ALCHEMY_DIR)/hardware/alchemy-lab/$(BOARD)/src/*.cpp))

C_INCLUDES += \
    -Isrc \
    -I$(ALCHEMY_DIR)/framework/include \
    -I$(ALCHEMY_DIR)/hardware/include \
    -I$(ALCHEMY_DIR)/hardware/alchemy-lab/$(BOARD)/include

ifeq ($(BOARD),v2)
C_DEFS += -DALCHEMY_BOARD_V2
endif

# ── Daisy bootloader build (BOOT_SRAM) ──────────────────────────────────────
APP_TYPE = BOOT_SRAM
LDSCRIPT = $(ALCHEMY_DIR)/cmake/linkers/alchemy_stm32h750ib_sram.lds

# The Alchemy SDK requires C++17 (libDaisy's default is gnu++14).
CPP_STANDARD = -std=gnu++17

# ── libDaisy core Makefile does the rest ────────────────────────────────────
SYSTEM_FILES_DIR = $(LIBDAISY_DIR)/core
include $(SYSTEM_FILES_DIR)/Makefile

# ── Link the Rust DSP lib (appended AFTER the include so it lands in LDFLAGS).
# The group lets libpedal_dsp resolve against libc/libm/nosys regardless of
# link order; libOBJECTS reference pk_* which pulls the archive in.
LIBDIR += -L$(RUST_LIB_DIR)
LIBS   += -Wl,--start-group -lpedal_dsp -lc -lm -lnosys -Wl,--end-group

# Build the Rust lib before linking. `rust-dsp` is phony so cargo always runs
# (its own incremental cache makes that cheap); the firmware .elf depends on
# the resulting archive.
.PHONY: rust-dsp
rust-dsp:
	cd $(RUST_DIR) && PK_SAMPLE_RATE=$(PK_SAMPLE_RATE) PK_OVERSAMPLING=$(PK_OVERSAMPLING) PK_PEDAL=$(PK_PEDAL) cargo build --release

$(RUST_LIB): rust-dsp
$(BUILD_DIR)/$(TARGET).elf: $(RUST_LIB)

# Re-clean object files when switching board revision.
BOARD_STAMP := $(BUILD_DIR)/.board-$(BOARD)
ifeq ($(wildcard $(BOARD_STAMP)),)
_BOARD_GUARD := $(shell rm -f $(BUILD_DIR)/*.o $(BUILD_DIR)/*.d $(BUILD_DIR)/*.lst $(BUILD_DIR)/.board-* 2>/dev/null; mkdir -p $(BUILD_DIR); touch $(BOARD_STAMP))
endif

.PHONY: libdaisy
libdaisy:
	$(MAKE) -C $(LIBDAISY_DIR)

# Extend the core `clean` to also clean the Rust build.
.PHONY: rust-clean
clean: rust-clean
rust-clean:
	cd $(RUST_DIR) && cargo clean
