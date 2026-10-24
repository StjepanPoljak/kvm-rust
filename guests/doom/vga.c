/*
 * VGA mode 13h: 320x200, one byte per pixel indexing a 256-colour
 * palette, framebuffer linear at 0xA0000. With no BIOS to call int 10h,
 * the mode is set by writing the VGA registers directly. The register
 * values are the standard mode 13h ones (as in Chris Giese's modes.c).
 */

#include <string.h>
#include "kvm.h"

#define VGA_AC_INDEX		0x3c0	/* attribute controller: index and data */
#define VGA_MISC_WRITE		0x3c2
#define VGA_SEQ_INDEX		0x3c4
#define VGA_SEQ_DATA		0x3c5
#define VGA_DAC_WRITE_INDEX	0x3c8
#define VGA_DAC_DATA		0x3c9
#define VGA_GC_INDEX		0x3ce
#define VGA_GC_DATA		0x3cf
#define VGA_CRTC_INDEX		0x3d4
#define VGA_CRTC_DATA		0x3d5
#define VGA_INSTAT_READ		0x3da	/* reading resets the AC index/data flip-flop */

#define VGA_AC_PAS		0x20	/* palette address source: video on */
#define VGA_CRTC_PROTECT	0x11	/* bit 7 write-protects CRTC 0-7 */
#define VGA_CRTC_HBLANK_END	0x03	/* bit 7 must be set */

static const uint8_t misc = 0x63;
static const uint8_t seq[5] = { 0x03, 0x01, 0x0f, 0x00, 0x0e };
static const uint8_t crtc[25] = {
	0x5f, 0x4f, 0x50, 0x82, 0x54, 0x80, 0xbf, 0x1f,
	0x00, 0x41, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
	0x9c, 0x0e, 0x8f, 0x28, 0x40, 0x96, 0xb9, 0xa3,
	0xff,
};
static const uint8_t gc[9] = { 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x05, 0x0f, 0xff };
static const uint8_t ac[21] = {
	0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
	0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
	0x41, 0x00, 0x0f, 0x00, 0x00,
};

void vga_set_mode_13h(void)
{
	outb(VGA_MISC_WRITE, misc);

	for (uint8_t i = 0; i < sizeof(seq); i++) {
		outb(VGA_SEQ_INDEX, i);
		outb(VGA_SEQ_DATA, seq[i]);
	}

	/* Unlock CRTC 0-7, and keep them unlocked through the writes below. */
	outb(VGA_CRTC_INDEX, VGA_CRTC_HBLANK_END);
	outb(VGA_CRTC_DATA, inb(VGA_CRTC_DATA) | 0x80);
	outb(VGA_CRTC_INDEX, VGA_CRTC_PROTECT);
	outb(VGA_CRTC_DATA, inb(VGA_CRTC_DATA) & ~0x80);
	for (uint8_t i = 0; i < sizeof(crtc); i++) {
		uint8_t val = crtc[i];
		if (i == VGA_CRTC_HBLANK_END)
			val |= 0x80;
		if (i == VGA_CRTC_PROTECT)
			val &= ~0x80;
		outb(VGA_CRTC_INDEX, i);
		outb(VGA_CRTC_DATA, val);
	}

	for (uint8_t i = 0; i < sizeof(gc); i++) {
		outb(VGA_GC_INDEX, i);
		outb(VGA_GC_DATA, gc[i]);
	}

	/* The attribute controller takes index and data on one port. */
	for (uint8_t i = 0; i < sizeof(ac); i++) {
		inb(VGA_INSTAT_READ);
		outb(VGA_AC_INDEX, i);
		outb(VGA_AC_INDEX, ac[i]);
	}
	inb(VGA_INSTAT_READ);
	outb(VGA_AC_INDEX, VGA_AC_PAS);

	memset(VGA_FRAMEBUFFER, 0, VGA_WIDTH * VGA_HEIGHT);
}

/* The DAC takes 6 bits per component; the write index auto-increments
 * after each blue. */
void vga_set_palette(const uint8_t rgb[256][3])
{
	outb(VGA_DAC_WRITE_INDEX, 0);
	for (int i = 0; i < 256; i++) {
		outb(VGA_DAC_DATA, rgb[i][0] >> 2);
		outb(VGA_DAC_DATA, rgb[i][1] >> 2);
		outb(VGA_DAC_DATA, rgb[i][2] >> 2);
	}
}
