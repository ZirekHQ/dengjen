use dengjen_tts::{
    default_batch_size, synthesis_thread_pool, AudioOutputConfig, DengjenSpeechSynthesizer,
    StreamMode,
};
use dengjen_tts_core::{
    panic_message, AudioSamples, CancellationToken, DengjenError, DengjenModel, DengjenResult,
};
use ffi_support::{call_with_result, define_string_destructor, ErrorCode, ExternError, FfiStr};
use std::ops::Deref;
use std::os::raw::c_void;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

#[repr(transparent)]
struct UserDataPtr(*mut c_void);

// SAFETY: UserDataPtr wraps an opaque pointer that Rust never dereferences; thread-safety is
// the FFI caller's responsibility, the same as every other raw-pointer FFI parameter.
unsafe impl Send for UserDataPtr {}

pub type SpeechSynthesisCallback = extern "C" fn(SynthesisEvent, *mut c_void) -> u8;

#[inline]
fn invoke_callback(cb: SpeechSynthesisCallback, event: SynthesisEvent, user_data: UserDataPtr) {
    cb(event, user_data.0);
}

define_string_destructor!(_internal_libdengjenFreeString);
ffi_support::implement_into_ffi_by_pointer!(DengjenVoice);
ffi_support::define_box_destructor!(DengjenVoice, _internal_libdengjenUnloadDengjenVoice);
ffi_support::implement_into_ffi_by_pointer!(PiperSynthConfig);
ffi_support::define_box_destructor!(PiperSynthConfig, _internal_libdengjenFreePiperSynthConfig);

static INIT_ORT_ENVIRONMENT: OnceLock<Result<(), String>> = OnceLock::new();

pub mod error_codes {
    pub const INVALID_SYNTHESIS_MODE: i32 = 16;
    pub const FAILED_TO_LOAD_RESOURCE: i32 = 17;
    pub const PHONEMIZATION_ERROR: i32 = 18;
    pub const OPERATION_ERROR: i32 = 19;
    pub const INVALID_UTF8_SEQUENCE: i32 = 20;
    pub const UNKNOWN_ERROR: i32 = 21;
    pub const NULL_POINTER: i32 = 22;
    pub const INFERENCE_ERROR: i32 = 23;
    pub const INVALID_CONFIGURATION: i32 = 24;
    pub const UNSUPPORTED_OPERATION: i32 = 25;
}

pub mod synth_event {
    pub const SYNTH_EVENT_SPEECH: i32 = 0;
    pub const SYNTH_EVENT_FINISHED: i32 = 1;
    pub const SYNTH_EVENT_ERROR: i32 = 2;
}

pub mod synth_mode {
    pub const SYNTH_MODE_LAZY: i32 = 0;
    pub const SYNTH_MODE_PARALLEL: i32 = 1;
    pub const SYNTH_MODE_REALTIME: i32 = 2;
    pub const SYNTH_MODE_BATCHED: i32 = 3;
}

pub const PIPER_SYNTH_CONFIG_NO_SPEAKER: u32 = u32::MAX;

pub struct DengjenVoice {
    synth: AssertUnwindSafe<Arc<DengjenSpeechSynthesizer>>,

    active_cancel_token: Arc<Mutex<Option<CancellationToken>>>,

    in_flight: Arc<InFlight>,
}

impl DengjenVoice {
    fn wrapping(synth: Arc<DengjenSpeechSynthesizer>) -> Self {
        Self {
            active_cancel_token: Arc::new(Mutex::new(None)),
            synth: AssertUnwindSafe(synth),
            in_flight: Arc::new(InFlight::default()),
        }
    }
}

impl Drop for DengjenVoice {
    fn drop(&mut self) {
        // Realtime syntheses stop through the token; every mode stops at the next chunk once
        // `closing` is set, so no nonblocking worker callback fires after unload returns, except
        // when unloading from that voice's own callback.
        let _ = _cancel(&self.active_cancel_token);
        self.in_flight.close_and_wait();
    }
}

thread_local! {
    static ACTIVE_WORKER_VOICE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Tracks nonblocking synthesis workers so unloading a voice can stop and drain them.
#[derive(Default)]
struct InFlight {
    closing: AtomicBool,
    running: Mutex<usize>,
    idle: Condvar,
}

impl InFlight {
    fn enter(self: &Arc<Self>) -> InFlightGuard {
        *self.running.lock().unwrap_or_else(|p| p.into_inner()) += 1;
        InFlightGuard(Arc::clone(self))
    }

    fn is_closing(&self) -> bool {
        self.closing.load(Ordering::Acquire)
    }

    fn close_and_wait(&self) {
        self.closing.store(true, Ordering::Release);
        // A callback that unloads its own voice runs on one of its workers; waiting there would
        // never end. Unloading a different voice from a callback still drains that voice.
        if ACTIVE_WORKER_VOICE.with(std::cell::Cell::get) == self as *const Self as usize {
            return;
        }
        let mut running = self.running.lock().unwrap_or_else(|p| p.into_inner());
        while *running > 0 {
            running = self.idle.wait(running).unwrap_or_else(|p| p.into_inner());
        }
    }
}

struct InFlightGuard(Arc<InFlight>);

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        let mut running = self.0.running.lock().unwrap_or_else(|p| p.into_inner());
        *running -= 1;
        if *running == 0 {
            self.0.idle.notify_all();
        }
    }
}

impl From<DengjenSpeechSynthesizer> for DengjenVoice {
    fn from(synthesizer: DengjenSpeechSynthesizer) -> Self {
        Self::wrapping(Arc::new(synthesizer))
    }
}

impl Deref for DengjenVoice {
    type Target = DengjenSpeechSynthesizer;

    fn deref(&self) -> &Self::Target {
        &self.synth
    }
}

impl<T> AsRef<T> for DengjenVoice
where
    T: ?Sized,
    <DengjenVoice as Deref>::Target: AsRef<T>,
{
    fn as_ref(&self) -> &T {
        (**self.synth).as_ref()
    }
}

#[derive(Debug)]
pub struct DengjenFFIError(i32, String);

impl DengjenFFIError {
    fn with_code(code: i32, message: impl Into<String>) -> Self {
        Self(code, message.into())
    }

    fn invalid_utf8() -> Self {
        Self::with_code(
            error_codes::INVALID_UTF8_SEQUENCE,
            "input string is not valid UTF-8",
        )
    }

    fn invalid_synthesis_mode() -> Self {
        Self::with_code(
            error_codes::INVALID_SYNTHESIS_MODE,
            "synthesis mode is not a recognized value",
        )
    }

    fn null_pointer(param_name: &str) -> Self {
        Self::with_code(
            error_codes::NULL_POINTER,
            format!("parameter `{param_name}` must not be null"),
        )
    }
}

impl From<DengjenError> for DengjenFFIError {
    fn from(error: DengjenError) -> Self {
        let code = match error.kind() {
            DengjenError::FailedToLoadResource(_) => error_codes::FAILED_TO_LOAD_RESOURCE,
            DengjenError::PhonemizationError(_) => error_codes::PHONEMIZATION_ERROR,
            DengjenError::InferenceError(_) => error_codes::INFERENCE_ERROR,
            DengjenError::InvalidConfiguration(_) => error_codes::INVALID_CONFIGURATION,
            DengjenError::UnsupportedOperation(_) => error_codes::UNSUPPORTED_OPERATION,
            _ => error_codes::OPERATION_ERROR,
        };
        Self::with_code(code, format!("{}{}", error.message(), error.cause_chain()))
    }
}

impl From<DengjenFFIError> for ExternError {
    fn from(error: DengjenFFIError) -> Self {
        ExternError::new_error(ErrorCode::new(error.0), error.1)
    }
}

pub type DengjenFFIResult<T> = Result<T, DengjenFFIError>;

#[repr(C)]
pub struct SynthesisEvent {
    event_type: i32,
    error_ptr: *mut ExternError,
    len: i64,
    data: *mut u8,
}

impl SynthesisEvent {
    fn leak_bytes(bytes: Vec<u8>) -> (i64, *mut u8) {
        let boxed: Box<[u8]> = bytes.into_boxed_slice();
        let len = boxed.len() as i64;
        let ptr = Box::into_raw(boxed) as *mut u8;
        (len, ptr)
    }

    fn with_speech(speech: Vec<u8>) -> Self {
        let (len, data) = Self::leak_bytes(speech);
        Self {
            event_type: synth_event::SYNTH_EVENT_SPEECH,
            error_ptr: std::ptr::null_mut(),
            len,
            data,
        }
    }

    fn with_error(error: impl Into<ExternError>) -> Self {
        let (len, data) = Self::leak_bytes(Vec::new());
        Self {
            event_type: synth_event::SYNTH_EVENT_ERROR,
            error_ptr: Box::into_raw(Box::new(error.into())),
            len,
            data,
        }
    }

    fn with_finished() -> Self {
        let (len, data) = Self::leak_bytes(Vec::new());
        Self {
            event_type: synth_event::SYNTH_EVENT_FINISHED,
            error_ptr: std::ptr::null_mut(),
            len,
            data,
        }
    }
}

#[repr(C)]
pub struct AudioInfo {
    sample_rate: u32,
    num_channels: u32,
    sample_width: u32,
}

/// Parameters for [`libdengjenSpeak`] and [`libdengjenSpeakToFile`].
///
/// Callers set `struct_size` to `sizeof(SynthesisParams)` as they compiled it. Later library
/// versions only append fields and read them when `struct_size` covers them, so an older caller
/// keeps working.
#[derive(Clone)]
#[repr(C)]
pub struct SynthesisParams {
    struct_size: u32,
    mode: i32,
    rate: u8,
    volume: u8,
    pitch: u8,
    appended_silence_ms: u32,
    callback: Option<extern "C" fn(SynthesisEvent, *mut c_void) -> u8>,
    nonblocking: u8,

    user_data: *mut c_void,
}

// SAFETY: `SynthesisParams` is moved by value across the SYNTHESIS_THREAD_POOL boundary in
// nonblocking mode (see `_synthesize`). Every field but `user_data` is plain data or a bare
// `extern "C" fn` pointer (already Send); `user_data` is an opaque caller-supplied pointer that
// Rust never dereferences — its cross-thread-safety is the FFI caller's contract to uphold, the
// same as every other raw-pointer FFI parameter this crate accepts.
unsafe impl Send for SynthesisParams {}

