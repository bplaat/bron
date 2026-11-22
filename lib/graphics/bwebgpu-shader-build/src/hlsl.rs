/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::path::Path;

use crate::Result;
pub(crate) use crate::hlsl::platform::compile;

#[cfg(target_os = "windows")]
#[allow(unsafe_code, clippy::undocumented_unsafe_blocks)]
mod platform {
    use std::ffi::{CStr, CString, c_void};
    use std::fs;
    use std::ptr::NonNull;

    use super::*;
    use crate::output::write_if_changed;

    pub(crate) fn compile(
        source: &Path,
        out: &Path,
        name: &str,
        backend: &str,
        shader_model: &str,
        entries: &[(&str, &str, naga::ShaderStage)],
    ) -> Result<Vec<(String, String)>> {
        let source_bytes = fs::read(source)
            .map_err(|e| format!("{}: read generated HLSL: {e}", source.display()))?;
        let source_name = CString::new(source.to_string_lossy().as_bytes())?;
        let compiler = Compiler::new()?;
        entries
            .iter()
            .map(|(_, entry, stage)| {
                let prefix = match stage {
                    naga::ShaderStage::Vertex => "vs",
                    naga::ShaderStage::Fragment => "ps",
                    naga::ShaderStage::Compute => "cs",
                    _ => {
                        return Err("Direct3D 12 shader stage is unsupported".into());
                    }
                };
                let profile = CString::new(format!("{prefix}_{shader_model}"))?;
                let bytes = compiler.compile(&source_bytes, &source_name, entry, &profile)?;
                let relative = format!("/{name}.{backend}.{entry}.dxbc");
                write_if_changed(&out.join(&relative[1..]), &bytes)?;
                Ok((format!("{backend}:{entry}"), relative))
            })
            .collect()
    }

    /// Encode a string literal as a null-terminated UTF-16 string at compile time.
    macro_rules! w {
        ($text:literal) => {{
            const TEXT: &str = $text;
            const LEN: usize = {
                let bytes = TEXT.as_bytes();
                let mut len = 1;
                let mut i = 0;
                while i < bytes.len() {
                    if bytes[i] & 0xc0 != 0x80 {
                        len += if bytes[i] >= 0xf0 { 2 } else { 1 };
                    }
                    i += 1;
                }
                len
            };
            const WIDE: &[u16] = &{
                let bytes = TEXT.as_bytes();
                let mut result = [0; LEN];
                let mut i = 0;
                let mut j = 0;
                while i < bytes.len() {
                    let first = bytes[i];
                    let (mut code, extra) = match first {
                        0..=0x7f => (first as u32, 0),
                        0xc0..=0xdf => ((first & 0x1f) as u32, 1),
                        0xe0..=0xef => ((first & 0x0f) as u32, 2),
                        _ => ((first & 0x07) as u32, 3),
                    };
                    let end = i + extra + 1;
                    i += 1;
                    while i < end {
                        code = (code << 6) | (bytes[i] & 0x3f) as u32;
                        i += 1;
                    }
                    assert!(code != 0, "wide string literal contains a null character");
                    if code > 0xffff {
                        code -= 0x10000;
                        result[j] = 0xd800 | (code >> 10) as u16;
                        j += 1;
                        result[j] = 0xdc00 | (code & 0x3ff) as u16;
                    } else {
                        result[j] = code as u16;
                    }
                    j += 1;
                }
                result
            };
            WIDE
        }};
    }

    struct Compiler {
        module: *mut c_void,
        compile: D3DCompile,
    }

    impl Compiler {
        fn new() -> Result<Self> {
            let name = w!("d3dcompiler_47.dll");
            unsafe {
                let module = LoadLibraryW(name.as_ptr());
                if module.is_null() {
                    return Err("d3dcompiler_47.dll is required for HLSL compilation".into());
                }
                let function = GetProcAddress(module, c"D3DCompile".as_ptr());
                if function.is_null() {
                    FreeLibrary(module);
                    return Err("D3DCompile is unavailable".into());
                }
                Ok(Self {
                    module,
                    compile: std::mem::transmute::<*const c_void, D3DCompile>(function),
                })
            }
        }

