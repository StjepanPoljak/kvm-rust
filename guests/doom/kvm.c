/*
 * doomgeneric platform layer for the KVM Rust VMM. Entered from boot.S
 * with the PVH start info. Modules passed with --initramfs show up as
 * files named after their command line (the VMM puts the file name
 * there), so `--initramfs freedoom1.wad` is found by Doom's IWAD search.
 */

#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include "doomgeneric.h"
#include "i_video.h"
#include "kvm.h"

#define XEN_HVM_START_MAGIC	0x336ec578
#define E820_RAM		1

struct hvm_start_info {
	uint32_t magic;
	uint32_t version;
	uint32_t flags;
	uint32_t nr_modules;
	uint64_t modlist_paddr;
	uint64_t cmdline_paddr;
	uint64_t rsdp_paddr;
	uint64_t memmap_paddr;
	uint32_t memmap_entries;
	uint32_t reserved;
};

struct hvm_modlist_entry {
	uint64_t paddr;
	uint64_t size;
	uint64_t cmdline_paddr;
	uint64_t reserved;
};

struct hvm_memmap_entry {
	uint64_t addr;
	uint64_t size;
	uint32_t type;
	uint32_t reserved;
};

/* End of the loaded image, from linker.ld. */
extern char _end[];

#define ARGV_MAX	32

static char cmdline[1024];
static char *argv[ARGV_MAX];

/* Splits the kernel command line into argv after `fixed` leading entries.
 * Linux-style words such as console=ttyS0 are harmless: Doom only looks
 * for the options it knows. */
static int build_argv(const char *line, int fixed)
{
	int argc = fixed;
	strncpy(cmdline, line, sizeof(cmdline) - 1);
	for (char *p = cmdline; *p && argc < ARGV_MAX - 1;) {
		while (*p == ' ')
			*p++ = '\0';
		if (!*p)
			break;
		argv[argc++] = p;
		while (*p && *p != ' ')
			p++;
	}
	argv[argc] = NULL;
	return argc;
}

/* The heap starts above the image and every module, and runs to the end
 * of the RAM range it starts in. */
static void setup_memory(const struct hvm_start_info *si)
{
	uintptr_t start = (uintptr_t)_end;
	const struct hvm_modlist_entry *mods = (const void *)(uintptr_t)si->modlist_paddr;

	for (uint32_t i = 0; i < si->nr_modules; i++) {
		const char *name = mods[i].cmdline_paddr ? (const char *)(uintptr_t)mods[i].cmdline_paddr : "";
		if (!*name)
			name = "module";
		memfs_add(name, (const void *)(uintptr_t)mods[i].paddr, mods[i].size);
		printf("[kvm] module %s: %lu KiB at %#lx\n", name,
		       (unsigned long)(mods[i].size >> 10), (unsigned long)mods[i].paddr);
		if (mods[i].paddr + mods[i].size > start)
			start = mods[i].paddr + mods[i].size;
	}

	uintptr_t end = 0;
	const struct hvm_memmap_entry *map = (const void *)(uintptr_t)si->memmap_paddr;
	for (uint32_t i = 0; i < si->memmap_entries; i++)
		if (map[i].type == E820_RAM && map[i].addr <= start && start < map[i].addr + map[i].size)
			end = map[i].addr + map[i].size;
	if (!end) {
		puts("[kvm] no RAM above the image");
		power_off(1);
	}

	heap_init(start, end);
	printf("[kvm] heap %#lx-%#lx (%lu MiB)\n", (unsigned long)start, (unsigned long)end,
	       (unsigned long)((end - start) >> 20));
}

/* --- doomgeneric */

static uint32_t frames, frames_reported_at;

void DG_Init(void)
{
	vga_set_mode_13h();
	frames_reported_at = clock_ms();
}

/* Built with CMAP256 and a 320x200 DG_ScreenBuffer, Doom's frame is
 * already in mode 13h's format: palette indices, one byte per pixel. */
void DG_DrawFrame(void)
{
	if (palette_changed) {
		static uint8_t rgb[256][3];
		for (int i = 0; i < 256; i++) {
			rgb[i][0] = colors[i].r;
			rgb[i][1] = colors[i].g;
			rgb[i][2] = colors[i].b;
		}
		vga_set_palette(rgb);
		palette_changed = false;
	}
	memcpy(VGA_FRAMEBUFFER, DG_ScreenBuffer, VGA_WIDTH * VGA_HEIGHT);

	uint32_t now = clock_ms();
	frames++;
	if (now - frames_reported_at >= 1000) {
		printf("[kvm] %u frames in %u ms\n", frames, now - frames_reported_at);
		frames = 0;
		frames_reported_at = now;
	}
}

void DG_SleepMs(uint32_t ms)
{
	sleep_ms(ms);
}

uint32_t DG_GetTicksMs(void)
{
	return clock_ms();
}

/* No keyboard yet. */
int DG_GetKey(int *pressed, unsigned char *key)
{
	(void)pressed;
	(void)key;
	return 0;
}

void DG_SetWindowTitle(const char *title)
{
	printf("[kvm] title: %s\n", title);
}

void kmain(uint32_t start_info_paddr)
{
	const struct hvm_start_info *si = (const void *)(uintptr_t)start_info_paddr;

	puts("[kvm] doom guest started");
	if (si->magic != XEN_HVM_START_MAGIC) {
		printf("[kvm] bad PVH start info magic %#x\n", si->magic);
		power_off(1);
	}
	setup_memory(si);
	clock_init();
	interrupts_init();

	/* User options come first: Doom takes the first match of an option,
	 * so they override the defaults after them. -mb sizes Doom's zone. */
	argv[0] = "doom";
	int argc = build_argv(si->cmdline_paddr ? (const char *)(uintptr_t)si->cmdline_paddr : "", 1);
	if (argc + 2 < ARGV_MAX) {
		argv[argc++] = "-mb";
		argv[argc++] = "64";
		argv[argc] = NULL;
	}

	doomgeneric_Create(argc, argv);
	for (;;)
		doomgeneric_Tick();
}
