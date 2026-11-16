/*
 * v86vid.sys - video miniport for the Bochs VBE (dispi) adapter v86 emulates.
 *
 * Pairs with XP's stock framebuf.dll. Common sizes in 8 bit (palette), 16 and
 * 32 bit colour, so DirectDraw games find their fullscreen modes, plus one extra
 * 32 bit mode: the size the page currently wants (asked over the backdoor),
 * which is how the desktop follows the browser window.
 *
 * Hardware pointer: framebuf.dll hands monochrome pointers of up to 32x32 to
 * the miniport, which passes them to the page; the page shows them as the
 * browser's own cursor, so it moves at native speed and Windows no longer draws
 * it. Colour pointers and shapes the page declines stay drawn by Windows.
 */
#include <ntdef.h>
#include <winerror.h>
#include <devioctl.h>
#include <miniport.h>
#include <ntddvdeo.h>
#include <video.h>
#include "v86.h"

#define DISPI_INDEX ((PUSHORT)0x1CE)
#define DISPI_DATA ((PUSHORT)0x1CF)
#define DISPI_ID 0
#define DISPI_XRES 1
#define DISPI_YRES 2
#define DISPI_BPP 3
#define DISPI_ENABLE 4
#define DISPI_VIDEO_MEMORY_64K 0xA
#define DISPI_ENABLED 0x01
#define DISPI_8BIT_DAC 0x20
#define DISPI_LFB 0x40
#define VGA_DAC_WRITE ((PUCHAR)0x3C8)
#define VGA_DAC_DATA ((PUCHAR)0x3C9)

#define MAX_MODES 64
#define POINTER_SIZE 32
#define POINTER_MASKS (2 * (POINTER_SIZE / 8) * POINTER_SIZE) /* AND then XOR mask, 1 bit per pixel */

typedef struct {
    USHORT width, height, bpp;
} MODE;

typedef struct {
    PHYSICAL_ADDRESS lfb;
    ULONG vram;
    MODE modes[MAX_MODES];
    ULONG mode_count;
    MODE current;
    BOOLEAN host;           /* the page answers the v86 backdoor commands */
    BOOLEAN pointer_shown;  /* the page accepted the last shape, kept below for when it asks again */
    union {
        VIDEO_POINTER_ATTRIBUTES attr;
        UCHAR bytes[sizeof(VIDEO_POINTER_ATTRIBUTES) + POINTER_MASKS];
    } pointer;
} DEVICE_EXT;

static const MODE std_modes[] = {
    {640, 480, 0}, {800, 600, 0}, {1024, 768, 0}, {1152, 864, 0}, {1280, 720, 0}, {1280, 800, 0}, {1280, 1024, 0},
    {1366, 768, 0}, {1440, 900, 0}, {1600, 900, 0}, {1600, 1200, 0}, {1680, 1050, 0}, {1920, 1080, 0},
    {1920, 1200, 0}, {2560, 1440, 0}, {2560, 1600, 0},
};
static const USHORT depths[] = {32, 16, 8};

static void dispi_write(USHORT index, USHORT value)
{
    VideoPortWritePortUshort(DISPI_INDEX, index);
    VideoPortWritePortUshort(DISPI_DATA, value);
}

static USHORT dispi_read(USHORT index)
{
    VideoPortWritePortUshort(DISPI_INDEX, index);
    return VideoPortReadPortUshort(DISPI_DATA);
}

static BOOLEAN host_present(void)
{
    V86_REGS r = {0, 0, VMWARE_CMD_GETVERSION, 0, 0, 0};
    v86_backdoor(&r);
    return r.ebx == VMWARE_MAGIC;
}

/* The page reads the shape from the device extension, which is nonpaged */
static BOOLEAN pointer_shape(DEVICE_EXT *ext)
{
    ext->pointer_shown = v86_call(V86_CMD_POINTER_SHAPE, (ULONG)&ext->pointer.attr) == 1;
    return ext->pointer_shown;
}

