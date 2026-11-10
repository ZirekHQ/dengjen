use std::ffi::c_void;

use windows::Win32::Foundation::{CLASS_E_NOAGGREGATION, E_POINTER};
use windows::Win32::Media::Speech::ISpTTSEngine;
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};
use windows_core::{implement, IUnknown, Interface, Ref, BOOL, GUID};

use super::guard;
use super::voice::Voice;

#[implement(IClassFactory)]
pub(super) struct Factory;

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(
        &self,
        outer: Ref<IUnknown>,
        interface_id: *const GUID,
        out: *mut *mut c_void,
    ) -> windows_core::Result<()> {
        guard(|| {
            if out.is_null() {
                return Err(E_POINTER.into());
            }
            // SAFETY: `out` is non-null and COM's caller passes writable storage for it.
            unsafe { out.write(std::ptr::null_mut()) };
            if outer.is_some() {
                return Err(CLASS_E_NOAGGREGATION.into());
            }
            let voice: ISpTTSEngine = Voice::default().into();
            // SAFETY: `interface_id` and `out` come from COM's caller and satisfy `query`'s
            // contract: a GUID to look up and a slot for the resulting interface pointer.
            unsafe { voice.query(interface_id, out) }.ok()
        })
    }

    fn LockServer(&self, _lock: BOOL) -> windows_core::Result<()> {
        Ok(())
    }
}