/// Bumped on any incompatible change to the exported functions or structs.
pub const DENGJEN_ABI_VERSION: u32 = 1;

/// The `struct_size` of the first `SynthesisParams` layout, ending at its last field. Fields
/// appended later do not move it, so callers built against this layout stay valid.
const SYNTHESIS_PARAMS_V1_SIZE: u32 =
    (std::mem::offset_of!(SynthesisParams, user_data) + std::mem::size_of::<*mut c_void>()) as u32;

/// # Safety
/// `params_ptr` must be null or point to a `SynthesisParams` whose first field, `struct_size`,
/// is readable and truthfully states how many bytes the caller's struct occupies.
unsafe fn read_params(params_ptr: *const SynthesisParams) -> DengjenFFIResult<SynthesisParams> {
    if params_ptr.is_null() {
        return Err(DengjenFFIError::null_pointer("params"));
    }
    // SAFETY: `struct_size` is the first field and the caller guarantees it is readable.
    let declared = unsafe { params_ptr.cast::<u32>().read_unaligned() };
    if declared < SYNTHESIS_PARAMS_V1_SIZE {
        return Err(DengjenFFIError::with_code(
            error_codes::INVALID_CONFIGURATION,
            format!(
                "params.struct_size is {declared}, but at least {SYNTHESIS_PARAMS_V1_SIZE} bytes \
                 are required; set it to sizeof(SynthesisParams)"
            ),
        ));
    }
    // Only the bytes the caller declared are copied; fields beyond them keep their zero default.
    let readable = (declared as usize).min(std::mem::size_of::<SynthesisParams>());
    let mut params = std::mem::MaybeUninit::<SynthesisParams>::zeroed();
    // SAFETY: the caller vouches that `declared` bytes are readable at `params_ptr`, `readable`
    // never exceeds `declared` or the destination, and an all-zero `SynthesisParams` is valid.
    Ok(unsafe {
        std::ptr::copy_nonoverlapping(
            params_ptr.cast::<u8>(),
            params.as_mut_ptr().cast::<u8>(),
            readable,
        );
        params.assume_init()
    })
}

/// The ABI version this library was built with; bindings compare it with the one they were
/// generated against before making any other call.
#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn libdengjenAbiVersion() -> u32 {
    DENGJEN_ABI_VERSION
}

impl SynthesisParams {
    fn as_synth_output_config(&self) -> AudioOutputConfig {
        let &Self {
            rate,
            volume,
            pitch,
            appended_silence_ms,
            ..
        } = self;
        AudioOutputConfig {
            appended_silence_ms: Some(appended_silence_ms),
            pitch: Some(pitch),
            rate: Some(rate),
            volume: Some(volume),
        }
    }
}

#[repr(C)]
pub struct PiperSynthConfig {
    speaker: u32,
    length_scale: f32,
    noise_scale: f32,
    noise_w: f32,
}

impl PiperSynthConfig {
    fn as_piper_synth_config(&self) -> dengjen_tts_piper::PiperSynthesisConfig {
        let &Self {
            speaker,
            length_scale,
            noise_scale,
            noise_w,
        } = self;
        dengjen_tts_piper::PiperSynthesisConfig {
            speaker: (speaker != PIPER_SYNTH_CONFIG_NO_SPEAKER).then(|| i64::from(speaker)),
            length_scale,
            noise_scale,
            noise_w,
        }
    }
}

/// Dereferences `ptr` and hands back a shared reference, treating null the way this C ABI
/// expects errors to surface: a `NULL_POINTER` `DengjenFFIError` tagged with `param_name` is
/// written into `out_error` and `None` comes back instead of triggering undefined behavior.
///
/// # Safety
/// If non-null, `ptr` must be well-aligned and point to a live, initialized `T`.
unsafe fn require_ref<'a, T>(
    ptr: *const T,
    param_name: &str,
    out_error: &mut ExternError,
) -> Option<&'a T> {
    // SAFETY: caller's obligation is this function's own `# Safety` doc.
    match unsafe { ptr.as_ref() } {
        Some(value) => Some(value),
        None => {
            *out_error = DengjenFFIError::null_pointer(param_name).into();
            None
        }
    }
}

/// Exclusive-access counterpart of [`require_ref`]; identical null handling.
///
/// # Safety
/// If non-null, `ptr` must be well-aligned, point to a live, initialized `T`, and have no other
/// live reference to it for the duration of `'a`.
unsafe fn require_mut<'a, T>(
    ptr: *mut T,
    param_name: &str,
    out_error: &mut ExternError,
) -> Option<&'a mut T> {
    // SAFETY: caller's obligation is this function's own `# Safety` doc.
    match unsafe { ptr.as_mut() } {
        Some(value) => Some(value),
        None => {
            *out_error = DengjenFFIError::null_pointer(param_name).into();
            None
        }
    }
}

/// # Safety
/// `string_ptr` must be non-null and well-aligned.
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn libdengjenFreeString(string_ptr: *mut std::os::raw::c_char) {
    // SAFETY: this function's own `# Safety` doc is the destructor's contract; nothing else
    // happens to `string_ptr` before the hand-off.
    unsafe { _internal_libdengjenFreeString(string_ptr) };
}

/// # Safety
/// `synth_config` must be non-null and well-aligned.
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn libdengjenFreePiperSynthConfig(synth_config: *mut PiperSynthConfig) {
    // SAFETY: this function's own `# Safety` doc is the destructor's contract; nothing else
    // happens to `synth_config` before the hand-off.
    unsafe { _internal_libdengjenFreePiperSynthConfig(synth_config) };
}

/// # Safety
/// `event` must be a value previously returned by a `SpeechSynthesisCallback` invocation (i.e.
/// built by `SynthesisEvent::with_speech`/`with_error`/`with_finished`), and must be passed to
/// this function at most once. This frees everything `event` owns, including the error message
/// behind a non-null `error_ptr` — callers must not separately free that message.
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn libdengjenFreeSynthesisEvent(event: SynthesisEvent) {
    ffi_support::abort_on_panic::with_abort_on_panic(|| {
        let SynthesisEvent {
            error_ptr,
            data,
            len,
            ..
        } = event;

        // SAFETY: every `SynthesisEvent` constructor builds `data`/`len` via `leak_bytes`, which
        // leaks a `Box<[u8]>` of exactly `len` bytes through `Box::into_raw`. Rebuilding the fat
        // pointer with `slice_from_raw_parts_mut` and handing it straight to `Box::from_raw`
        // reclaims that allocation without first materializing a reference to memory this
        // function doesn't own yet — the caller's one-and-only-free obligation is this
        // function's `# Safety` doc.
        drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(data, len as usize)) });
        if !error_ptr.is_null() {
            // SAFETY: only `with_error` sets a non-null `error_ptr`, via
            // `Box::into_raw(Box::new(..))`; this is that box's one and only reclaim.
            let boxed_error = unsafe { Box::from_raw(error_ptr) };

            // SAFETY: `ExternError`'s `Drop` is a documented no-op (its message is a
            // C-owned string, normally freed by the caller via `libdengjenFreeString`) —
            // a plain `drop` here would leak the message on every real synthesis-error
            // event. `manually_release()` is `ffi_support`'s own way to reclaim it, and
            // this is that box's one and only reclaim (see above), so it's called at
            // most once.
            unsafe { boxed_error.manually_release() };
        }
    });
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn libdengjenLoadVoiceFromConfigPath(
    config_path_ptr: FfiStr,
    out_error: &mut ExternError,
) -> *mut DengjenVoice {
    let load_from_config = move || _load_voice(config_path_ptr);
    call_with_result(out_error, load_from_config)
}

/// Cancels any running synthesis, waits for nonblocking workers to stop, then frees the voice.
/// No callback fires after this returns, so `user_data` may be released immediately afterwards.
/// The exception is a call from inside a callback of the same voice: it frees the voice without
/// waiting, and that worker still delivers a final FINISHED event, so keep `user_data` alive
/// until it arrives.
///
/// # Safety
/// `voice_ptr` must be non-null and well-aligned, and no other call may use the voice
/// concurrently with or after this one.
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn libdengjenUnloadDengjenVoice(voice_ptr: *mut DengjenVoice) {
    // SAFETY: this function's own `# Safety` doc is the destructor's contract; nothing else
    // happens to `voice_ptr` before the hand-off.
    unsafe { _internal_libdengjenUnloadDengjenVoice(voice_ptr) };
}

/// # Safety
/// If non-null, `voice_ptr` and `audio_info_ptr` must each be well-aligned and point to a valid
/// `DengjenVoice`/`AudioInfo`. Either being null is handled gracefully: a NULL_POINTER error is
/// written to `out_error` and `audio_info_ptr` is left untouched.
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn libdengjenGetAudioInfo(
    voice_ptr: *mut DengjenVoice,
    audio_info_ptr: *mut AudioInfo,
    out_error: &mut ExternError,
) {
    // SAFETY: `voice_ptr` carries this function's own non-null-or-valid contract.
    let Some(voice) = (unsafe { require_ref(voice_ptr, "voice_ptr", out_error) }) else {
        return;
    };

    // SAFETY: `audio_info_ptr` carries this function's own non-null-or-valid contract.
    let Some(audio_info) = (unsafe { require_mut(audio_info_ptr, "audio_info_ptr", out_error) })
    else {
        return;
    };
    let mut out = AssertUnwindSafe(audio_info);
    call_with_result(out_error, move || match voice.audio_output_info() {
        Ok(info) => {
            out.sample_rate = info.sample_rate as u32;
            out.num_channels = info.num_channels as u32;
            out.sample_width = info.sample_width as u32;
            Ok(())
        }
        Err(e) => Err(DengjenFFIError::from(e)),
    })
}