static void pointer_move(DEVICE_EXT *ext, ULONG position)
{
    if (v86_call(V86_CMD_POINTER_MOVE, position) == V86_POINTER_RESEND && ext->pointer_shown) pointer_shape(ext);
}

static ULONG pointer_position(SHORT column, SHORT row)
{
    return ((ULONG)(USHORT)column << 16) | (USHORT)row;
}

static BOOLEAN mode_fits(DEVICE_EXT *ext, ULONG w, ULONG h, ULONG bpp)
{
    return w >= 320 && h >= 240 && w <= 2560 && h <= 1600 && w * h * (bpp / 8) <= ext->vram;
}

static void add_mode(DEVICE_EXT *ext, ULONG w, ULONG h, ULONG bpp)
{
    if (!mode_fits(ext, w, h, bpp) || ext->mode_count == MAX_MODES) return;
    for (ULONG i = 0; i < ext->mode_count; i++)
        if (ext->modes[i].width == w && ext->modes[i].height == h && ext->modes[i].bpp == bpp) return;
    ext->modes[ext->mode_count].width = (USHORT)w;
    ext->modes[ext->mode_count].height = (USHORT)h;
    ext->modes[ext->mode_count].bpp = (USHORT)bpp;
    ext->mode_count++;
}

/* Rebuilt on every mode query, so the host's current size is always offered */
static void build_modes(DEVICE_EXT *ext)
{
    ext->mode_count = 0;
    for (ULONG d = 0; d < sizeof(depths) / sizeof(depths[0]); d++)
        for (ULONG i = 0; i < sizeof(std_modes) / sizeof(std_modes[0]); i++)
            add_mode(ext, std_modes[i].width, std_modes[i].height, depths[d]);
    if (ext->host) {
        ULONG size = v86_call(V86_CMD_GETSIZE, 0);
        if (size) add_mode(ext, size >> 16, size & 0xFFFF, 32);
    }
    if (ext->current.width) add_mode(ext, ext->current.width, ext->current.height, ext->current.bpp);
}

static void fill_mode(ULONG index, MODE *m, PVIDEO_MODE_INFORMATION info)
{
    VideoPortZeroMemory(info, sizeof(*info));
    info->Length = sizeof(*info);
    info->ModeIndex = index;
    info->VisScreenWidth = m->width;
    info->VisScreenHeight = m->height;
    info->ScreenStride = m->width * (m->bpp / 8);
    info->NumberOfPlanes = 1;
    info->BitsPerPlane = m->bpp;
    info->Frequency = 60;
    info->XMillimeter = 320;
    info->YMillimeter = 240;
    info->AttributeFlags = VIDEO_MODE_COLOR | VIDEO_MODE_GRAPHICS;
    if (m->bpp == 8) {
        /* The VGA DAC holds the palette, switched to 8 bits per channel */
        info->NumberRedBits = info->NumberGreenBits = info->NumberBlueBits = 8;
        info->AttributeFlags |= VIDEO_MODE_PALETTE_DRIVEN | VIDEO_MODE_MANAGED_PALETTE;
    } else if (m->bpp == 16) {
        info->NumberRedBits = info->NumberBlueBits = 5;
        info->NumberGreenBits = 6;
        info->RedMask = 0xF800;
        info->GreenMask = 0x07E0;
        info->BlueMask = 0x001F;
    } else {
        info->NumberRedBits = info->NumberGreenBits = info->NumberBlueBits = 8;
        info->RedMask = 0xFF0000;
        info->GreenMask = 0x00FF00;
        info->BlueMask = 0x0000FF;
    }
    info->VideoMemoryBitmapWidth = m->width;
    info->VideoMemoryBitmapHeight = m->height;
}

static void set_mode(MODE *m)
{
    dispi_write(DISPI_ENABLE, 0);
    dispi_write(DISPI_BPP, m->bpp);
    dispi_write(DISPI_XRES, m->width);
    dispi_write(DISPI_YRES, m->height);
    dispi_write(DISPI_ENABLE, DISPI_ENABLED | DISPI_LFB | (m->bpp == 8 ? DISPI_8BIT_DAC : 0));
}

