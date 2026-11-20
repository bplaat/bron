# Checks Explorer's real property-loading path after installing ImageView.
# Run in a fresh PowerShell process to avoid cached COM registrations.
param([Parameter(Mandatory)][string]$Path)

$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class ImageViewInstalledProperties
{
    [StructLayout(LayoutKind.Sequential)]
    public struct PropertyKey { public Guid Format; public uint Id; }

    [StructLayout(LayoutKind.Explicit, Size = 24)]
    public struct PropVariant
    {
        [FieldOffset(0)] public ushort Type;
        [FieldOffset(8)] public IntPtr Pointer;
        [FieldOffset(16)] public IntPtr Padding;
    }

    [ComImport, Guid("886d8eeb-8cf2-4446-8d02-cdba1dbdcf99")]
    [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    public interface IPropertyStore
    {
        [PreserveSig] int GetCount(out uint count);
        [PreserveSig] int GetAt(uint index, out PropertyKey key);
        [PreserveSig] int GetValue(ref PropertyKey key, out PropVariant value);
        [PreserveSig] int SetValue(ref PropertyKey key, ref PropVariant value);
        [PreserveSig] int Commit();
    }

    [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
    static extern int SHGetPropertyStoreFromParsingName(string path, IntPtr context,
        uint flags, ref Guid iid, out IPropertyStore store);
    [DllImport("propsys.dll", CharSet = CharSet.Unicode)]
    static extern int PSGetPropertyKeyFromName(string name, out PropertyKey key);
    [DllImport("propsys.dll", CharSet = CharSet.Unicode)]
    static extern int PropVariantToString(ref PropVariant value, StringBuilder text, uint length);
    [DllImport("ole32.dll")]
    static extern int PropVariantClear(ref PropVariant value);

    public static void Check(string path)
    {
        // GPS_DEFAULT exercises the same aggregate store as Explorer. Calling the DLL's
        // factory directly misses registration failures that blank the entire Details tab.
        Guid iid = typeof(IPropertyStore).GUID;
        IPropertyStore store;
        Marshal.ThrowExceptionForHR(SHGetPropertyStoreFromParsingName(
            path, IntPtr.Zero, 0, ref iid, out store));
        try
        {
            foreach (string name in new[] { "System.Size", "System.Image.Dimensions",
                "System.Image.HorizontalSize", "System.Image.VerticalSize" })
            {
                PropertyKey key;
                Marshal.ThrowExceptionForHR(PSGetPropertyKeyFromName(name, out key));
                PropVariant value;
                Marshal.ThrowExceptionForHR(store.GetValue(ref key, out value));
                try
                {
                    if (value.Type == 0)
                        throw new InvalidOperationException(name + " is empty");
                    var text = new StringBuilder(512);
                    Marshal.ThrowExceptionForHR(PropVariantToString(ref value, text, 512));
                    Console.WriteLine("{0}: {1}", name, text);
                }
                finally { PropVariantClear(ref value); }
            }
        }
        finally { Marshal.ReleaseComObject(store); }
    }
}
'@

[ImageViewInstalledProperties]::Check((Resolve-Path -LiteralPath $Path).Path)