/// # Safety
/// If non-null, `voice_ptr` must be well-aligned and point to a valid `DengjenVoice`. Passing
/// null is handled gracefully: a NULL_POINTER error is written to `out_error` and a null pointer
/// is returned.
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn libdengjenGetPiperDefaultSynthConfig(
    voice_ptr: *mut DengjenVoice,
    out_error: &mut ExternError,
) -> *mut PiperSynthConfig {
    // SAFETY: `voice_ptr` carries this function's own non-null-or-valid contract.
    let Some(voice) = (unsafe { require_ref(voice_ptr, "voice_ptr", out_error) }) else {
        return std::ptr::null_mut();
    };
    call_with_result(out_error, move || {
        let config = voice
            .get_default_synthesis_config()
            .map_err(DengjenFFIError::from)?
            .ok_or_else(|| {
                DengjenFFIError::with_code(
                    error_codes::INVALID_CONFIGURATION,
                    "voice has no default Piper synthesis config to return",
                )
            })?;
        let piper_config = dengjen_tts_piper::PiperSynthesisConfig::from(&config);
        Ok::<_, DengjenFFIError>(PiperSynthConfig {
            speaker: piper_config
                .speaker
                .map_or(PIPER_SYNTH_CONFIG_NO_SPEAKER, |sid| sid as u32),
            length_scale: piper_config.length_scale,
            noise_scale: piper_config.noise_scale,
            noise_w: piper_config.noise_w,
        })
    })
}

/// # Safety
/// If non-null, `voice_ptr` must be well-aligned and point to a valid `DengjenVoice`. Passing
/// null is handled gracefully: a NULL_POINTER error is written to `out_error`.
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn libdengjenSetPiperSynthConfig(
    voice_ptr: *mut DengjenVoice,
    synth_config: PiperSynthConfig,
    out_error: &mut ExternError,
) {
    // SAFETY: `voice_ptr` carries this function's own non-null-or-valid contract.
    let Some(voice) = (unsafe { require_ref(voice_ptr, "voice_ptr", out_error) }) else {
        return;
    };
    let new_config = dengjen_tts_core::SynthesisConfig::from(&synth_config.as_piper_synth_config());
    call_with_result(out_error, move || {
        voice
            .set_fallback_synthesis_config(&new_config)
            .map_err(DengjenFFIError::from)
    })
}

/// Sets a single named entry in the voice's generic synthesis `parameters` map. This is an
/// additive escape hatch alongside the named-field setters (e.g. [`libdengjenSetPiperSynthConfig`]):
/// `key` may be any string, but whether it has any effect depends on the loaded backend — a
/// backend that doesn't recognize `key` silently ignores it. Piper only recognizes
/// `length_scale`, `noise_scale`, and `noise_w`; Kokoro ignores all three (it has no tunable
/// synthesis parameters).
///
/// # Safety
/// If non-null, `voice_ptr` must be well-aligned and point to a valid `DengjenVoice`. `key_ptr`
/// must be a valid, NUL-terminated UTF-8 C string for the duration of this call. Passing a null
/// `voice_ptr` is handled gracefully: a NULL_POINTER error is written to `out_error`.
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn libdengjenSetSynthesisParameter(
    voice_ptr: *mut DengjenVoice,
    key_ptr: FfiStr,
    value: f32,
    out_error: &mut ExternError,
) {
    // SAFETY: `voice_ptr` carries this function's own non-null-or-valid contract.
    let Some(voice) = (unsafe { require_ref(voice_ptr, "voice_ptr", out_error) }) else {
        return;
    };
    call_with_result(out_error, move || {
        let Some(key) = key_ptr.into_opt_string() else {
            return Err(DengjenFFIError::invalid_utf8());
        };
        let mut config = voice
            .get_fallback_synthesis_config()
            .map_err(DengjenFFIError::from)?
            .unwrap_or_default();
        config.parameters.insert(key, value);
        voice
            .set_fallback_synthesis_config(&config)
            .map_err(DengjenFFIError::from)
    })
}

/// Reads a single named entry from the voice's generic synthesis `parameters` map — the
/// read-side counterpart to [`libdengjenSetSynthesisParameter`]. Returns `true` and writes
/// `*out_value_ptr` if `key` is present in the voice's current generic parameters map; returns
/// `false` (not an error) if `key` is absent, or if the loaded backend has no synthesis config
/// at all (e.g. Kokoro).
///
/// # Safety
/// If non-null, `voice_ptr` and `out_value_ptr` must each be well-aligned and point to a valid
/// `DengjenVoice`/`f32`. `key_ptr` must be a valid, NUL-terminated UTF-8 C string for the
/// duration of this call. Either `voice_ptr` or `out_value_ptr` being null is handled
/// gracefully: a NULL_POINTER error is written to `out_error`, `false` is returned, and
/// `out_value_ptr` (if non-null) is left untouched.
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn libdengjenGetSynthesisParameter(
    voice_ptr: *mut DengjenVoice,
    key_ptr: FfiStr,
    out_value_ptr: *mut f32,
    out_error: &mut ExternError,
) -> bool {
    // SAFETY: `voice_ptr` carries this function's own non-null-or-valid contract.
    let Some(voice) = (unsafe { require_ref(voice_ptr, "voice_ptr", out_error) }) else {
        return false;
    };

    // SAFETY: `out_value_ptr` carries this function's own non-null-or-valid contract.
    let Some(out_value) = (unsafe { require_mut(out_value_ptr, "out_value_ptr", out_error) })
    else {
        return false;
    };
    let mut out_value = AssertUnwindSafe(out_value);
    (call_with_result(out_error, move || {
        let Some(key) = key_ptr.into_opt_string() else {
            return Err(DengjenFFIError::invalid_utf8());
        };
        let config = voice
            .get_fallback_synthesis_config()
            .map_err(DengjenFFIError::from)?
            .unwrap_or_default();
        match config.parameters.get(&key) {
            Some(value) => {
                **out_value = *value;
                Ok::<_, DengjenFFIError>(true)
            }
            None => Ok(false),
        }
    }) as u8)
        != 0
}

/// # Safety
/// If non-null, `voice_ptr` must be well-aligned and point to a valid `DengjenVoice`. Passing
/// null is handled gracefully: a NULL_POINTER error is written to `out_error`. A null
/// `params_ptr`, or a null callback in it, is handled the same way — this call has no other way
/// to report results. `(*params_ptr).struct_size` must be the caller's
/// `sizeof(SynthesisParams)`.
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn libdengjenSpeak(
    voice_ptr: *mut DengjenVoice,
    text_ptr: FfiStr,
    params_ptr: *const SynthesisParams,
    out_error: &mut ExternError,
) {
    // SAFETY: `voice_ptr` carries this function's own non-null-or-valid contract.
    let Some(voice) = (unsafe { require_ref(voice_ptr, "voice_ptr", out_error) }) else {
        return;
    };
    // SAFETY: `params_ptr` carries this function's own null-or-valid `struct_size` contract.
    let params = match unsafe { read_params(params_ptr) } {
        Ok(params) => params,
        Err(error) => {
            *out_error = error.into();
            return;
        }
    };
    let owned_synth = AssertUnwindSafe(Arc::clone(&voice.synth));
    let owned_cancel_slot = Arc::clone(&voice.active_cancel_token);
    let in_flight = Arc::clone(&voice.in_flight);
    call_with_result(out_error, move || {
        _synthesize(owned_synth, owned_cancel_slot, in_flight, text_ptr, params)
    })
}

/// # Safety
/// If non-null, `voice_ptr` must be well-aligned and point to a valid `DengjenVoice`. Passing
/// null is handled gracefully: a NULL_POINTER error is written to `out_error`.
///
/// This is meant to be called from a different thread than the one running `libdengjenSpeak` —
/// cross-thread cancellation is the entire purpose. Because of that, the caller must guarantee
/// the same voice is not torn down via `libdengjenUnloadDengjenVoice` while, or immediately
/// after, this call is in flight; racing the two is a use-after-free only the caller can avoid.
///
/// # Behaviour
/// - Has no effect unless a realtime-mode synthesis is currently running on this voice; lazy
///   and parallel syntheses cannot be interrupted this way.
/// - Only the most recently started realtime synthesis on this voice is reachable. If
///   nonblocking mode started two realtime syntheses concurrently, the earlier one cannot be
///   cancelled from here.
/// - The streaming callback still receives `SYNTH_EVENT_FINISHED` after a successful
///   cancellation — the callback sequence alone can't tell a cancelled stream from one that ran
///   to completion.
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn libdengjenCancel(
    voice_ptr: *mut DengjenVoice,
    out_error: &mut ExternError,
) {
    // SAFETY: `voice_ptr` carries this function's own non-null-or-valid contract.
    let Some(voice) = (unsafe { require_ref(voice_ptr, "voice_ptr", out_error) }) else {
        return;
    };
    call_with_result(out_error, || _cancel(&voice.active_cancel_token))
}

/// # Safety
/// If non-null, `voice_ptr` must be well-aligned and point to a valid `DengjenVoice`. Passing
/// null is handled gracefully: a NULL_POINTER error is written to `out_error` and `0` is
/// returned. A null `params_ptr` is handled the same way, and `(*params_ptr).struct_size` must
/// be the caller's `sizeof(SynthesisParams)`.
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn libdengjenSpeakToFile(
    voice_ptr: *mut DengjenVoice,
    text_ptr: FfiStr,
    params_ptr: *const SynthesisParams,
    out_filename_ptr: FfiStr,
    out_error: &mut ExternError,
) -> u8 {
    // SAFETY: `voice_ptr` carries this function's own non-null-or-valid contract.
    let Some(voice) = (unsafe { require_ref(voice_ptr, "voice_ptr", out_error) }) else {
        return 0;
    };
    // SAFETY: `params_ptr` carries this function's own null-or-valid `struct_size` contract.
    let params = match unsafe { read_params(params_ptr) } {
        Ok(params) => params,
        Err(error) => {
            *out_error = error.into();
            return 0;
        }
    };
    let owned_synth = AssertUnwindSafe(Arc::clone(&voice.synth));
    call_with_result(out_error, move || {
        _synthesize_to_file(owned_synth, text_ptr, params, out_filename_ptr)?;
        Ok::<u8, DengjenFFIError>(1)
    })
}

fn init_ort_environment() -> DengjenResult<()> {
    INIT_ORT_ENVIRONMENT
        .get_or_init(commit_ort_environment)
        .clone()
        .map_err(DengjenError::OperationError)
}

