/*
 * v86mou.sys - absolute mouse filter and host channel for v86.
 *
 * Sits above mouclass, so i8042prt keeps its stock stack. While the VMware
 * absolute pointer is enabled, v86 still raises PS/2 packets for host events;
 * each mouse read they complete is rewritten with the queued backdoor packets.
 *
 * It also creates \\.\V86Tools, which lets the user mode agent talk to the
 * page (ring 3 code cannot touch the backdoor port under XP's IOPL).
 */
#include <ntddk.h>
#include <ntddmou.h>
#include "v86.h"

#define BUTTONS (VMWARE_BUTTON_LEFT | VMWARE_BUTTON_RIGHT | VMWARE_BUTTON_MIDDLE)

typedef struct {
    PDEVICE_OBJECT lower;
    LONG x, y;
    USHORT buttons;
    BOOLEAN started;
} FILTER_EXT;

static PDEVICE_OBJECT g_control;
static PVOID g_buffer;
static ULONG g_buffer_phys;
static FAST_MUTEX g_lock;
static BOOLEAN g_host;

static void enable_absolute(void)
{
    v86_call(VMWARE_CMD_ABSPOINTER_COMMAND, VMWARE_ABSPOINTER_ENABLE);
    if ((v86_call(VMWARE_CMD_ABSPOINTER_STATUS, 0) & 0xFFFF) >= 1)
        v86_call(VMWARE_CMD_ABSPOINTER_DATA, 1); /* consume READ_ID */
    v86_call(VMWARE_CMD_ABSPOINTER_COMMAND, VMWARE_ABSPOINTER_ABSOLUTE);
}

static USHORT button_flags(USHORT old, USHORT now)
{
    static const struct { USHORT mask, down, up; } map[] = {
        {VMWARE_BUTTON_LEFT, MOUSE_LEFT_BUTTON_DOWN, MOUSE_LEFT_BUTTON_UP},
        {VMWARE_BUTTON_RIGHT, MOUSE_RIGHT_BUTTON_DOWN, MOUSE_RIGHT_BUTTON_UP},
        {VMWARE_BUTTON_MIDDLE, MOUSE_MIDDLE_BUTTON_DOWN, MOUSE_MIDDLE_BUTTON_UP},
    };
    USHORT flags = 0;
    for (ULONG i = 0; i < sizeof(map) / sizeof(map[0]); i++) {
        if ((old ^ now) & map[i].mask) flags |= (now & map[i].mask) ? map[i].down : map[i].up;
    }
    return flags;
}

/* Replace one PS/2 record with the next backdoor packet, or a no-op at the current position */
static void rewrite(FILTER_EXT *ext, PMOUSE_INPUT_DATA m, BOOLEAN have_packet)
{
    m->Flags = MOUSE_MOVE_ABSOLUTE;
    m->ButtonFlags = 0;
    m->ButtonData = 0;
    m->RawButtons = 0;
    if (have_packet) {
        V86_REGS r = {0, 4, VMWARE_CMD_ABSPOINTER_DATA, 0, 0, 0};
        v86_backdoor(&r);
        USHORT buttons = (USHORT)(r.eax & BUTTONS);
        m->ButtonFlags = button_flags(ext->buttons, buttons);
        ext->buttons = buttons;
        LONG wheel = (LONG)(signed char)(r.edx & 0xFF);
        if (wheel) {
            m->ButtonFlags |= MOUSE_WHEEL;
            m->ButtonData = (USHORT)(-wheel * 120);
        }
        if (r.eax & VMWARE_RELATIVE_PACKET) {
            /* Host pointer is locked: relative motion, positive y is up */
            m->Flags = MOUSE_MOVE_RELATIVE;
            m->LastX = (LONG)r.ebx;
            m->LastY = -(LONG)r.ecx;
            return;
        }
        ext->x = (LONG)r.ebx;
        ext->y = (LONG)r.ecx;
    }
    m->LastX = ext->x;
    m->LastY = ext->y;
}

static NTSTATUS NTAPI read_done(PDEVICE_OBJECT dev, PIRP irp, PVOID ctx)
{
    (void)ctx;
    if (irp->PendingReturned) IoMarkIrpPending(irp);
    if (!g_host || !NT_SUCCESS(irp->IoStatus.Status)) return STATUS_CONTINUE_COMPLETION;

    ULONG status = v86_call(VMWARE_CMD_ABSPOINTER_STATUS, 0);
    if ((status & 0xFFFF0000) == 0xFFFF0000) {
        /* Absolute mode is off (or the host dropped it): pass PS/2 through */
        enable_absolute();
        return STATUS_CONTINUE_COMPLETION;
    }
    /* Emit every queued packet (v86 may merge PS/2 packets), padding with no-ops */
    FILTER_EXT *ext = dev->DeviceExtension;
    PMOUSE_INPUT_DATA data = irp->AssociatedIrp.SystemBuffer;
    ULONG count = (ULONG)(irp->IoStatus.Information / sizeof(MOUSE_INPUT_DATA));
    ULONG capacity = IoGetCurrentIrpStackLocation(irp)->Parameters.Read.Length / sizeof(MOUSE_INPUT_DATA);
    ULONG packets = (status & 0xFFFF) / 4;
    ULONG total = min(max(count, packets), capacity);
    for (ULONG i = count; i < total; i++) data[i] = data[0];
    for (ULONG i = 0; i < total; i++) rewrite(ext, &data[i], i < packets);
    irp->IoStatus.Information = total * sizeof(MOUSE_INPUT_DATA);
    return STATUS_CONTINUE_COMPLETION;
}

