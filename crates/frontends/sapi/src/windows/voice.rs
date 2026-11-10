//! The COM object SAPI creates for each selected voice: `ISpTTSEngine` and `ISpObjectWithToken`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use windows::Win32::Foundation::{E_FAIL, E_OUTOFMEMORY, E_POINTER};
use windows::Win32::Media::Audio::{WAVEFORMATEX, WAVE_FORMAT_PCM};
use windows::Win32::Media::Speech::{
    ISpObjectToken, ISpObjectWithToken, ISpObjectWithToken_Impl, ISpTTSEngine, ISpTTSEngineSite,
    ISpTTSEngine_Impl, SPVA_Bookmark, SPVA_Silence, SPVA_Speak, SPVA_SpellOut, SPVACTIONS,
    SPVTEXTFRAG,
};
use windows::Win32::System::Com::{CoTaskMemAlloc, CoTaskMemFree};
use windows_core::{implement, w, Ref, GUID, HRESULT};

use super::guard;
use super::site::EngineSite;
use crate::engine::{self, Engine};
use crate::fragments::{self, Action, Fragment, FragmentProsody};
use crate::registration;
use crate::speak;

/// `SPDFID_WaveFormatEx` from sapi.h: the output format is described by a `WAVEFORMATEX`.
/// `SPERR_UNINITIALIZED` from sapi.h: the voice was used before `SetObjectToken`.
const SPERR_UNINITIALIZED: HRESULT = HRESULT(0x8004_5001_u32 as i32);
const SPDFID_WAVE_FORMAT_EX: GUID = GUID::from_u128(0xC31ADBAE_527F_4FF5_A230_F62BB61FF70C);

#[derive(Default)]
struct State {
    token: Option<ISpObjectToken>,
    config_path: Option<PathBuf>,
    sample_rate: u32,
    engine: Option<Arc<Engine>>,
}

#[derive(Default)]
#[implement(ISpTTSEngine, ISpObjectWithToken)]
pub(super) struct Voice {
    state: Mutex<State>,
}

impl Voice {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The loaded voice, loading it on first use: selecting a voice should stay instant, and
    /// the model's ONNX session costs most of a second.
    fn engine(&self) -> windows_core::Result<Arc<Engine>> {
        let mut state = self.state();
        if let Some(engine) = &state.engine {
            return Ok(Arc::clone(engine));
        }
        let config_path = state
            .config_path
            .clone()
            .ok_or_else(|| windows_core::Error::from(SPERR_UNINITIALIZED))?;
        engine::init_runtime(&super::dll_dir()?).map_err(failure)?;
        let engine = Arc::new(Engine::load(&config_path).map_err(failure)?);
        state.engine = Some(Arc::clone(&engine));
        Ok(engine)
    }
}

fn failure(message: String) -> windows_core::Error {
    windows_core::Error::new(E_FAIL, message)
}

// The `windows` crate keeps sapi.h's mixed-case constant names.
#[allow(non_upper_case_globals)]
fn action(action: SPVACTIONS) -> Action {
    match action {
        SPVA_Speak => Action::Speak,
        SPVA_Silence => Action::Silence,
        SPVA_Bookmark => Action::Bookmark,
        SPVA_SpellOut => Action::SpellOut,
        _ => Action::Ignore,
    }
}

/// Copies SAPI's fragment list into owned [`Fragment`]s.
///
/// # Safety
///
/// `first` must be null or the head of a valid `SPVTEXTFRAG` list, each node's text pointing to
/// at least `ulTextLen` UTF-16 units, as SAPI guarantees for the duration of `Speak`.
unsafe fn copy_fragments(first: *const SPVTEXTFRAG) -> Vec<Fragment> {
    let mut fragments = Vec::new();
    let mut next = first;
    // SAFETY: per this function's contract, every non-null node in the list is valid to read.
    while let Some(fragment) = unsafe { next.as_ref() } {
        let text = if fragment.pTextStart.is_null() || fragment.ulTextLen == 0 {
            Vec::new()
        } else {
            // SAFETY: per this function's contract, a non-null `pTextStart` points to at least
            // `ulTextLen` UTF-16 units that stay valid while `Speak` runs.
            unsafe {
                std::slice::from_raw_parts(fragment.pTextStart.0, fragment.ulTextLen as usize)
            }
            .to_vec()
        };
        let state = &fragment.State;
        fragments.push(Fragment {
            action: action(state.eAction),
            text,
            offset: fragment.ulTextSrcOffset,
            prosody: FragmentProsody {
                rate_adj: state.RateAdj,
                volume: state.Volume,
                pitch_adj: state.PitchAdj.MiddleAdj,
            },
            silence_ms: state.SilenceMSecs,
        });
        next = fragment.pNext;
    }
    fragments
}