static VP_STATUS NTAPI find_adapter(PVOID hw, PVOID ctx, PWSTR args, PVIDEO_PORT_CONFIG_INFO config, PUCHAR again)
{
    (void)ctx;
    (void)args;
    DEVICE_EXT *ext = hw;
    *again = FALSE;
    USHORT id = dispi_read(DISPI_ID);
    if (id < 0xB0C0 || id > 0xB0C5) return ERROR_DEV_NOT_EXIST;

    VIDEO_ACCESS_RANGE ranges[4];
    VideoPortZeroMemory(ranges, sizeof(ranges));
    ULONG vendor = 0x1234, device = 0x1111, slot = 0;
    ext->lfb.QuadPart = 0xE0000000;
    if (VideoPortGetAccessRanges(hw, 0, NULL, 4, ranges, &vendor, &device, &slot) == NO_ERROR) {
        for (ULONG i = 0; i < 4; i++) {
            if (ranges[i].RangeLength && !ranges[i].RangeInIoSpace) {
                ext->lfb = ranges[i].RangeStart;
                break;
            }
        }
    }
    ext->vram = (ULONG)dispi_read(DISPI_VIDEO_MEMORY_64K) * 0x10000;
    if (ext->vram < 0x400000) ext->vram = 0x400000;
    ext->host = host_present() && v86_call(V86_CMD_HELLO, 0) == V86_HELLO_REPLY;

    config->NumEmulatorAccessEntries = 0;
    config->EmulatorAccessEntries = NULL;
    config->VdmPhysicalVideoMemoryAddress.QuadPart = 0;
    config->VdmPhysicalVideoMemoryLength = 0;
    config->HardwareStateSize = 0;
    return NO_ERROR;
}

static BOOLEAN NTAPI initialize(PVOID hw)
{
    build_modes(hw);
    return TRUE;
}

