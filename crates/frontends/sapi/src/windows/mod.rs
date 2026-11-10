//! The COM side of the voice: DLL exports, the class factory and the SAPI interfaces.
//!
//! Every exported function and COM method runs its body through [`guard`], so a Rust panic turns
//! into `E_FAIL` for the caller instead of unwinding into Narrator's or NVDA's stack frames.

mod factory;
mod registry;
mod site;
mod voice;

use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicPtr, Ordering};

use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, E_FAIL, E_INVALIDARG, E_POINTER, HINSTANCE, HMODULE, S_FALSE, S_OK,
};
use windows::Win32::System::Com::IClassFactory;
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows_core::{Interface, BOOL, GUID, HRESULT, PCWSTR};

use crate::registration::{self, InstallOptions};

pub(crate) const CLSID_DENGJEN_VOICE: GUID =
    GUID::from_u128(0xEE94E0DE_50A8_4645_B76B_38F65AE052D4);

static MODULE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// Runs a COM entry point's body, turning a panic into `E_FAIL`.
pub(crate) fn guard<T>(body: impl FnOnce() -> windows_core::Result<T>) -> windows_core::Result<T> {
    catch_unwind(AssertUnwindSafe(body))
        .unwrap_or_else(|_| Err(windows_core::Error::new(E_FAIL, "dengjen voice panicked")))
}

fn hresult(result: windows_core::Result<()>) -> HRESULT {
    result.map_or_else(|error| error.code(), |()| S_OK)
}

/// The full path of this DLL, wherever the host loaded it from.
pub(crate) fn dll_path() -> windows_core::Result<PathBuf> {
    let module = HMODULE(MODULE.load(Ordering::Acquire));
    let mut buffer = vec![0u16; 32_768];
    // SAFETY: `buffer` is a live, writable slice and the binding passes its length; `module` is
    // the handle Windows gave `DllMain` for this DLL, which stays loaded while code in it runs.
    let len = unsafe { GetModuleFileNameW(Some(module), &mut buffer) } as usize;
    match buffer.get(..len) {
        Some(path) if len > 0 && len < buffer.len() => {
            Ok(PathBuf::from(String::from_utf16_lossy(path)))
        }
        _ => Err(windows_core::Error::from_thread()),
    }
}

pub(crate) fn dll_dir() -> windows_core::Result<PathBuf> {
    dll_path()?
        .parent()
        .map(PathBuf::from)
        .ok_or_else(|| windows_core::Error::new(E_FAIL, "the DLL path has no directory"))
}

#[no_mangle]
#[allow(non_snake_case)]
extern "system" fn DllMain(module: HINSTANCE, reason: u32, _reserved: *mut c_void) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        MODULE.store(module.0, Ordering::Release);
    }
    BOOL(1)
}

/// # Safety
///
/// `class_id` and `interface_id` must point to valid GUIDs and `out` to writable storage for an
/// interface pointer, as COM guarantees when it calls this export.
#[no_mangle]
#[allow(non_snake_case)]
unsafe extern "system" fn DllGetClassObject(
    class_id: *const GUID,
    interface_id: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: `out` is non-null and writable per this function's contract.
    unsafe { out.write(std::ptr::null_mut()) };
    // SAFETY: `class_id` points to a valid GUID per this function's contract, or is null.
    let Some(class_id) = (unsafe { class_id.as_ref() }) else {
        return E_INVALIDARG;
    };
    if *class_id != CLSID_DENGJEN_VOICE {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    let factory: IClassFactory = factory::Factory.into();
    // SAFETY: `interface_id` and `out` come straight from COM's caller and satisfy `query`'s
    // contract: a GUID to look up and a slot for the resulting interface pointer.
    unsafe { factory.query(interface_id, out) }
}

/// The DLL stays loaded for the life of the process: ONNX Runtime and eSpeak-ng keep global
/// state and threads that cannot be torn down safely.
#[no_mangle]
#[allow(non_snake_case)]
extern "system" fn DllCanUnloadNow() -> HRESULT {
    S_FALSE
}

/// `regsvr32 dengjen_sapi.dll`: machine-wide registration of the voices beside the DLL.
#[no_mangle]
#[allow(non_snake_case)]
extern "system" fn DllRegisterServer() -> HRESULT {
    hresult(guard(|| registry::register(&InstallOptions::default())))
}

#[no_mangle]
#[allow(non_snake_case)]
extern "system" fn DllUnregisterServer() -> HRESULT {
    hresult(guard(|| registry::unregister(registration::Scope::Machine)))
}

/// `regsvr32 /n /i:"user;kurdish=both" dengjen_sapi.dll` registers with options, and
/// `regsvr32 /u /n /i:user dengjen_sapi.dll` removes that registration.
///
/// # Safety
///
/// `command_line` must be null or point to a NUL-terminated UTF-16 string, as `regsvr32` passes.
#[no_mangle]
#[allow(non_snake_case)]
unsafe extern "system" fn DllInstall(install: BOOL, command_line: PCWSTR) -> HRESULT {
    let text = if command_line.is_null() {
        String::new()
    } else {
        // SAFETY: non-null and NUL-terminated per this function's contract.
        match unsafe { command_line.to_string() } {
            Ok(text) => text,
            Err(_) => return E_INVALIDARG,
        }
    };
    hresult(guard(|| {
        let options = registration::parse_install_args(&text)
            .map_err(|message| windows_core::Error::new(E_INVALIDARG, message))?;
        if install.as_bool() {
            registry::register(&options)
        } else {
            registry::unregister(options.scope)
        }
    }))
}
