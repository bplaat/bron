/*
 * v86 guest integration - shared host protocol definitions.
 *
 * The host side lives in the web page: it extends v86's VMware backdoor
 * (port 0x5658) with a few extra commands. Bulk data moves through a
 * physically contiguous guest buffer that the host reads and writes directly.
 */
#ifndef V86_H
#define V86_H

#define VMWARE_PORT 0x5658
#define VMWARE_MAGIC 0x564D5868

/* Stock VMware commands implemented by v86 */
#define VMWARE_CMD_GETVERSION 10
#define VMWARE_CMD_GETTIME 23 /* EAX = unix seconds, EBX = usec, EDX = UTC offset in minutes */
#define VMWARE_CMD_ABSPOINTER_DATA 39
#define VMWARE_CMD_ABSPOINTER_STATUS 40
#define VMWARE_CMD_ABSPOINTER_COMMAND 41
#define VMWARE_ABSPOINTER_ENABLE 0x45414552
#define VMWARE_ABSPOINTER_ABSOLUTE 0x53424152
#define VMWARE_ABSPOINTER_READ_ID 0x3442554A
#define VMWARE_BUTTON_LEFT 0x20
#define VMWARE_BUTTON_RIGHT 0x10
#define VMWARE_BUTTON_MIDDLE 0x08
#define VMWARE_RELATIVE_PACKET 0x00010000

/* Extra commands implemented by the page */
#define V86_CMD_HELLO 0x8600   /* returns V86_HELLO_REPLY */
#define V86_CMD_GETSIZE 0x8601 /* returns (width << 16) | height wanted by the host, 0 = none */
#define V86_CMD_RECV 0x8602    /* EBX = buffer phys, ESI = capacity; returns type, EBX = length */
#define V86_CMD_SEND 0x8603    /* EBX = buffer phys, ESI = length, EDI = type */
/* EDX always holds the port number, so arguments travel in EBX, ESI and EDI */
#define V86_HELLO_REPLY 0x86868686

/* Message types */
#define V86_MSG_NONE 0
#define V86_MSG_CLIP_FORMAT 1 /* both directions: UINT32 format, UINT32 size, starts a clipboard format */
#define V86_MSG_FILE_BEGIN 2 /* host->guest: UINT32 size, UTF-16LE name */
#define V86_MSG_FILE_DATA 3  /* host->guest: next chunk */
#define V86_MSG_FILE_END 4   /* host->guest: file complete */
#define V86_MSG_LOG 5        /* guest->host: UTF-16LE diagnostic line */
#define V86_MSG_SHUTDOWN 6   /* host->guest: power off (used by the build) */
#define V86_MSG_CLIP_DATA 7  /* both directions: next chunk of the current clipboard format */
#define V86_MSG_CLIP_END 8   /* both directions: clipboard complete, replaces the previous one */
#define V86_MSG_OPEN_URL 9   /* guest->host: UTF-16LE URL to open in a new browser tab */
#define V86_MSG_NET_RENEW 10 /* host->guest: renew the DHCP lease, the snapshot comes up with a stale address */

/* Clipboard formats */
#define V86_CLIP_TEXT 0  /* UTF-16LE, CRLF line ends */
#define V86_CLIP_HTML 1  /* UTF-8 fragment */
#define V86_CLIP_IMAGE 2 /* UINT32 width, UINT32 height, top-down BGRX pixels */
#define V86_CLIP_COUNT 3
#define V86_CLIP_MAX (64 * 1024 * 1024)

#define V86_BUFFER_SIZE (256 * 1024)

/* Control device exposed by v86mou.sys for the user mode agent */
#define V86_DEVICE_NAME L"\\Device\\V86Tools"
#define V86_DOS_NAME L"\\DosDevices\\V86Tools"
#define V86_USER_PATH "\\\\.\\V86Tools"

#define V86_IOCTL(n) CTL_CODE(FILE_DEVICE_UNKNOWN, 0x860 + (n), METHOD_BUFFERED, FILE_ANY_ACCESS)
#define IOCTL_V86_GETSIZE V86_IOCTL(1) /* out: ULONG */
#define IOCTL_V86_RECV V86_IOCTL(2)    /* out: ULONG type, ULONG length, payload */
#define IOCTL_V86_SEND V86_IOCTL(3)    /* in: ULONG type, payload */
#define IOCTL_V86_GETTIME V86_IOCTL(4) /* out: ULONG seconds, ULONG usec, LONG UTC offset in minutes */

typedef struct {
    unsigned long eax, ebx, ecx, edx, esi, edi;
} V86_REGS;

static inline void v86_backdoor(V86_REGS *r)
{
    __asm__ __volatile__("inl %%dx, %%eax"
                         : "=a"(r->eax), "=b"(r->ebx), "=c"(r->ecx), "=d"(r->edx), "=S"(r->esi), "=D"(r->edi)
                         : "a"(VMWARE_MAGIC), "b"(r->ebx), "c"(r->ecx), "d"(VMWARE_PORT), "S"(r->esi), "D"(r->edi)
                         : "memory");
}

static inline unsigned long v86_call(unsigned long cmd, unsigned long ebx)
{
    V86_REGS r = {0, ebx, cmd, 0, 0, 0};
    v86_backdoor(&r);
    return r.eax;
}

#endif