static BOOLEAN NTAPI start_io(PVOID hw, PVIDEO_REQUEST_PACKET rp)
{
    DEVICE_EXT *ext = hw;
    VP_STATUS status = ERROR_INVALID_FUNCTION;
    rp->StatusBlock->Information = 0;

    switch (rp->IoControlCode) {
    case IOCTL_VIDEO_QUERY_NUM_AVAIL_MODES: {
        if (rp->OutputBufferLength < sizeof(VIDEO_NUM_MODES)) { status = ERROR_INSUFFICIENT_BUFFER; break; }
        build_modes(ext);
        PVIDEO_NUM_MODES num = rp->OutputBuffer;
        num->NumModes = ext->mode_count;
        num->ModeInformationLength = sizeof(VIDEO_MODE_INFORMATION);
        rp->StatusBlock->Information = sizeof(VIDEO_NUM_MODES);
        status = NO_ERROR;
        break;
    }
    case IOCTL_VIDEO_QUERY_AVAIL_MODES: {
        ULONG need = ext->mode_count * sizeof(VIDEO_MODE_INFORMATION);
        if (rp->OutputBufferLength < need) { status = ERROR_INSUFFICIENT_BUFFER; break; }
        PVIDEO_MODE_INFORMATION info = rp->OutputBuffer;
        for (ULONG i = 0; i < ext->mode_count; i++) fill_mode(i, &ext->modes[i], &info[i]);
        rp->StatusBlock->Information = need;
        status = NO_ERROR;
        break;
    }
    case IOCTL_VIDEO_SET_CURRENT_MODE: {
        if (rp->InputBufferLength < sizeof(VIDEO_MODE)) { status = ERROR_INSUFFICIENT_BUFFER; break; }
        ULONG index = ((PVIDEO_MODE)rp->InputBuffer)->RequestedMode & 0x0FFFFFFF;
        if (index >= ext->mode_count) { status = ERROR_INVALID_PARAMETER; break; }
        ext->current = ext->modes[index];
        set_mode(&ext->current);
        status = NO_ERROR;
        break;
    }
    case IOCTL_VIDEO_QUERY_CURRENT_MODE: {
        if (rp->OutputBufferLength < sizeof(VIDEO_MODE_INFORMATION)) { status = ERROR_INSUFFICIENT_BUFFER; break; }
        ULONG index = 0;
        for (ULONG i = 0; i < ext->mode_count; i++)
            if (ext->modes[i].width == ext->current.width && ext->modes[i].height == ext->current.height &&
                ext->modes[i].bpp == ext->current.bpp)
                index = i;
        fill_mode(index, &ext->current, rp->OutputBuffer);
        rp->StatusBlock->Information = sizeof(VIDEO_MODE_INFORMATION);
        status = NO_ERROR;
        break;
    }
    case IOCTL_VIDEO_MAP_VIDEO_MEMORY: {
        if (rp->InputBufferLength < sizeof(VIDEO_MEMORY) ||
            rp->OutputBufferLength < sizeof(VIDEO_MEMORY_INFORMATION)) { status = ERROR_INSUFFICIENT_BUFFER; break; }
        PVIDEO_MEMORY_INFORMATION mem = rp->OutputBuffer;
        PVOID base = ((PVIDEO_MEMORY)rp->InputBuffer)->RequestedVirtualAddress;
        ULONG length = ext->vram, in_io = VIDEO_MEMORY_SPACE_MEMORY;
        status = VideoPortMapMemory(hw, ext->lfb, &length, &in_io, &base);
        if (status != NO_ERROR) break;
        mem->VideoRamBase = mem->FrameBufferBase = base;
        mem->VideoRamLength = length;
        mem->FrameBufferLength = (ULONG)ext->current.width * ext->current.height * (ext->current.bpp / 8);
        rp->StatusBlock->Information = sizeof(VIDEO_MEMORY_INFORMATION);
        break;
    }
    case IOCTL_VIDEO_SET_COLOR_REGISTERS: {
        PVIDEO_CLUT clut = rp->InputBuffer;
        if (rp->InputBufferLength < sizeof(VIDEO_CLUT) - sizeof(ULONG) ||
            rp->InputBufferLength < sizeof(VIDEO_CLUT) - sizeof(ULONG) + clut->NumEntries * sizeof(ULONG) ||
            clut->FirstEntry + clut->NumEntries > 256) { status = ERROR_INVALID_PARAMETER; break; }
        for (ULONG i = 0; i < clut->NumEntries; i++) {
            VideoPortWritePortUchar(VGA_DAC_WRITE, (UCHAR)(clut->FirstEntry + i));
            VideoPortWritePortUchar(VGA_DAC_DATA, clut->LookupTable[i].RgbArray.Red);
            VideoPortWritePortUchar(VGA_DAC_DATA, clut->LookupTable[i].RgbArray.Green);
            VideoPortWritePortUchar(VGA_DAC_DATA, clut->LookupTable[i].RgbArray.Blue);
        }
        status = NO_ERROR;
        break;
    }
    case IOCTL_VIDEO_UNMAP_VIDEO_MEMORY:
        if (rp->InputBufferLength < sizeof(VIDEO_MEMORY)) { status = ERROR_INSUFFICIENT_BUFFER; break; }
        status = VideoPortUnmapMemory(hw, ((PVIDEO_MEMORY)rp->InputBuffer)->RequestedVirtualAddress, NULL);
        break;
    case IOCTL_VIDEO_QUERY_POINTER_CAPABILITIES: {
        if (!ext->host) break;
        if (rp->OutputBufferLength < sizeof(VIDEO_POINTER_CAPABILITIES)) { status = ERROR_INSUFFICIENT_BUFFER; break; }
        PVIDEO_POINTER_CAPABILITIES caps = rp->OutputBuffer;
        caps->Flags = VIDEO_MODE_ASYNC_POINTER | VIDEO_MODE_MONO_POINTER;
        caps->MaxWidth = caps->MaxHeight = POINTER_SIZE;
        caps->HWPtrBitmapStart = caps->HWPtrBitmapEnd = (ULONG)-1;
        rp->StatusBlock->Information = sizeof(VIDEO_POINTER_CAPABILITIES);
        status = NO_ERROR;
        break;
    }
    case IOCTL_VIDEO_SET_POINTER_ATTR: {
        PVIDEO_POINTER_ATTRIBUTES attr = rp->InputBuffer;
        if (!ext->host || rp->InputBufferLength < sizeof(VIDEO_POINTER_ATTRIBUTES) || attr->Width > POINTER_SIZE ||
            attr->Height > POINTER_SIZE || attr->WidthInBytes != POINTER_SIZE / 8 ||
            rp->InputBufferLength < sizeof(VIDEO_POINTER_ATTRIBUTES) + 2 * attr->WidthInBytes * attr->Height ||
            (attr->Flags & VIDEO_MODE_COLOR_POINTER)) { status = ERROR_INVALID_PARAMETER; break; }
        VideoPortZeroMemory(&ext->pointer, sizeof(ext->pointer));
        VideoPortMoveMemory(&ext->pointer, attr,
                            sizeof(VIDEO_POINTER_ATTRIBUTES) + 2 * attr->WidthInBytes * attr->Height);
        status = pointer_shape(ext) ? NO_ERROR : ERROR_INVALID_PARAMETER;
        if (status == NO_ERROR)
            pointer_move(ext, attr->Enable && (attr->Column != -1 || attr->Row != -1)
                                  ? pointer_position(attr->Column, attr->Row) : V86_POINTER_HIDDEN);
        break;
    }
    case IOCTL_VIDEO_SET_POINTER_POSITION: {
        if (rp->InputBufferLength < sizeof(VIDEO_POINTER_POSITION)) { status = ERROR_INSUFFICIENT_BUFFER; break; }
        PVIDEO_POINTER_POSITION pos = rp->InputBuffer;
        pointer_move(ext, pointer_position(pos->Column, pos->Row));
        status = NO_ERROR;
        break;
    }
    case IOCTL_VIDEO_DISABLE_POINTER:
        pointer_move(ext, V86_POINTER_HIDDEN);
        status = NO_ERROR;
        break;
    case IOCTL_VIDEO_ENABLE_POINTER:
        status = NO_ERROR; /* shown again by the next position */
        break;
    case IOCTL_VIDEO_RESET_DEVICE:
        dispi_write(DISPI_ENABLE, 0);
        status = NO_ERROR;
        break;
    }
    rp->StatusBlock->Status = status;
    return TRUE;
}

