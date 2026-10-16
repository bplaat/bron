// setjmp() and longjmp() save and restore the registers a function has to keep: r4 to r11, sp
// and lr. They are ARM code, Thumb code calls them with interworking.
    .syntax unified
    .arm
    .text

    .global setjmp
    .type setjmp, %function
setjmp:
    mov r12, sp
    stmia r0, {r4-r12, lr}
    mov r0, #0
    bx lr
    .size setjmp, . - setjmp

    .global longjmp
    .type longjmp, %function
longjmp:
    ldmia r0, {r4-r12, lr}
    mov sp, r12
    movs r0, r1
    moveq r0, #1
    bx lr
    .size longjmp, . - longjmp
