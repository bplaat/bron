// The emulated machine, shared by the site and the offline/ build: a snapshot only restores on the same machine
export const DISK_CHUNK_SIZE = 1024 * 1024;

export const MACHINE = {
    memory_size: 256 * 1024 * 1024,
    vga_memory_size: 16 * 1024 * 1024,
    acpi: false, // XP runs the Standard PC HAL
    cpuid_level: 2,
    net_device: { type: 'none' }, // no network for now, v86 adds an NE2000 by default
};