fn commit_ort_environment() -> Result<(), String> {
    #[cfg(target_os = "android")]
    let execution_providers = vec![
        ort::execution_providers::NNAPI::default().build(),
        ort::execution_providers::CPU::default().build(),
    ];
    #[cfg(target_os = "ios")]
    let execution_providers = vec![
        ort::execution_providers::CoreML::default().build(),
        ort::execution_providers::CPU::default().build(),
    ];
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let execution_providers = vec![ort::execution_providers::CPU::default().build()];

    let committed = ort::init()
        .with_name("dengjen")
        .with_execution_providers(execution_providers)
        .commit();

    committed_or_error(committed)
}

fn committed_or_error(committed: bool) -> Result<(), String> {
    if committed {
        Ok(())
    } else {
        Err("Failed to initialize onnxruntime".to_string())
    }
}

fn load_voice(config_path: &std::path::Path) -> DengjenResult<Arc<dyn DengjenModel + Send + Sync>> {
    dengjen_tts::load_voice_with(config_path, |model_type, path| match model_type {
        dengjen_tts::ModelType::Piper => dengjen_tts_piper::from_config_path(path),
        dengjen_tts::ModelType::Kokoro => dengjen_tts_kokoro::from_config_path(path),
        dengjen_tts::ModelType::MeloTts => dengjen_tts_melotts::from_config_path(path),
        other => Err(DengjenError::InvalidConfiguration(format!(
            "unsupported model type `{other}`"
        ))),
    })
}

fn _load_voice(config_path_ptr: FfiStr) -> DengjenFFIResult<DengjenVoice> {
    init_ort_environment()?;
    let Some(config_path) = config_path_ptr.into_opt_string() else {
        return Err(DengjenFFIError::invalid_utf8());
    };
    let model = load_voice(&PathBuf::from(config_path))?;
    let synth = DengjenSpeechSynthesizer::new(model)?;
    Ok(synth.into())
}

fn _cancel(cancel_slot: &Arc<Mutex<Option<CancellationToken>>>) -> DengjenFFIResult<()> {
    let held_token = cancel_slot
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(active_token) = held_token.as_ref() {
        active_token.cancel();
    }
    Ok(())
}

struct CancelSlotGuard {
    slot: Arc<Mutex<Option<CancellationToken>>>,
    token: CancellationToken,
}

impl Drop for CancelSlotGuard {
    fn drop(&mut self) {
        let mut held_token = self
            .slot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let still_owns_slot = held_token
            .as_ref()
            .is_some_and(|current| current.points_to_same_flag(&self.token));
        if still_owns_slot {
            *held_token = None;
        }
    }
}

fn _synthesize(
    synth: AssertUnwindSafe<Arc<DengjenSpeechSynthesizer>>,
    cancel_slot: Arc<Mutex<Option<CancellationToken>>>,
    in_flight: Arc<InFlight>,
    text_ptr: FfiStr,
    params: SynthesisParams,
) -> DengjenFFIResult<()> {
    let Some(text) = text_ptr.into_opt_string() else {
        return Err(DengjenFFIError::invalid_utf8());
    };
    let Some(callback) = params.callback else {
        return Err(DengjenFFIError::null_pointer("params.callback"));
    };
    if params.nonblocking == 0 {
        return _do_synthesize(synth, cancel_slot, &in_flight, text, callback, params);
    }

    let report_to_caller = callback;
    let report_user_data = UserDataPtr(params.user_data);
    let worker_guard = in_flight.enter();
    // An unwinding job would abort the host process; report it as an error event.
    synthesis_thread_pool()?.spawn(move || {
        let _worker_guard = worker_guard;
        let voice_id = Arc::as_ptr(&_worker_guard.0) as usize;
        let previous_voice = ACTIVE_WORKER_VOICE.with(|id| id.replace(voice_id));
        let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
            _do_synthesize(synth, cancel_slot, &_worker_guard.0, text, callback, params)
        }))
        .unwrap_or_else(|payload| {
            Err(DengjenError::InferenceError(format!(
                "synthesis worker panicked: {}",
                panic_message(payload.as_ref())
            ))
            .into())
        });
        if let Err(error) = outcome {
            invoke_callback(
                report_to_caller,
                SynthesisEvent::with_error(error),
                report_user_data,
            );
        }
        ACTIVE_WORKER_VOICE.with(|id| id.set(previous_voice));
    });
    Ok(())
}

fn _do_synthesize(
    synth: AssertUnwindSafe<Arc<DengjenSpeechSynthesizer>>,
    cancel_slot: Arc<Mutex<Option<CancellationToken>>>,
    in_flight: &InFlight,
    text: String,
    callback: SpeechSynthesisCallback,
    params: SynthesisParams,
) -> DengjenFFIResult<()> {
    const REALTIME_CHUNK_SIZE: usize = 72;
    const REALTIME_CHUNK_PADDING: usize = 3;

    let output_config = Some(params.as_synth_output_config());
    let user_data = params.user_data;

    let mut _release_slot_on_drop = None;
    let mode = match params.mode {
        synth_mode::SYNTH_MODE_LAZY => StreamMode::Lazy,
        synth_mode::SYNTH_MODE_PARALLEL => StreamMode::Parallel,
        synth_mode::SYNTH_MODE_BATCHED => StreamMode::Batched {
            batch_size: default_batch_size(),
        },
        synth_mode::SYNTH_MODE_REALTIME => {
            let cancel_token = CancellationToken::new();
            *cancel_slot
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(cancel_token.clone());
            _release_slot_on_drop = Some(CancelSlotGuard {
                slot: cancel_slot,
                token: cancel_token.clone(),
            });
            StreamMode::Realtime {
                chunk_size: REALTIME_CHUNK_SIZE,
                chunk_padding: REALTIME_CHUNK_PADDING,
                cancel_token,
            }
        }
        _ => return Err(DengjenFFIError::invalid_synthesis_mode()),
    };

    let stream = synth
        .synthesize_samples(text, output_config, mode)
        .map_err(DengjenFFIError::from)?;
    iterate_stream(stream, callback, user_data, in_flight)
}

fn iterate_stream(
    stream: impl Iterator<Item = DengjenResult<AudioSamples>> + Send + Sync + 'static,
    callback: SpeechSynthesisCallback,
    user_data: *mut c_void,
    in_flight: &InFlight,
) -> DengjenFFIResult<()> {
    for item in stream {
        if in_flight.is_closing() {
            break;
        }
        let audio = match item {
            Ok(audio) => audio,
            Err(error) => {
                callback(
                    SynthesisEvent::with_error(DengjenFFIError::from(error)),
                    user_data,
                );
                return Ok(());
            }
        };
        let caller_wants_more = callback(
            SynthesisEvent::with_speech(audio.as_wave_bytes()),
            user_data,
        ) == 0;
        if !caller_wants_more {
            return Ok(());
        }
    }
    callback(SynthesisEvent::with_finished(), user_data);
    Ok(())
}

