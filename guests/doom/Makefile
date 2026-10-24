# Bare-metal Doom for the KVM Rust VMM: doomgeneric, a small libc and a
# PVH entry stub, linked into one ELF that the VMM boots with --kernel.
#
#   make            fetch doomgeneric and Freedoom if needed, build doom.elf
#   make run        boot it (from the repository root's target/debug)
#
# Downloads and build output go to the repository's build/ directory.

ROOT		:= $(abspath ../..)
BUILD		:= $(ROOT)/build/doom
DOOMGENERIC	:= $(ROOT)/build/doomgeneric
DOOMGENERIC_URL	:= https://github.com/ozkl/doomgeneric
DOOMGENERIC_REV	:= e257a09527062bfb986b6e5e0780839b6cbd5b71
FREEDOOM_VER	:= 0.13.0
FREEDOOM_URL	:= https://github.com/freedoom/freedoom/releases/download/v$(FREEDOOM_VER)/freedoom-$(FREEDOOM_VER).zip
WAD		:= $(ROOT)/build/wad/freedoom-$(FREEDOOM_VER)/freedoom1.wad
VMM		:= $(ROOT)/target/debug/rust

CC		:= gcc
SRC		:= $(DOOMGENERIC)/doomgeneric

# The upstream Makefile's list, minus its X11 platform file.
DOOM_OBJS := dummy am_map doomdef doomstat dstrings d_event d_items d_iwad \
	d_loop d_main d_mode d_net f_finale f_wipe g_game hu_lib hu_stuff info \
	i_cdmus i_endoom i_joystick i_scale i_sound i_system i_timer memio \
	m_argv m_bbox m_cheat m_config m_controls m_fixed m_menu m_misc m_random \
	p_ceilng p_doors p_enemy p_floor p_inter p_lights p_map p_maputl p_mobj \
	p_plats p_pspr p_saveg p_setup p_sight p_spec p_switch p_telept p_tick \
	p_user r_bsp r_data r_draw r_main r_plane r_segs r_sky r_things sha1 \
	sounds statdump st_lib st_stuff s_sound tables v_video wi_stuff \
	w_checksum w_file w_main w_wad z_zone w_file_stdc i_input i_video \
	doomgeneric
LIBC_OBJS := clock ctype stdio stdlib string sys

OBJS := $(BUILD)/boot.o $(BUILD)/kvm.o \
	$(addprefix $(BUILD)/libc/, $(addsuffix .o, $(LIBC_OBJS))) \
	$(addprefix $(BUILD)/doom/, $(addsuffix .o, $(DOOM_OBJS)))

# Freestanding: only our libc headers and the compiler's own (stdint.h,
# stdarg.h, ...). SSE is fine, boot.S enables it.
CFLAGS := -O2 -g -ffreestanding -nostdinc \
	-isystem libc/include -isystem $(shell $(CC) -print-file-name=include) \
	-fno-pie -fno-stack-protector -fno-asynchronous-unwind-tables \
	-mno-red-zone -mcmodel=small -ffunction-sections -fdata-sections
LDFLAGS := -nostdlib -static -no-pie -T linker.ld -Wl,--gc-sections -Wl,--build-id=none \
	-Wl,-z,max-page-size=0x1000 -Wl,-z,noexecstack -Wl,--no-warn-rwx-segments

all: $(BUILD)/doom.elf $(WAD)

$(BUILD)/doom.elf: $(OBJS) linker.ld
	$(CC) $(CFLAGS) $(LDFLAGS) $(OBJS) -o $@ -lgcc

$(BUILD)/boot.o: boot.S | $(BUILD)
	$(CC) $(CFLAGS) -c $< -o $@

$(BUILD)/kvm.o: kvm.c libc/kvm.h $(SRC)/doomgeneric.h | $(BUILD)
	$(CC) $(CFLAGS) -Wall -Wextra -Ilibc -I$(SRC) -c $< -o $@

$(BUILD)/libc/%.o: libc/%.c libc/kvm.h | $(BUILD)
	$(CC) $(CFLAGS) -Wall -Wextra -Ilibc -c $< -o $@

# Doom's sources are old and noisy; their warnings are not ours to fix.
$(BUILD)/doom/%.o: $(SRC)/%.c | $(BUILD)
	$(CC) $(CFLAGS) -w -c $< -o $@

$(SRC)/doomgeneric.h:
	git clone -q $(DOOMGENERIC_URL) $(DOOMGENERIC)
	git -C $(DOOMGENERIC) checkout -q $(DOOMGENERIC_REV)

$(SRC)/%.c: $(SRC)/doomgeneric.h ;

$(WAD):
	mkdir -p $(ROOT)/build/wad
	curl -sSfL -o $(ROOT)/build/wad/freedoom.zip $(FREEDOOM_URL)
	cd $(ROOT)/build/wad && unzip -o -q freedoom.zip

$(BUILD):
	mkdir -p $(BUILD)/libc $(BUILD)/doom

run: all
	$(VMM) --kernel $(BUILD)/doom.elf --initramfs $(WAD)

clean:
	rm -rf $(BUILD)

.PHONY: all run clean
