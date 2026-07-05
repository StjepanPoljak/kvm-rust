org 0x1000

bits 16

mov ax, 0xb800
mov es, ax

mov si, msg
xor di, di

.loop:
    lodsb
    test al, al
    jz .done

    mov [es:di], al
    inc di

    mov byte [es:di], 0x07
    inc di

    jmp .loop

.done:
    hlt

msg db "Hello from KVM!", 0x0d, 0x0a, 0

