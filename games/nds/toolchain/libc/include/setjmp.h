#pragma once

// r4 to r11, sp and lr
typedef int jmp_buf[10];

int setjmp(jmp_buf env) __attribute__((returns_twice));
void longjmp(jmp_buf env, int value) __attribute__((noreturn));
