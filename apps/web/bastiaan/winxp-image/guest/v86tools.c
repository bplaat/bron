/*
 * v86tools.sys - host channel for the user mode guest agent.
 *
 * Exposes \\.\V86Tools for display size, clock and message transfers. Ring 3
 * code cannot touch the backdoor port under XP's IOPL, so IOCTLs proxy it here.
 */
#include <ntddk.h>

#include "v86.h"

static PVOID g_buffer;
static ULONG g_buffer_phys;
static FAST_MUTEX g_lock;
static BOOLEAN g_host;

/* Control device: the agent's window onto the host */
static NTSTATUS control_ioctl(PIRP irp, PIO_STACK_LOCATION sp) {
    PUCHAR sys = irp->AssociatedIrp.SystemBuffer;
    ULONG in_len = sp->Parameters.DeviceIoControl.InputBufferLength;
    ULONG out_len = sp->Parameters.DeviceIoControl.OutputBufferLength;
    if (!g_host)
        return STATUS_DEVICE_NOT_CONNECTED;

    switch (sp->Parameters.DeviceIoControl.IoControlCode) {
        case IOCTL_V86_GETSIZE:
            if (out_len < sizeof(ULONG))
                return STATUS_BUFFER_TOO_SMALL;
            *(PULONG)sys = v86_call(V86_CMD_GETSIZE, 0);
            irp->IoStatus.Information = sizeof(ULONG);
            return STATUS_SUCCESS;

        case IOCTL_V86_GETTIME: {
            if (out_len < 3 * sizeof(ULONG))
                return STATUS_BUFFER_TOO_SMALL;
            V86_REGS r = {0, 0, VMWARE_CMD_GETTIME, 0, 0, 0};
            v86_backdoor(&r);
            ((PULONG)sys)[0] = r.eax;
            ((PULONG)sys)[1] = r.ebx;
            ((PULONG)sys)[2] = r.edx;
            irp->IoStatus.Information = 3 * sizeof(ULONG);
            return STATUS_SUCCESS;
        }

        case IOCTL_V86_RECV: {
            if (out_len < 2 * sizeof(ULONG) || !g_buffer)
                return STATUS_BUFFER_TOO_SMALL;
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
            if (in_len < sizeof(ULONG) || !g_buffer)
                return STATUS_INVALID_PARAMETER;
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

static NTSTATUS complete(PIRP irp, NTSTATUS status) {
    irp->IoStatus.Status = status;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    return status;
}

static NTSTATUS NTAPI dispatch(PDEVICE_OBJECT dev, PIRP irp) {
    (void)dev;
    PIO_STACK_LOCATION sp = IoGetCurrentIrpStackLocation(irp);
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

static void NTAPI unload(PDRIVER_OBJECT drv) {
    UNICODE_STRING link;
    RtlInitUnicodeString(&link, V86_DOS_NAME);
    IoDeleteSymbolicLink(&link);
    IoDeleteDevice(drv->DeviceObject);
    MmFreeContiguousMemory(g_buffer);
}

NTSTATUS NTAPI DriverEntry(PDRIVER_OBJECT drv, PUNICODE_STRING path) {
    (void)path;
    V86_REGS r = {0, 0, VMWARE_CMD_GETVERSION, 0, 0, 0};
    v86_backdoor(&r);
    g_host = r.ebx == VMWARE_MAGIC;

    for (ULONG i = 0; i <= IRP_MJ_MAXIMUM_FUNCTION; i++)
        drv->MajorFunction[i] = dispatch;
    drv->DriverUnload = unload;

    ExInitializeFastMutex(&g_lock);
    PHYSICAL_ADDRESS highest;
    highest.QuadPart = 0x7FFFFFFF;
    g_buffer = MmAllocateContiguousMemory(V86_BUFFER_SIZE, highest);
    if (!g_buffer)
        return STATUS_INSUFFICIENT_RESOURCES;
    g_buffer_phys = MmGetPhysicalAddress(g_buffer).LowPart;

    UNICODE_STRING name, link;
    RtlInitUnicodeString(&name, V86_DEVICE_NAME);
    RtlInitUnicodeString(&link, V86_DOS_NAME);
    PDEVICE_OBJECT dev;
    NTSTATUS status = IoCreateDevice(drv, 0, &name, FILE_DEVICE_UNKNOWN, 0, FALSE, &dev);
    if (!NT_SUCCESS(status)) {
        MmFreeContiguousMemory(g_buffer);
        return status;
    }
    status = IoCreateSymbolicLink(&link, &name);
    if (!NT_SUCCESS(status)) {
        IoDeleteDevice(dev);
        MmFreeContiguousMemory(g_buffer);
        return status;
    }
    dev->Flags |= DO_BUFFERED_IO;
    dev->Flags &= ~DO_DEVICE_INITIALIZING;
    return STATUS_SUCCESS;
}
