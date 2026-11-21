; Master boot record of the install disk (install.mjs adds the partition table): moves itself out
; of the way and boots the active partition, read with the BIOS disk extensions
; Build: nasm -f bin -o out/mbr.bin mbr.asm
bits 16
org 0x600

start:
    cli
    xor ax, ax
    mov ss, ax
    mov sp, 0x7c00
    mov ds, ax
    mov es, ax
    sti
    cld
    mov si, 0x7c00
    mov di, 0x600
    mov cx, 256
    rep movsw
    jmp 0:relocated

relocated:
    mov [drive], dl
    mov si, partitions
    mov cx, 4
.find:
    cmp byte [si], 0x80
    je .found
    add si, 16
    loop .find
    mov si, no_partition
    jmp fail

.found:
    ; The boot sector expects DS:SI on its partition entry and DL on the drive
    mov bp, si
    mov eax, [si + 8]
    mov [packet.lba], eax
    mov si, packet
    mov ah, 0x42
    int 0x13
    jc .read_error
    cmp word [0x7dfe], 0xaa55
    jne .read_error
    mov si, bp
    mov dl, [drive]
    jmp 0:0x7c00
.read_error:
    mov si, read_error

fail:
    lodsb
    test al, al
    jz .halt
    mov ah, 0x0e
    mov bx, 7
    int 0x10
    jmp fail
.halt:
    hlt
    jmp .halt

drive: db 0
no_partition: db "No active partition", 0
read_error: db "Can't read the boot sector", 0

align 4
packet:
    db 16, 0
    dw 1            ; sectors
    dw 0x7c00, 0    ; buffer
.lba: dd 0, 0

    times 440 - ($ - $$) db 0
partitions equ 0x600 + 446
