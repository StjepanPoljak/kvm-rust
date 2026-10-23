# Roadmap

## Device I/O without vcpu kicks

Today the virtio-net RX thread can't touch guest memory, so it kicks a vcpu
thread out of `KVM_RUN` with `SIGRTMIN` (`VmControl::kick()` in `src/main.rs`)
and that vcpu thread fills the RX buffers in `MMIODevice::poll()`.

Goal: device threads do their own I/O and never interrupt a vcpu.

- Give device threads a handle to guest memory (e.g. `Arc<GuestMemory>`).
  Guest memory is already complete before MMIO devices are created, and
  `MemRegion` is `Send + Sync`.
- Let the RX thread lock the `VirtioMmio`, fill the RX ring directly and raise
  the interrupt with `KVM_IRQ_LINE` (callable from any thread), or via an irqfd.
- Remove `VmControl::kick()`, `devices_pending` and `MMIODevice::poll()` once
  nothing uses them. The kick signal stays for `VmControl::stop()`.