        fn compile(
            &self,
            source: &[u8],
            source_name: &CStr,
            entry: &str,
            profile: &CStr,
        ) -> Result<Vec<u8>> {
            let entry = CString::new(entry)?;
            unsafe {
                let mut blob = std::ptr::null_mut();
                let mut errors = std::ptr::null_mut();
                let result = (self.compile)(
                    source.as_ptr().cast(),
                    source.len(),
                    source_name.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    entry.as_ptr(),
                    profile.as_ptr(),
                    1 << 15,
                    0,
                    &mut blob,
                    &mut errors,
                );
                let errors = ComPtr::from_raw(errors);
                let blob = ComPtr::from_raw(blob);
                if result < 0 {
                    let message = errors
                        .as_ref()
                        .map(|value| String::from_utf8_lossy(value.bytes()).into_owned())
                        .unwrap_or_else(|| format!("HLSL compilation failed: {result}"));
                    return Err(message.into());
                }
                let blob = blob.ok_or("HLSL compiler returned null")?;
                Ok(blob.bytes().to_vec())
            }
        }
    }

    impl Drop for Compiler {
        fn drop(&mut self) {
            unsafe {
                FreeLibrary(self.module);
            }
        }
    }

    struct ComPtr<T>(NonNull<T>);

    impl<T> ComPtr<T> {
        // The pointer must transfer one owned reference.
        unsafe fn from_raw(pointer: *mut T) -> Option<Self> {
            NonNull::new(pointer).map(Self)
        }

        fn unknown(&self) -> &IUnknownVtbl {
            unsafe { &**self.0.as_ptr().cast::<*const IUnknownVtbl>() }
        }
    }

    impl<T> Clone for ComPtr<T> {
        fn clone(&self) -> Self {
            unsafe { (self.unknown().add_ref)(self.0.as_ptr().cast()) };
            Self(self.0)
        }
    }

    impl<T> Drop for ComPtr<T> {
        fn drop(&mut self) {
            unsafe { (self.unknown().release)(self.0.as_ptr().cast()) };
        }
    }

    impl ComPtr<ID3DBlob> {
        fn bytes(&self) -> &[u8] {
            let blob = self.0.as_ptr();
            unsafe {
                let vtbl = &*(*blob).vtbl;
                std::slice::from_raw_parts(
                    (vtbl.get_buffer_pointer)(blob).cast(),
                    (vtbl.get_buffer_size)(blob),
                )
            }
        }
    }

    #[repr(C)]
    struct Guid {
        data1: u32,
        data2: u16,
        data3: u16,
        data4: [u8; 8],
    }

    #[repr(C)]
    struct IUnknownVtbl {
        query_interface:
            unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32,
        add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
        release: unsafe extern "system" fn(*mut c_void) -> u32,
    }

    #[repr(C)]
    struct ID3DBlob {
        vtbl: *const ID3DBlobVtbl,
    }

    #[repr(C)]
    struct ID3DBlobVtbl {
        base: IUnknownVtbl,
        get_buffer_pointer: unsafe extern "system" fn(*mut ID3DBlob) -> *mut c_void,
        get_buffer_size: unsafe extern "system" fn(*mut ID3DBlob) -> usize,
    }

    type D3DCompile = unsafe extern "system" fn(
        *const c_void,
        usize,
        *const i8,
        *const c_void,
        *mut c_void,
        *const i8,
        *const i8,
        u32,
        u32,
        *mut *mut ID3DBlob,
        *mut *mut ID3DBlob,
    ) -> i32;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LoadLibraryW(name: *const u16) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const i8) -> *const c_void;
        fn FreeLibrary(module: *mut c_void) -> i32;
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use super::*;

    pub(crate) fn compile(
        _: &Path,
        _: &Path,
        _: &str,
        _: &str,
        _: &str,
        _: &[(&str, &str, naga::ShaderStage)],
    ) -> Result<Vec<(String, String)>> {
        Err("HLSL bytecode compilation requires a Windows host".into())
    }
}