static BOOLEAN NTAPI reset_hw(PVOID hw, ULONG columns, ULONG rows)
{
    (void)hw;
    (void)columns;
    (void)rows;
    dispi_write(DISPI_ENABLE, 0);
    return FALSE; /* let the HAL finish with an int 10h text mode set */
}

static VP_STATUS NTAPI get_power(PVOID hw, ULONG id, PVIDEO_POWER_MANAGEMENT pm)
{
    (void)hw;
    (void)id;
    (void)pm;
    return NO_ERROR;
}

static VP_STATUS NTAPI set_power(PVOID hw, ULONG id, PVIDEO_POWER_MANAGEMENT pm)
{
    (void)hw;
    (void)id;
    (void)pm;
    return NO_ERROR;
}

ULONG NTAPI DriverEntry(PVOID ctx1, PVOID ctx2)
{
    VIDEO_HW_INITIALIZATION_DATA init;
    VideoPortZeroMemory(&init, sizeof(init));
    init.HwInitDataSize = sizeof(init);
    init.AdapterInterfaceType = PCIBus;
    init.HwFindAdapter = find_adapter;
    init.HwInitialize = initialize;
    init.HwStartIO = start_io;
    init.HwResetHw = reset_hw;
    init.HwGetPowerState = get_power;
    init.HwSetPowerState = set_power;
    init.HwDeviceExtensionSize = sizeof(DEVICE_EXT);
    return VideoPortInitialize(ctx1, ctx2, &init, NULL);
}