impl ISpTTSEngine_Impl for Voice_Impl {
    fn Speak(
        &self,
        _flags: u32,
        _format_id: *const GUID,
        _format: *const WAVEFORMATEX,
        fragments: *const SPVTEXTFRAG,
        site: Ref<ISpTTSEngineSite>,
    ) -> windows_core::Result<()> {
        guard(|| {
            let site = site.ok()?;
            // SAFETY: SAPI passes a valid fragment list that outlives this call.
            let fragments = unsafe { copy_fragments(fragments) };
            let steps = fragments::plan(&fragments);
            if steps.is_empty() {
                return Ok(());
            }
            let engine = self.engine()?;
            speak::speak(&steps, engine.as_ref(), &EngineSite::new(site))
                .map(drop)
                .map_err(failure)
        })
    }

    fn GetOutputFormat(
        &self,
        _target_id: *const GUID,
        _target: *const WAVEFORMATEX,
        format_id: *mut GUID,
        format: *mut *mut WAVEFORMATEX,
    ) -> windows_core::Result<()> {
        guard(|| {
            if format_id.is_null() || format.is_null() {
                return Err(E_POINTER.into());
            }
            let sample_rate = self.state().sample_rate;
            if sample_rate == 0 {
                return Err(SPERR_UNINITIALIZED.into());
            }
            let wave = WAVEFORMATEX {
                wFormatTag: WAVE_FORMAT_PCM as u16,
                nChannels: 1,
                nSamplesPerSec: sample_rate,
                nAvgBytesPerSec: sample_rate * 2,
                nBlockAlign: 2,
                wBitsPerSample: 16,
                cbSize: 0,
            };
            // SAFETY: SAPI frees the returned format with `CoTaskMemFree`, so it must come from
            // `CoTaskMemAlloc`; the size is that of the struct written into it below.
            let memory = unsafe { CoTaskMemAlloc(std::mem::size_of::<WAVEFORMATEX>()) };
            if memory.is_null() {
                return Err(E_OUTOFMEMORY.into());
            }
            // SAFETY: `memory` is a fresh allocation large enough for a `WAVEFORMATEX`, which is
            // `packed(1)` and so needs no alignment; both out-pointers were checked non-null and
            // SAPI passes them writable.
            unsafe {
                memory.cast::<WAVEFORMATEX>().write_unaligned(wave);
                format.write(memory.cast());
                format_id.write(SPDFID_WAVE_FORMAT_EX);
            }
            Ok(())
        })
    }
}

impl ISpObjectWithToken_Impl for Voice_Impl {
    fn SetObjectToken(&self, token: Ref<ISpObjectToken>) -> windows_core::Result<()> {
        guard(|| {
            let token = token.ok()?.clone();
            // SAFETY: a plain COM call; the value name is a static NUL-terminated string, the same
            // name as `registration::VOICE_PATH_VALUE`.
            let value = unsafe { token.GetStringValue(w!("VoicePath")) }?;
            // SAFETY: SAPI returned a NUL-terminated string allocated with `CoTaskMemAlloc`,
            // read once here and freed right after.
            let path = unsafe { value.to_string() };
            // SAFETY: `value` came from `CoTaskMemAlloc` and is not used after this.
            unsafe { CoTaskMemFree(Some(value.0.cast_const().cast())) };
            let config_path = PathBuf::from(
                path.map_err(|_| windows_core::Error::new(E_FAIL, "VoicePath is not UTF-16"))?,
            );
            let sample_rate = registration::sample_rate(&config_path).map_err(failure)?;
            let mut state = self.state();
            *state = State {
                token: Some(token),
                config_path: Some(config_path),
                sample_rate,
                engine: None,
            };
            Ok(())
        })
    }

    fn GetObjectToken(&self) -> windows_core::Result<ISpObjectToken> {
        guard(|| {
            self.state()
                .token
                .clone()
                .ok_or_else(|| SPERR_UNINITIALIZED.into())
        })
    }
}
