/*
 * doomgeneric platform layer for the Hearth VMM. Entered from boot.S
 * with the PVH start info. Modules passed with --initramfs show up as
 * files named after their command line (the VMM puts the file name
 * there), so `--initramfs freedoom1.wad` is found by Doom's IWAD search.
 */

#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include "doomgeneric.h"
#include "doomkeys.h"
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

/* Scancode set 1 to Doom keys: letters and digits as lowercase ASCII,
 * the rest as doomkeys.h names them. Ctrl fires and space uses, as in
 * the other doomgeneric ports; keypad arrows work as arrows. */
static const unsigned char set1_to_doom[0x59] = {
	[0x01] = KEY_ESCAPE,
	[0x02] = '1', '2', '3', '4', '5', '6', '7', '8', '9', '0', '-', '=',
	[0x0e] = KEY_BACKSPACE, KEY_TAB,
	[0x10] = 'q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p', '[', ']',
	[0x1c] = KEY_ENTER, KEY_FIRE,
	[0x1e] = 'a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l', ';', '\'', '`',
	[0x2a] = KEY_RSHIFT, '\\',
	[0x2c] = 'z', 'x', 'c', 'v', 'b', 'n', 'm', ',', '.', '/',
	[0x36] = KEY_RSHIFT,
	[0x38] = KEY_LALT, KEY_USE, KEY_CAPSLOCK,
	[0x3b] = KEY_F1, KEY_F2, KEY_F3, KEY_F4, KEY_F5, KEY_F6, KEY_F7, KEY_F8, KEY_F9, KEY_F10,
	[0x48] = KEY_UPARROW,
	[0x4b] = KEY_LEFTARROW,
	[0x4d] = KEY_RIGHTARROW,
	[0x50] = KEY_DOWNARROW,
	[0x57] = KEY_F11, KEY_F12,
};

/* After an 0xe0 prefix */
static unsigned char set1_extended_to_doom(uint8_t code)
{
	switch (code) {
	case 0x1c: return KEY_ENTER;	/* keypad Enter */
	case 0x1d: return KEY_FIRE;	/* right Ctrl */
	case 0x38: return KEY_RALT;
	case 0x47: return KEY_HOME;
	case 0x48: return KEY_UPARROW;
	case 0x49: return KEY_PGUP;
	case 0x4b: return KEY_LEFTARROW;
	case 0x4d: return KEY_RIGHTARROW;
	case 0x4f: return KEY_END;
	case 0x50: return KEY_DOWNARROW;
	case 0x51: return KEY_PGDN;
	case 0x52: return KEY_INS;
	case 0x53: return KEY_DEL;
	default: return 0;
	}
}

int DG_GetKey(int *pressed, unsigned char *key)
{
	static int extended;
	uint8_t scancode;

	while (keyboard_read(&scancode)) {
		if (scancode == 0xe0) {
			extended = 1;
			continue;
		}
		uint8_t code = scancode & 0x7f;
		unsigned char doom_key = extended ? set1_extended_to_doom(code) :
			code < sizeof(set1_to_doom) ? set1_to_doom[code] : 0;
		extended = 0;
		if (!doom_key)
			continue;
		*pressed = !(scancode & 0x80);
		*key = doom_key;
		return 1;
	}
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
	keyboard_init();

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
