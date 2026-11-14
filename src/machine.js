// The emulated machine, shared by the site and the offline/ build: a snapshot only restores on the same machine
export const DISK_CHUNK_SIZE = 1024 * 1024;

export const MACHINE = {
    memory_size: 256 * 1024 * 1024,
    vga_memory_size: 16 * 1024 * 1024,
    acpi: false, // XP runs the Standard PC HAL
    cpuid_level: 2,
    net_device: { type: 'ne2k' }, // XP's in-box RTL8029 driver, the site connects it to the server's LAN relay
};

// XP's RTL8029 driver selects register page 3 (Realtek's own, which v86 does not have) and then writes 0xFF, landing
// in PSTART on page 0: a receive ring that starts past its end (PSTOP) drops every packet, keep the one set up before
export function patchNetworkCard(emulator) {
    const net = emulator.v86.cpu.devices.net;
    if (!net) return;
    const port = emulator.v86.cpu.io.ports[net.port + 1];
    const write = port.write8;
    port.write8 = function (value) {
        if ((net.cr & 0xc0) === 0 && value >= net.pstop) return;
        write.call(this, value);
    };
}