fn _synthesize_to_file(
    synth: AssertUnwindSafe<Arc<DengjenSpeechSynthesizer>>,
    text_ptr: FfiStr,
    params: SynthesisParams,
    out_filename_ptr: FfiStr,
) -> DengjenFFIResult<()> {
    let (Some(text), Some(out_filename)) = (
        text_ptr.into_opt_string(),
        out_filename_ptr.into_opt_string(),
    ) else {
        return Err(DengjenFFIError::invalid_utf8());
    };
    synth
        .synthesize_to_file(
            &PathBuf::from(out_filename),
            text,
            Some(params.as_synth_output_config()),
        )
        .map_err(DengjenFFIError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ffi_support::ExternError;

    extern "C" fn noop_callback(_event: SynthesisEvent, _user_data: *mut c_void) -> u8 {
        1
    }

    fn synth_params() -> SynthesisParams {
        SynthesisParams {
            struct_size: SYNTHESIS_PARAMS_V1_SIZE,
            mode: synth_mode::SYNTH_MODE_LAZY,
            rate: 50,
            volume: 100,
            pitch: 50,
            appended_silence_ms: 0,
            callback: Some(noop_callback),
            nonblocking: 0,
            user_data: std::ptr::null_mut(),
        }
    }

    fn c_str(s: &str) -> std::ffi::CString {
        std::ffi::CString::new(s).unwrap()
    }

    fn new_test_piper_voice() -> *mut DengjenVoice {
        use std::sync::atomic::{AtomicU64, Ordering};

        // Each call gets its own subdirectory: this helper's callers run concurrently under
        // the default test harness, and a shared fixture path races on the file writes below.
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "dengjen_capi_user_data_test_{}",
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let model_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../dengjen/models/piper/tests/fixtures/synthetic_piper_batch.onnx");
        assert!(
            model_path.exists(),
            "Model path does not exist: {:?}",
            model_path
        );
        std::fs::copy(&model_path, dir.join("piper_test.onnx")).unwrap();
        let config_path = dir.join("piper_test.onnx.json");
        std::fs::write(
            &config_path,
            r#"{
                "key": null,
                "language": {"code": "en-US"},
                "audio": {"sample_rate": 22050, "quality": null},
                "num_speakers": 1,
                "speaker_id_map": {},
                "streaming": false,
                "espeak": {"voice": "en-us"},
                "inference": {"noise_scale": 0.667, "length_scale": 1.0, "noise_w": 0.8},
                "num_symbols": 8,
                "phoneme_map": {},
                "phoneme_id_map": {"^": [1], "$": [2], "_": [3], "t": [4]},
                "phoneme_type": "text",
                "hop_length": 256
            }"#,
        )
        .unwrap();
        let config_cstring = c_str(config_path.to_str().unwrap());
        let mut out_error = ExternError::default();
        // SAFETY: config_path points at a file this function just wrote; config_cstring
        // stays alive (not dropped) for the full duration of this call.
        unsafe {
            libdengjenLoadVoiceFromConfigPath(
                FfiStr::from_raw(config_cstring.as_ptr()),
                &mut out_error,
            )
        }
    }

    #[test]
    fn batched_mode_reaches_finished_without_an_error_event() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::OnceLock;

        static SAW_FINISHED: OnceLock<AtomicBool> = OnceLock::new();
        static SAW_ERROR: OnceLock<AtomicBool> = OnceLock::new();
        SAW_FINISHED.get_or_init(|| AtomicBool::new(false));
        SAW_ERROR.get_or_init(|| AtomicBool::new(false));

        extern "C" fn recording_callback(event: SynthesisEvent, _user_data: *mut c_void) -> u8 {
            match event.event_type {
                synth_event::SYNTH_EVENT_FINISHED => {
                    SAW_FINISHED.get().unwrap().store(true, Ordering::SeqCst)
                }
                synth_event::SYNTH_EVENT_ERROR => {
                    SAW_ERROR.get().unwrap().store(true, Ordering::SeqCst)
                }
                _ => {}
            }
            // SAFETY: test-only reclaim of an event this test's own call produced.
            unsafe { libdengjenFreeSynthesisEvent(event) };
            0
        }

        let voice_ptr = new_test_piper_voice();
        assert!(!voice_ptr.is_null(), "Failed to load test piper voice");
        let mut params = synth_params();
        params.mode = synth_mode::SYNTH_MODE_BATCHED;
        params.callback = Some(recording_callback);
        let mut out_error = ExternError::default();
        let text = c_str("t:_");

        // SAFETY: voice_ptr is a valid handle just loaded above; text stays alive (not
        // dropped) for the full duration of this call.
        unsafe {
            libdengjenSpeak(
                voice_ptr,
                FfiStr::from_raw(text.as_ptr()),
                &params,
                &mut out_error,
            );
            libdengjenUnloadDengjenVoice(voice_ptr);
        }

        assert!(SAW_FINISHED.get().unwrap().load(Ordering::SeqCst));
        assert!(!SAW_ERROR.get().unwrap().load(Ordering::SeqCst));
    }

    #[test]
    fn user_data_round_trips_unchanged_through_every_callback_invocation() {
        use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
        use std::sync::OnceLock;

        static LAST_USER_DATA: OnceLock<AtomicPtr<u8>> = OnceLock::new();
        static MISMATCH: OnceLock<AtomicPtr<u8>> = OnceLock::new();
        static SAW_FINISHED: OnceLock<AtomicBool> = OnceLock::new();
        LAST_USER_DATA.get_or_init(|| AtomicPtr::new(std::ptr::null_mut()));
        MISMATCH.get_or_init(|| AtomicPtr::new(std::ptr::null_mut()));
        SAW_FINISHED.get_or_init(|| AtomicBool::new(false));

        extern "C" fn recording_callback(event: SynthesisEvent, user_data: *mut c_void) -> u8 {
            let expected = LAST_USER_DATA.get().unwrap().load(Ordering::SeqCst);
            if user_data as *mut u8 != expected {
                MISMATCH
                    .get()
                    .unwrap()
                    .store(user_data as *mut u8, Ordering::SeqCst);
            }
            if event.event_type == synth_event::SYNTH_EVENT_FINISHED {
                SAW_FINISHED.get().unwrap().store(true, Ordering::SeqCst);
            }
            // SAFETY: test-only reclaim of an event this test's own call produced.
            unsafe { libdengjenFreeSynthesisEvent(event) };
            0
        }

        let mut sentinel: u8 = 0;
        let token: *mut c_void = std::ptr::addr_of_mut!(sentinel).cast();
        LAST_USER_DATA
            .get()
            .unwrap()
            .store(token as *mut u8, Ordering::SeqCst);

        let voice_ptr = new_test_piper_voice();
        assert!(!voice_ptr.is_null(), "Failed to load test piper voice");
        let mut params = synth_params();
        params.callback = Some(recording_callback);
        params.user_data = token;
        let mut out_error = ExternError::default();
        let text = c_str("t:_");

        // SAFETY: voice_ptr is a valid handle just loaded above; text stays alive (not
        // dropped) for the full duration of this call.
        unsafe {
            libdengjenSpeak(
                voice_ptr,
                FfiStr::from_raw(text.as_ptr()),
                &params,
                &mut out_error,
            )
        };
        assert_eq!(out_error.get_code(), ffi_support::ErrorCode::SUCCESS);

        let mismatch = MISMATCH.get().unwrap().load(Ordering::SeqCst);
        assert!(
            mismatch.is_null(),
            "callback received a user_data pointer ({mismatch:?}) that didn't match the one \
             passed into libdengjenSpeak ({token:?})"
        );

        assert!(
            SAW_FINISHED.get().unwrap().load(Ordering::SeqCst),
            "callback never received SYNTH_EVENT_FINISHED; test did not exercise the \
             terminal event path"
        );

        // SAFETY: voice_ptr is a valid handle owned by this test, not used again after this.
        unsafe { libdengjenUnloadDengjenVoice(voice_ptr) };
    }

    #[test]
    fn get_audio_info_null_voice_returns_null_pointer_error_without_panicking() {
        let mut out_error = ExternError::default();
        let mut audio_info = AudioInfo {
            sample_rate: 0,
            num_channels: 0,
            sample_width: 0,
        };

        // SAFETY: a null `voice_ptr` is explicitly covered by `libdengjenGetAudioInfo`'s own
        // `# Safety` doc -- it's handled gracefully (writes a NULL_POINTER error) rather than
        // dereferenced.
        unsafe {
            libdengjenGetAudioInfo(std::ptr::null_mut(), &mut audio_info, &mut out_error);
        }
        assert_eq!(out_error.get_code().code(), error_codes::NULL_POINTER);

        // SAFETY: `out_error`'s message is never freed by `Drop` per ffi_support's documented
        // contract (real FFI consumers free it via `libdengjenFreeString`); release it here so
        // the test doesn't leak.
        unsafe { out_error.manually_release() };
    }

    #[test]
    fn get_piper_default_synth_config_null_voice_returns_null_pointer_error_without_panicking() {
        let mut out_error = ExternError::default();

        // SAFETY: a null `voice_ptr` is explicitly covered by `libdengjenGetPiperDefaultSynthConfig`'s
        // own `# Safety` doc -- it's handled gracefully (writes a NULL_POINTER error) rather than
        // dereferenced.
        let result =
            unsafe { libdengjenGetPiperDefaultSynthConfig(std::ptr::null_mut(), &mut out_error) };
        assert!(result.is_null());
        assert_eq!(out_error.get_code().code(), error_codes::NULL_POINTER);

        // SAFETY: see `get_audio_info_null_voice_returns_null_pointer_error_without_panicking`.
        unsafe { out_error.manually_release() };
    }

    #[test]
    fn set_piper_synth_config_null_voice_returns_null_pointer_error_without_panicking() {
        let mut out_error = ExternError::default();
        let synth_config = PiperSynthConfig {
            speaker: 0,
            length_scale: 1.0,
            noise_scale: 1.0,
            noise_w: 1.0,
        };

        // SAFETY: see `get_audio_info_null_voice_returns_null_pointer_error_without_panicking`
        // -- same null-`voice_ptr`-handled-gracefully contract, on `libdengjenSetPiperSynthConfig`.
        unsafe {
            libdengjenSetPiperSynthConfig(std::ptr::null_mut(), synth_config, &mut out_error);
        }
        assert_eq!(out_error.get_code().code(), error_codes::NULL_POINTER);

        // SAFETY: see `get_audio_info_null_voice_returns_null_pointer_error_without_panicking`.
        unsafe { out_error.manually_release() };
    }

    #[test]
    fn set_synthesis_parameter_null_voice_returns_null_pointer_error_without_panicking() {
        let mut out_error = ExternError::default();
        let key = FfiStr::from_cstr(c"noise_scale");

        // SAFETY: see `get_audio_info_null_voice_returns_null_pointer_error_without_panicking`
        // -- same null-`voice_ptr`-handled-gracefully contract, on `libdengjenSetSynthesisParameter`.
        unsafe {
            libdengjenSetSynthesisParameter(std::ptr::null_mut(), key, 0.5, &mut out_error);
        }
        assert_eq!(out_error.get_code().code(), error_codes::NULL_POINTER);

        // SAFETY: see `get_audio_info_null_voice_returns_null_pointer_error_without_panicking`.
        unsafe { out_error.manually_release() };
    }

    #[test]
    fn load_voice_errors_on_a_missing_config_path() {
        let path = std::path::Path::new("/nonexistent-dengjen-capi-load-voice-test.json");
        assert!(load_voice(path).is_err());
    }

    fn write_temp_config(dir: &std::path::Path, name: &str, contents: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn load_voice_routes_kokoro_model_type_toward_the_kokoro_loader() {
        let dir = std::env::temp_dir().join("dengjen_capi_load_voice_test_kokoro");
        std::fs::create_dir_all(&dir).unwrap();

        let path = write_temp_config(&dir, "config.json", r#"{"model_type": "kokoro"}"#);
        let err = match load_voice(&path) {
            Err(e) => format!("{}{}", e, e.cause_chain()),
            Ok(_) => panic!("expected an error for an incomplete Kokoro config"),
        };
        assert!(
            err.contains("model_path"),
            "expected a Kokoro-loader error naming the missing `model_path` field, got: {err}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_voice_routes_melotts_model_type_toward_the_melotts_loader() {
        let dir = std::env::temp_dir().join("dengjen_capi_load_voice_test_melotts");
        std::fs::create_dir_all(&dir).unwrap();

        let path = write_temp_config(
            &dir,
            "config.json",
            r#"{"model_type": "melotts", "audio": {"sample_rate": 24000}}"#,
        );
        let err = match load_voice(&path) {
            Err(e) => format!("{}{}", e, e.cause_chain()),
            Ok(_) => panic!("expected an error for an incomplete MeloTTS config"),
        };
        assert!(
            err.contains("phonemizer"),
            "expected a MeloTTS-loader error naming the missing `phonemizer` field, got: {err}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_voice_routes_vits_model_type_toward_the_piper_loader() {
        let dir = std::env::temp_dir().join("dengjen_capi_load_voice_test_vits");
        std::fs::create_dir_all(&dir).unwrap();

        let path = write_temp_config(&dir, "config.json", r#"{"model_type": "vits"}"#);
        let err = match load_voice(&path) {
            Err(e) => format!("{}{}", e, e.cause_chain()),
            Ok(_) => panic!("expected an error for an incomplete VITS config"),
        };
        assert!(
            err.contains("audio"),
            "expected a Piper-loader error naming the missing `audio` field, got: {err}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    struct FakeModel {
        fallback_config: Mutex<Option<dengjen_tts_core::SynthesisConfig>>,
    }

    impl DengjenModel for FakeModel {
        fn audio_output_info(&self) -> DengjenResult<dengjen_tts_core::AudioInfo> {
            Ok(dengjen_tts_core::AudioInfo {
                sample_rate: 16000,
                num_channels: 1,
                sample_width: 2,
            })
        }
        fn phonemize_text(&self, _text: &str) -> DengjenResult<dengjen_tts_core::Phonemes> {
            Ok(dengjen_tts_core::Phonemes::from(Vec::<String>::new()))
        }
        fn speak_batch(
            &self,
            _phoneme_batches: Vec<String>,
        ) -> DengjenResult<Vec<dengjen_tts_core::Audio>> {
            Ok(Vec::new())
        }
        fn speak_one_sentence(&self, _phonemes: String) -> dengjen_tts_core::DengjenAudioResult {
            Ok(dengjen_tts_core::Audio::new(
                AudioSamples::from(Vec::new()),
                16000,
                None,
            ))
        }
        fn get_default_synthesis_config(
            &self,
        ) -> DengjenResult<Option<dengjen_tts_core::SynthesisConfig>> {
            Ok(None)
        }
        fn get_fallback_synthesis_config(
            &self,
        ) -> DengjenResult<Option<dengjen_tts_core::SynthesisConfig>> {
            Ok(self.fallback_config.lock().unwrap().clone())
        }
        fn set_fallback_synthesis_config(
            &self,
            synthesis_config: &dengjen_tts_core::SynthesisConfig,
        ) -> DengjenResult<()> {
            *self.fallback_config.lock().unwrap() = Some(synthesis_config.clone());
            Ok(())
        }
    }

    fn fake_voice() -> DengjenVoice {
        let model: Arc<dyn DengjenModel + Send + Sync> = Arc::new(FakeModel {
            fallback_config: Mutex::new(None),
        });
        DengjenVoice::from(DengjenSpeechSynthesizer::new(model).unwrap())
    }

    #[test]
    fn set_synthesis_parameter_null_key_returns_invalid_utf8_error_without_panicking() {
        let mut voice = fake_voice();
        let mut out_error = ExternError::default();

        // SAFETY: constructing a null `FfiStr` is the whole point of this test — no C caller is
        // dereferencing it, `libdengjenSetSynthesisParameter` must reject it gracefully instead.
        let null_key = unsafe { FfiStr::from_raw(std::ptr::null()) };

        // SAFETY: `voice` is a valid handle owned by this test; `null_key` is deliberately
        // invalid, which `libdengjenSetSynthesisParameter`'s own `# Safety` doc covers by
        // rejecting it (`INVALID_UTF8_SEQUENCE`) instead of dereferencing it.
        unsafe {
            libdengjenSetSynthesisParameter(&mut voice, null_key, 0.5, &mut out_error);
        }
        assert_eq!(
            out_error.get_code().code(),
            error_codes::INVALID_UTF8_SEQUENCE
        );

        // SAFETY: see `get_audio_info_null_voice_returns_null_pointer_error_without_panicking`.
        unsafe { out_error.manually_release() };
    }

    #[test]
    fn get_synthesis_parameter_null_voice_returns_null_pointer_error_without_panicking() {
        let mut out_error = ExternError::default();
        let key = FfiStr::from_cstr(c"custom_knob");
        let mut value: f32 = 0.0;

        // SAFETY: see `get_audio_info_null_voice_returns_null_pointer_error_without_panicking`
        // -- same null-`voice_ptr`-handled-gracefully contract, on `libdengjenGetSynthesisParameter`.
        let found = unsafe {
            libdengjenGetSynthesisParameter(std::ptr::null_mut(), key, &mut value, &mut out_error)
        };
        assert!(!found);
        assert_eq!(out_error.get_code().code(), error_codes::NULL_POINTER);

        // SAFETY: see `get_audio_info_null_voice_returns_null_pointer_error_without_panicking`.
        unsafe { out_error.manually_release() };
    }

    #[test]
    fn get_synthesis_parameter_returns_false_for_a_key_that_was_never_set() {
        let mut voice = fake_voice();
        let mut out_error = ExternError::default();
        let key = FfiStr::from_cstr(c"custom_knob");
        let mut value: f32 = 0.0;

        // SAFETY: `voice`/`value` are well-aligned pointers to a live `DengjenVoice`/`f32`, and
        // `key` is a valid NUL-terminated C string for the call's duration -- satisfies
        // `libdengjenGetSynthesisParameter`'s own `# Safety` doc.
        let found =
            unsafe { libdengjenGetSynthesisParameter(&mut voice, key, &mut value, &mut out_error) };
        assert!(!found);
        assert!(out_error.get_code().is_success());
    }

    #[test]
    fn get_synthesis_parameter_round_trips_a_value_set_via_set_synthesis_parameter() {
        let mut voice = fake_voice();
        let mut out_error = ExternError::default();
        let key = FfiStr::from_cstr(c"custom_knob");

        // SAFETY: `voice` is a valid handle owned by this test, and `key` is valid for the
        // duration of this call.
        unsafe {
            libdengjenSetSynthesisParameter(&mut voice, key, 1.25, &mut out_error);
        }
        assert!(out_error.get_code().is_success());

        let mut value: f32 = 0.0;
        let key2 = FfiStr::from_cstr(c"custom_knob");

        // SAFETY: see above -- same valid `voice` handle, `key2` valid for the duration of
        // this call.
        let found = unsafe {
            libdengjenGetSynthesisParameter(&mut voice, key2, &mut value, &mut out_error)
        };
        assert!(found);
        assert!(out_error.get_code().is_success());
        assert_eq!(value, 1.25);
    }

    #[test]
    fn speak_null_voice_returns_null_pointer_error_without_panicking() {
        let mut out_error = ExternError::default();
        let text = std::ffi::CString::new("hello").unwrap();

        // SAFETY: see `get_audio_info_null_voice_returns_null_pointer_error_without_panicking`
        // -- same null-`voice_ptr`-handled-gracefully contract, on `libdengjenSpeak`.
        unsafe {
            libdengjenSpeak(
                std::ptr::null_mut(),
                FfiStr::from_cstr(&text),
                &synth_params(),
                &mut out_error,
            );
        }
        assert_eq!(out_error.get_code().code(), error_codes::NULL_POINTER);

        // SAFETY: see `get_audio_info_null_voice_returns_null_pointer_error_without_panicking`.
        unsafe { out_error.manually_release() };
    }

    #[test]
    fn speak_null_params_returns_null_pointer_error_without_panicking() {
        let mut voice = fake_voice();
        let mut out_error = ExternError::default();
        let text = std::ffi::CString::new("hello").unwrap();

        // SAFETY: `voice` is a valid handle owned by this test; a null `params` is handled.
        unsafe {
            libdengjenSpeak(
                &mut voice,
                FfiStr::from_cstr(&text),
                std::ptr::null(),
                &mut out_error,
            );
        }
        assert_eq!(out_error.get_code().code(), error_codes::NULL_POINTER);

        // SAFETY: the message was allocated by `call_with_result` and is released once.
        unsafe { out_error.manually_release() };
    }

    #[test]
    fn speak_rejects_a_params_struct_smaller_than_the_first_layout() {
        let mut voice = fake_voice();
        let mut out_error = ExternError::default();
        let text = std::ffi::CString::new("hello").unwrap();
        let mut params = synth_params();
        params.struct_size = SYNTHESIS_PARAMS_V1_SIZE - 1;

        // SAFETY: `voice` is a valid handle owned by this test and `params` is a full struct.
        unsafe {
            libdengjenSpeak(
                &mut voice,
                FfiStr::from_cstr(&text),
                &params,
                &mut out_error,
            );
        }
        assert_eq!(
            out_error.get_code().code(),
            error_codes::INVALID_CONFIGURATION
        );

        // SAFETY: the message was allocated by `call_with_result` and is released once.
        unsafe { out_error.manually_release() };
    }

    #[test]
    fn abi_version_matches_the_exported_constant() {
        assert_eq!(libdengjenAbiVersion(), DENGJEN_ABI_VERSION);
    }

    #[test]
    fn speak_null_callback_returns_null_pointer_error_without_panicking() {
        let mut voice = fake_voice();
        let mut out_error = ExternError::default();
        let text = std::ffi::CString::new("hello").unwrap();
        let mut params = synth_params();
        params.callback = None;

        // SAFETY: `voice` is a valid handle owned by this test; a null `params.callback` is
        // explicitly covered by `libdengjenSpeak`'s own `# Safety` doc -- handled the same way
        // as a null `voice_ptr` (a NULL_POINTER error written to `out_error`).
        unsafe {
            libdengjenSpeak(
                &mut voice,
                FfiStr::from_cstr(&text),
                &params,
                &mut out_error,
            );
        }
        assert_eq!(out_error.get_code().code(), error_codes::NULL_POINTER);

        // SAFETY: see `get_audio_info_null_voice_returns_null_pointer_error_without_panicking`.
        unsafe { out_error.manually_release() };
    }

    #[test]
    fn speak_to_file_null_voice_returns_null_pointer_error_without_panicking() {
        let mut out_error = ExternError::default();
        let text = std::ffi::CString::new("hello").unwrap();
        let filename = std::ffi::CString::new("out.wav").unwrap();

        // SAFETY: see `get_audio_info_null_voice_returns_null_pointer_error_without_panicking`
        // -- same null-`voice_ptr`-handled-gracefully contract, on `libdengjenSpeakToFile`.
        let result = unsafe {
            libdengjenSpeakToFile(
                std::ptr::null_mut(),
                FfiStr::from_cstr(&text),
                &synth_params(),
                FfiStr::from_cstr(&filename),
                &mut out_error,
            )
        };
        assert_eq!(result, 0);
        assert_eq!(out_error.get_code().code(), error_codes::NULL_POINTER);

        // SAFETY: see `get_audio_info_null_voice_returns_null_pointer_error_without_panicking`.
        unsafe { out_error.manually_release() };
    }

    #[test]
    fn speak_to_file_write_failure_populates_out_error_instead_of_reporting_success() {
        let mut voice = fake_voice();
        let mut out_error = ExternError::default();
        let text = std::ffi::CString::new("hello").unwrap();
        let filename =
            std::ffi::CString::new("/nonexistent-dengjen-capi-test-dir-xyz/out.wav").unwrap();

        // SAFETY: `voice` is a valid handle owned by this test.
        let result = unsafe {
            libdengjenSpeakToFile(
                &mut voice,
                FfiStr::from_cstr(&text),
                &synth_params(),
                FfiStr::from_cstr(&filename),
                &mut out_error,
            )
        };
        assert_eq!(result, 0);
        assert!(!out_error.get_code().is_success());

        // SAFETY: see `get_audio_info_null_voice_returns_null_pointer_error_without_panicking`.
        unsafe { out_error.manually_release() };
    }

    #[test]
    fn cancel_null_voice_returns_null_pointer_error_without_panicking() {
        let mut out_error = ExternError::default();

        // SAFETY: see `get_audio_info_null_voice_returns_null_pointer_error_without_panicking`
        // -- same null-`voice_ptr`-handled-gracefully contract, on `libdengjenCancel`.
        unsafe {
            libdengjenCancel(std::ptr::null_mut(), &mut out_error);
        }
        assert_eq!(out_error.get_code().code(), error_codes::NULL_POINTER);

        // SAFETY: see `get_audio_info_null_voice_returns_null_pointer_error_without_panicking`.
        unsafe { out_error.manually_release() };
    }

    #[test]
    fn cancel_cancels_the_token_held_in_the_slot() {
        let token = CancellationToken::new();
        let slot = Arc::new(Mutex::new(Some(token.clone())));

        assert!(_cancel(&slot).is_ok());

        assert!(token.is_cancelled());
    }

    #[test]
    fn cancel_on_an_empty_slot_is_a_noop() {
        let slot: Arc<Mutex<Option<CancellationToken>>> = Arc::new(Mutex::new(None));

        assert!(_cancel(&slot).is_ok());

        assert!(slot.lock().unwrap().is_none());
    }

    #[test]
    fn cancel_slot_guard_clears_the_slot_when_it_still_holds_its_own_token() {
        let slot = Arc::new(Mutex::new(None));
        let token = CancellationToken::new();
        *slot.lock().unwrap() = Some(token.clone());

        drop(CancelSlotGuard {
            slot: Arc::clone(&slot),
            token,
        });

        assert!(slot.lock().unwrap().is_none());
    }

    #[test]
    fn cancel_slot_guard_does_not_clobber_a_different_tokens_slot() {
        let slot = Arc::new(Mutex::new(None));
        let token_a = CancellationToken::new();
        let token_b = CancellationToken::new();
        *slot.lock().unwrap() = Some(token_a.clone());
        let guard_a = CancelSlotGuard {
            slot: Arc::clone(&slot),
            token: token_a,
        };

        *slot.lock().unwrap() = Some(token_b.clone());
        drop(guard_a);

        let held = slot.lock().unwrap();
        assert!(held.as_ref().unwrap().points_to_same_flag(&token_b));
    }

    #[test]
    fn error_codes_round_trip_through_dengjen_ffi_error() {
        let cases = [
            (
                DengjenError::FailedToLoadResource("x".into()),
                error_codes::FAILED_TO_LOAD_RESOURCE,
            ),
            (
                DengjenError::PhonemizationError("x".into()),
                error_codes::PHONEMIZATION_ERROR,
            ),
            (
                DengjenError::InferenceError("x".into()),
                error_codes::INFERENCE_ERROR,
            ),
            (
                DengjenError::InvalidConfiguration("x".into()),
                error_codes::INVALID_CONFIGURATION,
            ),
            (
                DengjenError::UnsupportedOperation("x".into()),
                error_codes::UNSUPPORTED_OPERATION,
            ),
            (
                DengjenError::OperationError("x".into()),
                error_codes::OPERATION_ERROR,
            ),
        ];
        for (err, expected_code) in cases {
            let ffi_err: DengjenFFIError = err.into();
            assert_eq!(ffi_err.0, expected_code);
        }
    }
}

#[cfg(test)]
mod abi_struct_tests {
    use super::*;

    #[test]
    fn synthesis_event_with_speech_carries_the_pcm_bytes_and_a_null_error_pointer() {
        let event = SynthesisEvent::with_speech(vec![1, 2, 3, 4]);

        assert_eq!(event.event_type, synth_event::SYNTH_EVENT_SPEECH);
        assert!(event.error_ptr.is_null());
        assert_eq!(event.len, 4);

        // SAFETY: `event.data`/`event.len` were just produced by `with_speech` from a
        // 4-byte `Vec`, so the pointer is valid for exactly that many reads.
        let bytes = unsafe { std::slice::from_raw_parts(event.data, event.len as usize) };
        assert_eq!(bytes, &[1, 2, 3, 4]);

        // SAFETY: `event` came from `with_speech`, one of the constructors
        // `libdengjenFreeSynthesisEvent`'s `# Safety` doc covers; `error_ptr` is
        // null on this path so there's no message to leak.
        unsafe { libdengjenFreeSynthesisEvent(event) };
    }

    #[test]
    fn synthesis_event_with_error_carries_a_non_null_error_pointer_and_empty_data() {
        let event = SynthesisEvent::with_error(DengjenFFIError::invalid_utf8());

        assert_eq!(event.event_type, synth_event::SYNTH_EVENT_ERROR);
        assert!(!event.error_ptr.is_null());
        assert_eq!(event.len, 0);

        // SAFETY: `event.error_ptr` was just boxed by `with_error` and asserted non-null above;
        // it isn't freed until `libdengjenFreeSynthesisEvent` below.
        let code = unsafe { (*event.error_ptr).get_code().code() };
        assert_eq!(code, error_codes::INVALID_UTF8_SEQUENCE);

        // SAFETY: `event` came from `with_error`, one of the constructors
        // `libdengjenFreeSynthesisEvent`'s `# Safety` doc covers; this reclaims both the
        // leaked data slice and the boxed `ExternError` — message included — exactly once.
        unsafe { libdengjenFreeSynthesisEvent(event) };
    }

    #[test]
    fn synthesis_event_with_finished_carries_a_null_error_pointer_and_empty_data() {
        let event = SynthesisEvent::with_finished();

        assert_eq!(event.event_type, synth_event::SYNTH_EVENT_FINISHED);
        assert!(event.error_ptr.is_null());
        assert_eq!(event.len, 0);

        // SAFETY: see `synthesis_event_with_speech_...` above — same shape, no error to leak.
        unsafe { libdengjenFreeSynthesisEvent(event) };
    }

    extern "C" fn noop_callback(_event: SynthesisEvent, _user_data: *mut c_void) -> u8 {
        0
    }

    #[test]
    fn as_synth_output_config_carries_over_all_fields() {
        let params = SynthesisParams {
            struct_size: SYNTHESIS_PARAMS_V1_SIZE,
            mode: synth_mode::SYNTH_MODE_LAZY,
            rate: 60,
            volume: 80,
            pitch: 40,
            appended_silence_ms: 250,
            callback: Some(noop_callback),
            nonblocking: 0,
            user_data: std::ptr::null_mut(),
        };

        let config = params.as_synth_output_config();

        assert_eq!(config.rate, Some(60));
        assert_eq!(config.volume, Some(80));
        assert_eq!(config.pitch, Some(40));
        assert_eq!(config.appended_silence_ms, Some(250));
    }

    #[test]
    fn as_piper_synth_config_carries_over_speaker_and_synthesis_tuning_fields() {
        let synth_config = PiperSynthConfig {
            speaker: 3,
            length_scale: 1.2,
            noise_scale: 0.5,
            noise_w: 0.9,
        };

        let piper_config = synth_config.as_piper_synth_config();

        assert_eq!(piper_config.speaker, Some(3));
        assert_eq!(piper_config.length_scale, 1.2);
        assert_eq!(piper_config.noise_scale, 0.5);
        assert_eq!(piper_config.noise_w, 0.9);
    }

    #[test]
    fn committed_or_error_reports_a_failed_commit_as_an_error() {
        assert!(committed_or_error(true).is_ok());
        assert!(committed_or_error(false).is_err());
    }

    #[test]
    fn as_piper_synth_config_treats_speaker_zero_as_a_real_speaker_not_unset() {
        let synth_config = PiperSynthConfig {
            speaker: 0,
            length_scale: 1.0,
            noise_scale: 1.0,
            noise_w: 1.0,
        };
        assert_eq!(synth_config.as_piper_synth_config().speaker, Some(0));
    }

    #[test]
    fn as_piper_synth_config_maps_the_sentinel_to_no_speaker() {
        let synth_config = PiperSynthConfig {
            speaker: PIPER_SYNTH_CONFIG_NO_SPEAKER,
            length_scale: 1.0,
            noise_scale: 1.0,
            noise_w: 1.0,
        };
        assert_eq!(synth_config.as_piper_synth_config().speaker, None);
    }
}

#[cfg(test)]
mod ffi_flow_tests {
    use super::*;
    use dengjen_tts_core::{Audio, AudioInfo, AudioStreamIterator, Phonemes, SynthesisConfig};
    use std::thread::ThreadId;
    use std::time::{Duration, Instant};

    #[derive(Clone, Copy)]
    enum Script {
        Speak,
        PanicOnSpeak,
        SlowStream,
    }

    struct ScriptedModel {
        script: Script,
    }

    const SLOW_STREAM_CHUNKS: usize = 2000;

    impl DengjenModel for ScriptedModel {
        fn audio_output_info(&self) -> DengjenResult<AudioInfo> {
            Ok(AudioInfo {
                sample_rate: 16000,
                num_channels: 1,
                sample_width: 2,
            })
        }
        fn phonemize_text(&self, _text: &str) -> DengjenResult<Phonemes> {
            Ok(Phonemes::from(vec!["a".to_string()]))
        }
        fn speak_batch(&self, batches: Vec<String>) -> DengjenResult<Vec<Audio>> {
            batches
                .into_iter()
                .map(|phonemes| self.speak_one_sentence(phonemes))
                .collect()
        }
        fn speak_one_sentence(&self, _phonemes: String) -> dengjen_tts_core::DengjenAudioResult {
            if matches!(self.script, Script::PanicOnSpeak) {
                panic!("scripted model panic");
            }
            Ok(Audio::new(AudioSamples::from(vec![0.1; 160]), 16000, None))
        }
        fn get_default_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
            Ok(None)
        }
        fn get_fallback_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
            Ok(None)
        }
        fn set_fallback_synthesis_config(&self, _config: &SynthesisConfig) -> DengjenResult<()> {
            Ok(())
        }
        fn supports_streaming_output(&self) -> bool {
            matches!(self.script, Script::SlowStream)
        }
        fn stream_synthesis(
            &self,
            _phonemes: String,
            _chunk_size: usize,
            _chunk_padding: usize,
            _cancel_token: CancellationToken,
        ) -> DengjenResult<AudioStreamIterator<'_>> {
            Ok(Box::new((0..SLOW_STREAM_CHUNKS).map(|_| {
                std::thread::sleep(Duration::from_millis(5));
                Ok(AudioSamples::from(vec![0.1; 160]))
            })))
        }
    }

    struct EventLog {
        entries: Mutex<Vec<(i32, i32, ThreadId)>>,
    }

    impl EventLog {
        const fn new() -> Self {
            Self {
                entries: Mutex::new(Vec::new()),
            }
        }

        fn count(&self, event_type: i32) -> usize {
            let entries = self.entries.lock().unwrap();
            entries.iter().filter(|e| e.0 == event_type).count()
        }

        fn wait_for(&self, event_type: i32, at_least: usize) -> bool {
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                if self.count(event_type) >= at_least {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            false
        }
    }

    extern "C" fn record_event(event: SynthesisEvent, user_data: *mut c_void) -> u8 {
        // SAFETY: every test here passes a `&'static EventLog` as `user_data`.
        let log = unsafe { &*(user_data as *const EventLog) };
        let code = if event.error_ptr.is_null() {
            0
        } else {
            // SAFETY: a non-null `error_ptr` was produced by `SynthesisEvent::with_error`
            // and stays valid until `libdengjenFreeSynthesisEvent` below.
            unsafe { &*event.error_ptr }.get_code().code()
        };
        log.entries
            .lock()
            .unwrap()
            .push((event.event_type, code, std::thread::current().id()));
        // SAFETY: `event` was handed to this callback by the code under test and is freed once.
        unsafe { libdengjenFreeSynthesisEvent(event) };
        0
    }

    fn voice_with(script: Script) -> *mut DengjenVoice {
        let model: Arc<dyn DengjenModel + Send + Sync> = Arc::new(ScriptedModel { script });
        let synth = DengjenSpeechSynthesizer::new(model).unwrap();
        Box::into_raw(Box::new(DengjenVoice::from(synth)))
    }

    fn params(mode: i32, nonblocking: u8, log: &'static EventLog) -> SynthesisParams {
        SynthesisParams {
            struct_size: SYNTHESIS_PARAMS_V1_SIZE,
            mode,
            rate: 50,
            volume: 100,
            pitch: 50,
            appended_silence_ms: 0,
            callback: Some(record_event),
            nonblocking,
            user_data: (log as *const EventLog).cast_mut().cast(),
        }
    }

    fn speak(voice: *mut DengjenVoice, params: SynthesisParams) -> ExternError {
        let mut out_error = ExternError::default();
        // SAFETY: `voice` comes from `voice_with` and is live; the text is a valid C string.
        unsafe {
            libdengjenSpeak(voice, FfiStr::from_cstr(c"hello"), &params, &mut out_error);
        }
        out_error
    }

    #[test]
    fn nonblocking_speak_reports_events_from_a_worker_thread() {
        static LOG: EventLog = EventLog::new();
        let log = &LOG;
        let voice = voice_with(Script::Speak);
        let out_error = speak(voice, params(synth_mode::SYNTH_MODE_LAZY, 1, log));
        assert!(out_error.get_code().is_success());
        assert!(log.wait_for(synth_event::SYNTH_EVENT_FINISHED, 1));
        assert!(log.count(synth_event::SYNTH_EVENT_SPEECH) >= 1);
        assert_eq!(log.count(synth_event::SYNTH_EVENT_ERROR), 0);
        let entries = log.entries.lock().unwrap();
        assert!(entries
            .iter()
            .all(|(_, _, thread)| *thread != std::thread::current().id()));
        // SAFETY: `voice` is live and not used again.
        unsafe { libdengjenUnloadDengjenVoice(voice) };
    }

    #[test]
    fn nonblocking_worker_panic_becomes_an_inference_error_event() {
        static LOG: EventLog = EventLog::new();
        let log = &LOG;
        let voice = voice_with(Script::PanicOnSpeak);
        let out_error = speak(voice, params(synth_mode::SYNTH_MODE_LAZY, 1, log));
        assert!(out_error.get_code().is_success());
        assert!(log.wait_for(synth_event::SYNTH_EVENT_ERROR, 1));
        let entries = log.entries.lock().unwrap();
        let codes: Vec<i32> = entries
            .iter()
            .filter(|e| e.0 == synth_event::SYNTH_EVENT_ERROR)
            .map(|e| e.1)
            .collect();
        assert_eq!(codes, vec![error_codes::INFERENCE_ERROR]);
        // SAFETY: `voice` is live and not used again.
        unsafe { libdengjenUnloadDengjenVoice(voice) };
    }

    #[test]
    fn invalid_synthesis_mode_returns_the_mode_error_without_events() {
        static LOG: EventLog = EventLog::new();
        let log = &LOG;
        let voice = voice_with(Script::Speak);
        let out_error = speak(voice, params(99, 0, log));
        assert_eq!(
            out_error.get_code().code(),
            error_codes::INVALID_SYNTHESIS_MODE
        );
        assert!(log.entries.lock().unwrap().is_empty());
        // SAFETY: the message was allocated by `call_with_result`; `voice` is live and unused after.
        unsafe {
            out_error.manually_release();
            libdengjenUnloadDengjenVoice(voice);
        }
    }

    #[test]
    fn cancelling_a_realtime_synthesis_stops_the_stream_and_still_finishes() {
        static LOG: EventLog = EventLog::new();
        let log = &LOG;
        let voice = voice_with(Script::SlowStream);
        let out_error = speak(voice, params(synth_mode::SYNTH_MODE_REALTIME, 1, log));
        assert!(out_error.get_code().is_success());
        assert!(log.wait_for(synth_event::SYNTH_EVENT_SPEECH, 1));

        let mut cancel_error = ExternError::default();
        // SAFETY: `voice` is live; cancelling from the thread that started the synthesis is allowed.
        unsafe { libdengjenCancel(voice, &mut cancel_error) };
        assert!(cancel_error.get_code().is_success());

        assert!(log.wait_for(synth_event::SYNTH_EVENT_FINISHED, 1));
        assert!(log.count(synth_event::SYNTH_EVENT_SPEECH) < SLOW_STREAM_CHUNKS);
        // SAFETY: `voice` is live and not used again.
        unsafe { libdengjenUnloadDengjenVoice(voice) };
    }

    #[test]
    fn unloading_a_voice_stops_and_drains_a_running_lazy_synthesis() {
        static LOG: EventLog = EventLog::new();
        let log = &LOG;
        let voice = voice_with(Script::SlowStream);
        let out_error = speak(voice, params(synth_mode::SYNTH_MODE_LAZY, 1, log));
        assert!(out_error.get_code().is_success());
        assert!(log.wait_for(synth_event::SYNTH_EVENT_SPEECH, 1));

        // SAFETY: `voice` is live and not used again.
        unsafe { libdengjenUnloadDengjenVoice(voice) };

        let settled = log.entries.lock().unwrap().len();
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(log.entries.lock().unwrap().len(), settled);
        assert_eq!(log.count(synth_event::SYNTH_EVENT_FINISHED), 1);
        assert!(log.count(synth_event::SYNTH_EVENT_SPEECH) < SLOW_STREAM_CHUNKS);
    }

    static OTHER_VOICE: std::sync::atomic::AtomicPtr<DengjenVoice> =
        std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

    extern "C" fn unload_the_other_voice(event: SynthesisEvent, user_data: *mut c_void) -> u8 {
        let other = OTHER_VOICE.swap(std::ptr::null_mut(), Ordering::SeqCst);
        if !other.is_null() {
            // SAFETY: the test stores a live voice that nothing else uses afterwards.
            unsafe { libdengjenUnloadDengjenVoice(other) };
        }
        record_event(event, user_data)
    }

    #[test]
    fn unloading_another_voice_from_a_callback_still_drains_it() {
        static OWNER_LOG: EventLog = EventLog::new();
        static OTHER_LOG: EventLog = EventLog::new();
        let other = voice_with(Script::SlowStream);
        let out_error = speak(other, params(synth_mode::SYNTH_MODE_LAZY, 1, &OTHER_LOG));
        assert!(out_error.get_code().is_success());
        assert!(OTHER_LOG.wait_for(synth_event::SYNTH_EVENT_SPEECH, 1));
        OTHER_VOICE.store(other, Ordering::SeqCst);

        let owner = voice_with(Script::Speak);
        let mut owner_params = params(synth_mode::SYNTH_MODE_LAZY, 1, &OWNER_LOG);
        owner_params.callback = Some(unload_the_other_voice);
        let out_error = speak(owner, owner_params);
        assert!(out_error.get_code().is_success());
        assert!(OWNER_LOG.wait_for(synth_event::SYNTH_EVENT_FINISHED, 1));

        let settled = OTHER_LOG.entries.lock().unwrap().len();
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(OTHER_LOG.entries.lock().unwrap().len(), settled);
        assert_eq!(OTHER_LOG.count(synth_event::SYNTH_EVENT_FINISHED), 1);
        // SAFETY: `owner` is live and not used again.
        unsafe { libdengjenUnloadDengjenVoice(owner) };
    }

    #[test]
    fn load_voice_rejects_an_unknown_model_type_instead_of_falling_back_to_piper() {
        let dir = std::env::temp_dir().join("dengjen_capi_load_voice_unknown_model_type");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        std::fs::write(&path, r#"{"model_type": "bark"}"#).unwrap();
        let result = load_voice(&path);
        assert!(
            matches!(&result, Err(DengjenError::InvalidConfiguration(msg)) if msg.contains("bark")),
            "unknown model_type must be an explicit configuration error"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
