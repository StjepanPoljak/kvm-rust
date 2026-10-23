# Roadmap

## Device I/O without vcpu kicks

Today the virtio-net RX thread can't touch guest memory, so it kicks the vcpu
thread out of `KVM_RUN` with `SIGRTMIN` (`kick_vcpu()` in `src/main.rs`) and the
vcpu thread fills the RX buffers in `MMIODevice::poll()`.

Goal: device threads do their own I/O and never interrupt the vcpu.

- Make guest memory shareable across threads (e.g. `Arc<GuestMemory>`) and set
  it up before MMIO devices are created; today `load_linux` adds regions after
  `init_mmio_devices`.
- Let the RX thread lock the `VirtioMmio`, fill the RX ring directly and raise
  the interrupt with `KVM_IRQ_LINE` (callable from any thread), or via an irqfd.
- Remove `kick_vcpu()`, `DEVICES_PENDING`, the kick signal handler and
  `MMIODevice::poll()` once nothing uses them.