static NTSTATUS NTAPI attributes_done(PDEVICE_OBJECT dev, PIRP irp, PVOID ctx)
{
    (void)dev;
    (void)ctx;
    if (irp->PendingReturned) IoMarkIrpPending(irp);
    if (NT_SUCCESS(irp->IoStatus.Status) && irp->IoStatus.Information >= sizeof(MOUSE_ATTRIBUTES)) {
        /* Advertise a wheel: the backdoor reports it even though PS/2 runs 3-byte */
        PMOUSE_ATTRIBUTES a = irp->AssociatedIrp.SystemBuffer;
        a->MouseIdentifier = WHEELMOUSE_I8042_HARDWARE;
        if (a->NumberOfButtons < 3) a->NumberOfButtons = 3;
    }
    return STATUS_CONTINUE_COMPLETION;
}

/* Control device: the agent's window onto the host */
static NTSTATUS control_ioctl(PIRP irp, PIO_STACK_LOCATION sp)
{
    PUCHAR sys = irp->AssociatedIrp.SystemBuffer;
    ULONG in_len = sp->Parameters.DeviceIoControl.InputBufferLength;
    ULONG out_len = sp->Parameters.DeviceIoControl.OutputBufferLength;
    if (!g_host) return STATUS_DEVICE_NOT_CONNECTED;

    switch (sp->Parameters.DeviceIoControl.IoControlCode) {
    case IOCTL_V86_GETSIZE:
        if (out_len < sizeof(ULONG)) return STATUS_BUFFER_TOO_SMALL;
        *(PULONG)sys = v86_call(V86_CMD_GETSIZE, 0);
        irp->IoStatus.Information = sizeof(ULONG);
        return STATUS_SUCCESS;

    case IOCTL_V86_GETTIME: {
        if (out_len < 3 * sizeof(ULONG)) return STATUS_BUFFER_TOO_SMALL;
        V86_REGS r = {0, 0, VMWARE_CMD_GETTIME, 0, 0, 0};
        v86_backdoor(&r);
        ((PULONG)sys)[0] = r.eax;
        ((PULONG)sys)[1] = r.ebx;
        ((PULONG)sys)[2] = r.edx;
        irp->IoStatus.Information = 3 * sizeof(ULONG);
        return STATUS_SUCCESS;
    }

    case IOCTL_V86_RECV: {
        if (out_len < 2 * sizeof(ULONG) || !g_buffer) return STATUS_BUFFER_TOO_SMALL;
        ULONG cap = min(out_len - 2 * sizeof(ULONG), V86_BUFFER_SIZE);
        ExAcquireFastMutex(&g_lock);
        V86_REGS r = {0, g_buffer_phys, V86_CMD_RECV, 0, cap, 0};
        v86_backdoor(&r);
        ULONG len = r.eax == V86_MSG_NONE ? 0 : min(r.ebx, cap);
        ((PULONG)sys)[0] = r.eax;
        ((PULONG)sys)[1] = len;
        RtlCopyMemory(sys + 2 * sizeof(ULONG), g_buffer, len);
        ExReleaseFastMutex(&g_lock);
        irp->IoStatus.Information = 2 * sizeof(ULONG) + len;
        return STATUS_SUCCESS;
    }

    case IOCTL_V86_SEND: {
        if (in_len < sizeof(ULONG) || !g_buffer) return STATUS_INVALID_PARAMETER;
        ULONG len = min(in_len - sizeof(ULONG), V86_BUFFER_SIZE);
        ExAcquireFastMutex(&g_lock);
        RtlCopyMemory(g_buffer, sys + sizeof(ULONG), len);
        V86_REGS r = {0, g_buffer_phys, V86_CMD_SEND, 0, len, *(PULONG)sys};
        v86_backdoor(&r);
        ExReleaseFastMutex(&g_lock);
        return STATUS_SUCCESS;
    }
    }
    return STATUS_INVALID_DEVICE_REQUEST;
}

static NTSTATUS complete(PIRP irp, NTSTATUS status)
{
    irp->IoStatus.Status = status;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    return status;
}

