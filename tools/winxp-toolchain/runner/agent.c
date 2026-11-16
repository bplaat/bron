/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// The guest side of winxp-runner: runs from the Startup folder of Windows XP and runs the
// programs the host sends over COM1. The protocol is lines with a binary payload:
//   guest: READY                     waiting for a program
//   host:  RUN <size> <arguments>    then <size> bytes of the program
//   guest: OUT <size> / ERR <size>   then <size> bytes of its stdout or stderr
//   guest: EXIT <code>               the program exited, then READY again

#include <stdio.h>
#include <string.h>
#include <windows.h>

#define PROGRAM_DIR "C:\\runner"
#define PROGRAM_PATH PROGRAM_DIR "\\program.exe"

static HANDLE serial;
static CRITICAL_SECTION serial_lock;

static void serial_write(const void* data, DWORD size) {
    const char* bytes = data;
    while (size > 0) {
        DWORD written;
        if (!WriteFile(serial, bytes, size, &written, NULL))
            ExitProcess(1);
        bytes += written;
        size -= written;
    }
}

static void serial_read(void* data, DWORD size) {
    char* bytes = data;
    while (size > 0) {
        DWORD read;
        if (!ReadFile(serial, bytes, size, &read, NULL))
            ExitProcess(1);
        bytes += read;
        size -= read;
    }
}

// Reads a line without its newline, returns its length
static size_t serial_read_line(char* line, size_t size) {
    size_t length = 0;
    for (;;) {
        char c;
        serial_read(&c, 1);
        if (c == '\n')
            break;
        if (length < size - 1)
            line[length++] = c;
    }
    line[length] = '\0';
    return length;
}

// Sends a header line and its payload as one frame, the stdout and stderr threads share COM1
static void send_frame(const char* name, const void* data, DWORD size) {
    char header[32];
    int length = snprintf(header, sizeof(header), "%s %lu\n", name, (unsigned long)size);
    EnterCriticalSection(&serial_lock);
    serial_write(header, (DWORD)length);
    serial_write(data, size);
    LeaveCriticalSection(&serial_lock);
}

typedef struct {
    HANDLE pipe;
    const char* name;
} Stream;

// Forwards a pipe of the program to the host until the program closes it
static DWORD WINAPI forward_stream(void* argument) {
    Stream* stream = argument;
    char buffer[4096];
    DWORD read;
    while (ReadFile(stream->pipe, buffer, sizeof(buffer), &read, NULL) && read > 0) {
        send_frame(stream->name, buffer, read);
    }
    return 0;
}

static void receive_program(DWORD size) {
    HANDLE file = CreateFileA(PROGRAM_PATH, GENERIC_WRITE, 0, NULL, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE)
        ExitProcess(1);
    char buffer[4096];
    while (size > 0) {
        DWORD chunk = size < sizeof(buffer) ? size : sizeof(buffer), written;
        serial_read(buffer, chunk);
        WriteFile(file, buffer, chunk, &written, NULL);
        size -= chunk;
    }
    CloseHandle(file);
}

static DWORD run_program(const char* arguments) {
    SECURITY_ATTRIBUTES inherit = {sizeof(inherit), NULL, TRUE};
    HANDLE out_read, out_write, err_read, err_write;
    CreatePipe(&out_read, &out_write, &inherit, 0);
    CreatePipe(&err_read, &err_write, &inherit, 0);
    SetHandleInformation(out_read, HANDLE_FLAG_INHERIT, 0);
    SetHandleInformation(err_read, HANDLE_FLAG_INHERIT, 0);
    HANDLE input = CreateFileA("NUL", GENERIC_READ, FILE_SHARE_READ, &inherit, OPEN_EXISTING, 0, NULL);

    static char command_line[32768];
    snprintf(command_line, sizeof(command_line), "\"%s\" %s", PROGRAM_PATH, arguments);
    STARTUPINFOA startup = {.cb = sizeof(startup)};
    startup.dwFlags = STARTF_USESTDHANDLES;
    startup.hStdInput = input;
    startup.hStdOutput = out_write;
    startup.hStdError = err_write;
    PROCESS_INFORMATION process;
    DWORD code = 255;
    if (CreateProcessA(NULL, command_line, NULL, NULL, TRUE, 0, NULL, PROGRAM_DIR, &startup, &process)) {
        // Only the program holds the write ends now, so the pipes close when it exits
        CloseHandle(out_write);
        CloseHandle(err_write);
        // Static, the threads can outlive this call when a child process keeps a pipe open
        static Stream out, err;
        out = (Stream){out_read, "OUT"};
        err = (Stream){err_read, "ERR"};
        HANDLE threads[] = {CreateThread(NULL, 0, forward_stream, &out, 0, NULL),
                            CreateThread(NULL, 0, forward_stream, &err, 0, NULL)};
        WaitForSingleObject(process.hProcess, INFINITE);
        GetExitCodeProcess(process.hProcess, &code);
        // Child processes can keep the pipes open, give them a moment before moving on
        WaitForMultipleObjects(2, threads, TRUE, 2000);
        CloseHandle(threads[0]);
        CloseHandle(threads[1]);
        CloseHandle(process.hProcess);
        CloseHandle(process.hThread);
    } else {
        char message[64];
        int length = snprintf(message, sizeof(message), "can't start the program: error %lu\n", GetLastError());
        send_frame("ERR", message, (DWORD)length);
        CloseHandle(out_write);
        CloseHandle(err_write);
    }
    CloseHandle(input);
    return code;
}

int WINAPI WinMain(HINSTANCE instance, HINSTANCE previous, LPSTR command_line, int show) {
    (void)instance, (void)previous, (void)command_line, (void)show;
    serial = CreateFileA("\\\\.\\COM1", GENERIC_READ | GENERIC_WRITE, 0, NULL, OPEN_EXISTING, 0, NULL);
    if (serial == INVALID_HANDLE_VALUE)
        return 1;
    DCB dcb = {.DCBlength = sizeof(dcb)};
    GetCommState(serial, &dcb);
    dcb.BaudRate = CBR_115200;
    dcb.ByteSize = 8;
    dcb.Parity = NOPARITY;
    dcb.StopBits = ONESTOPBIT;
    dcb.fBinary = TRUE;
    dcb.fOutxCtsFlow = dcb.fOutxDsrFlow = dcb.fDsrSensitivity = FALSE;
    dcb.fDtrControl = DTR_CONTROL_ENABLE;
    dcb.fRtsControl = RTS_CONTROL_ENABLE;
    dcb.fOutX = dcb.fInX = FALSE;
    SetCommState(serial, &dcb);
    // The driver drops what doesn't fit in its input buffer, make it hold a whole program
    SetupComm(serial, 16 << 20, 64 << 10);
    // Block until data comes in, the host decides when the next program runs
    COMMTIMEOUTS timeouts = {0};
    SetCommTimeouts(serial, &timeouts);
    InitializeCriticalSection(&serial_lock);
    CreateDirectoryA(PROGRAM_DIR, NULL);

    for (;;) {
        serial_write("READY\n", 6);
        char line[32768];
        serial_read_line(line, sizeof(line));
        unsigned long size;
        int offset = 0;
        if (sscanf(line, "RUN %lu %n", &size, &offset) != 1)
            continue;
        receive_program((DWORD)size);
        DWORD code = run_program(line + offset);
        char exit_line[32];
        int length = snprintf(exit_line, sizeof(exit_line), "EXIT %lu\n", (unsigned long)code);
        EnterCriticalSection(&serial_lock);
        serial_write(exit_line, (DWORD)length);
        LeaveCriticalSection(&serial_lock);
    }
}
