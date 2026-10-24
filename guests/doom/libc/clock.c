/*
 * Time from the KVM pvclock ("kvmclock"). The guest gives KVM the address
 * of a pvclock_vcpu_time_info through an MSR, and KVM keeps it filled
 * with a (TSC, nanoseconds) reference point and the TSC's rate; the
 * current time is then that point plus the TSC ticks since, scaled.
 * See Documentation/virt/kvm/x86/msr.rst in Linux.
 */

#include <stdio.h>
#include <string.h>
#include "kvm.h"

#define KVM_CPUID_SIGNATURE	0x40000000
#define KVM_CPUID_FEATURES	0x40000001
#define KVM_FEATURE_CLOCKSOURCE2 (1 << 3)
#define MSR_KVM_SYSTEM_TIME_NEW	0x4b564d01
#define KVM_SYSTEM_TIME_ENABLE	1

struct pvclock_vcpu_time_info {
	uint32_t version;	/* odd while KVM is updating the rest */
	uint32_t pad0;
	uint64_t tsc_timestamp;
	uint64_t system_time;	/* ns at tsc_timestamp */
	uint32_t tsc_to_system_mul;
	int8_t tsc_shift;
	uint8_t flags;
	uint8_t pad[2];
};

/* Must not cross a page boundary. */
static volatile struct pvclock_vcpu_time_info time_info __attribute__((aligned(32)));
static uint64_t boot_ns;

static inline void cpuid(uint32_t leaf, uint32_t *a, uint32_t *b, uint32_t *c, uint32_t *d)
{
	__asm__ volatile("cpuid" : "=a"(*a), "=b"(*b), "=c"(*c), "=d"(*d) : "a"(leaf), "c"(0));
}

static inline void wrmsr(uint32_t msr, uint64_t val)
{
	__asm__ volatile("wrmsr" : : "c"(msr), "a"((uint32_t)val), "d"((uint32_t)(val >> 32)));
}

static inline uint64_t rdtsc(void)
{
	uint32_t lo, hi;
	__asm__ volatile("rdtsc" : "=a"(lo), "=d"(hi));
	return ((uint64_t)hi << 32) | lo;
}

static uint64_t pvclock_ns(void)
{
	uint32_t version;
	uint64_t ns;

	do {
		version = time_info.version;
		__asm__ volatile("" ::: "memory");
		uint64_t delta = rdtsc() - time_info.tsc_timestamp;
		int8_t shift = time_info.tsc_shift;
		delta = shift < 0 ? delta >> -shift : delta << shift;
		ns = time_info.system_time +
		     (uint64_t)(((unsigned __int128)delta * time_info.tsc_to_system_mul) >> 32);
		__asm__ volatile("" ::: "memory");
	} while ((version & 1) || version != time_info.version);
	return ns;
}

void clock_init(void)
{
	uint32_t a, b, c, d;
	char sig[13];

	cpuid(KVM_CPUID_SIGNATURE, &a, &b, &c, &d);
	memcpy(sig, &b, 4);
	memcpy(sig + 4, &c, 4);
	memcpy(sig + 8, &d, 4);
	sig[12] = '\0';
	cpuid(KVM_CPUID_FEATURES, &a, &b, &c, &d);
	if (strcmp(sig, "KVMKVMKVM") || !(a & KVM_FEATURE_CLOCKSOURCE2)) {
		printf("[kvm] no kvmclock (CPUID %#x: \"%s\", features %#x)\n",
		       KVM_CPUID_SIGNATURE, sig, a);
		power_off(1);
	}

	wrmsr(MSR_KVM_SYSTEM_TIME_NEW, (uintptr_t)&time_info | KVM_SYSTEM_TIME_ENABLE);
	boot_ns = pvclock_ns();
}

uint64_t clock_ns(void)
{
	return pvclock_ns() - boot_ns;
}

uint32_t clock_ms(void)
{
	return clock_ns() / 1000000;
}