static NTSTATUS forward(FILTER_EXT *ext, PIRP irp, PIO_COMPLETION_ROUTINE done)
{
    IoCopyCurrentIrpStackLocationToNext(irp);
    IoSetCompletionRoutine(irp, done, NULL, TRUE, TRUE, TRUE);
    return IoCallDriver(ext->lower, irp);
}

static NTSTATUS NTAPI dispatch(PDEVICE_OBJECT dev, PIRP irp)
{
    PIO_STACK_LOCATION sp = IoGetCurrentIrpStackLocation(irp);
    if (dev == g_control) {
        irp->IoStatus.Information = 0;
        switch (sp->MajorFunction) {
        case IRP_MJ_CREATE:
        case IRP_MJ_CLOSE:
        case IRP_MJ_CLEANUP:
            return complete(irp, STATUS_SUCCESS);
        case IRP_MJ_DEVICE_CONTROL:
            return complete(irp, control_ioctl(irp, sp));
        }
        return complete(irp, STATUS_INVALID_DEVICE_REQUEST);
    }

    FILTER_EXT *ext = dev->DeviceExtension;
    switch (sp->MajorFunction) {
    case IRP_MJ_READ:
        /* Switch to absolute before the first event, so none arrives relative */
        if (g_host && !ext->started) enable_absolute();
        ext->started = TRUE;
        return forward(ext, irp, read_done);

    case IRP_MJ_DEVICE_CONTROL:
    case IRP_MJ_INTERNAL_DEVICE_CONTROL:
        if (sp->Parameters.DeviceIoControl.IoControlCode == IOCTL_MOUSE_QUERY_ATTRIBUTES)
            return forward(ext, irp, attributes_done);
        break;

    case IRP_MJ_POWER:
        PoStartNextPowerIrp(irp);
        IoSkipCurrentIrpStackLocation(irp);
        return PoCallDriver(ext->lower, irp);

    case IRP_MJ_PNP:
        if (sp->MinorFunction == IRP_MN_REMOVE_DEVICE) {
            IoSkipCurrentIrpStackLocation(irp);
            NTSTATUS status = IoCallDriver(ext->lower, irp);
            IoDetachDevice(ext->lower);
            IoDeleteDevice(dev);
            return status;
        }
        break;
    }
    IoSkipCurrentIrpStackLocation(irp);
    return IoCallDriver(ext->lower, irp);
}

static NTSTATUS NTAPI add_device(PDRIVER_OBJECT drv, PDEVICE_OBJECT pdo)
{
    PDEVICE_OBJECT dev;
    NTSTATUS status = IoCreateDevice(drv, sizeof(FILTER_EXT), NULL, FILE_DEVICE_MOUSE, 0, FALSE, &dev);
    if (!NT_SUCCESS(status)) return status;
    FILTER_EXT *ext = dev->DeviceExtension;
    RtlZeroMemory(ext, sizeof(*ext));
    ext->lower = IoAttachDeviceToDeviceStack(dev, pdo);
    if (!ext->lower) {
        IoDeleteDevice(dev);
        return STATUS_DEVICE_NOT_CONNECTED;
    }
    dev->Flags |= ext->lower->Flags & (DO_BUFFERED_IO | DO_DIRECT_IO | DO_POWER_PAGABLE);
    dev->Flags &= ~DO_DEVICE_INITIALIZING;
    return STATUS_SUCCESS;
}

NTSTATUS NTAPI DriverEntry(PDRIVER_OBJECT drv, PUNICODE_STRING path)
{
    (void)path;
    V86_REGS r = {0, 0, VMWARE_CMD_GETVERSION, 0, 0, 0};
    v86_backdoor(&r);
    g_host = r.ebx == VMWARE_MAGIC;

    for (ULONG i = 0; i <= IRP_MJ_MAXIMUM_FUNCTION; i++) drv->MajorFunction[i] = dispatch;
    drv->DriverExtension->AddDevice = add_device;

    ExInitializeFastMutex(&g_lock);
    PHYSICAL_ADDRESS highest;
    highest.QuadPart = 0x7FFFFFFF;
    g_buffer = MmAllocateContiguousMemory(V86_BUFFER_SIZE, highest);
    if (g_buffer) g_buffer_phys = MmGetPhysicalAddress(g_buffer).LowPart;

    UNICODE_STRING name, link;
    RtlInitUnicodeString(&name, V86_DEVICE_NAME);
    RtlInitUnicodeString(&link, V86_DOS_NAME);
    if (NT_SUCCESS(IoCreateDevice(drv, 0, &name, FILE_DEVICE_UNKNOWN, 0, FALSE, &g_control))) {
        IoCreateSymbolicLink(&link, &name);
        g_control->Flags |= DO_BUFFERED_IO;
        g_control->Flags &= ~DO_DEVICE_INITIALIZING;
    }
    return STATUS_SUCCESS;
}
