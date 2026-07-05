aarch64-linux-gnu-as -o hello-world-arm.o hello-world-arm.asm
aarch64-linux-gnu-ld -T hello-world-arm.ld -o hello-world-arm.elf hello-world-arm.o
aarch64-linux-gnu-objcopy -O binary hello-world-arm.elf hello-world-arm.img
