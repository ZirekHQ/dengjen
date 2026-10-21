use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::OnceLock;

use dengjen_capi_jni_common::attach::with_attached;
use dengjen_capi_jni_common::guard::jni_failure;
use jni::objects::{GlobalRef, JClass, JStaticMethodID, JValue};
use jni::signature::{Primitive, ReturnType};
use jni::{JNIEnv, JavaVM};
use libdengjen::synth_event::{SYNTH_EVENT_ERROR, SYNTH_EVENT_SPEECH};
use libdengjen::SynthesisEvent;

use crate::ffi::read_message;

const VOICE_CLASS: &str = "io/github/zirekhq/dengjen/android/Voice";
const DISPATCH_SIG: &str =
    "(Lio/github/zirekhq/dengjen/android/SynthesisEventHandler;I[BILjava/lang/String;)Z";

pub struct Bridge {
    vm: JavaVM,
    class: GlobalRef,
    dispatch: JStaticMethodID,
}

static BRIDGE: OnceLock<Bridge> = OnceLock::new();

pub struct CallbackState {
    pub handler: GlobalRef,
}

pub struct Event {
    pub kind: i32,
    pub data: Vec<u8>,
    pub code: i32,
    pub message: String,
}

pub fn install(vm: JavaVM) -> jni::errors::Result<()> {
    let (class, dispatch) = {
        let mut env = vm.get_env()?;
        let class = env.find_class(VOICE_CLASS)?;
        let dispatch = env.get_static_method_id(&class, "dispatch", DISPATCH_SIG)?;
        (env.new_global_ref(&class)?, dispatch)
    };
    let _ = BRIDGE.set(Bridge {
        vm,
        class,
        dispatch,
    });
    Ok(())
}

pub fn decode(event: &SynthesisEvent) -> Event {
    let data = if event.event_type == SYNTH_EVENT_SPEECH && event.len > 0 && !event.data.is_null() {
        // SAFETY: SPEECH events carry `len` readable bytes at `data` until the event is freed.
        unsafe { std::slice::from_raw_parts(event.data, event.len as usize) }.to_vec()
    } else {
        Vec::new()
    };
    let (code, message) = if event.event_type == SYNTH_EVENT_ERROR && !event.error_ptr.is_null() {
        // SAFETY: ERROR events carry a valid ExternError until the event is freed.
        let err = unsafe { &*event.error_ptr };
        (err.get_code().code(), read_message(err.get_raw_message()))
    } else {
        (0, String::new())
    };
    Event {
        kind: event.event_type,
        data,
        code,
        message,
    }
}

pub extern "C" fn trampoline(event: SynthesisEvent, user_data: *mut c_void) -> u8 {
    let decoded = catch_unwind(AssertUnwindSafe(|| decode(&event)));
    // SAFETY: the event came from libdengjen's callback and is freed exactly once, after decoding.
    unsafe { libdengjen::libdengjenFreeSynthesisEvent(event) };
    let keep_going = decoded.ok().is_some_and(|ev| {
        // SAFETY: `user_data` is the CallbackState that nativeSpeak keeps alive for the whole blocking call.
        let state = unsafe { &*user_data.cast::<CallbackState>() };
        catch_unwind(AssertUnwindSafe(|| deliver(&ev, state))).unwrap_or(false)
    });
    u8::from(!keep_going)
}

pub fn deliver(ev: &Event, state: &CallbackState) -> bool {
    let Some(bridge) = BRIDGE.get() else {
        return false;
    };
    with_attached(&bridge.vm, |env| call_dispatch(env, bridge, state, ev)).unwrap_or(false)
}

fn call_dispatch(env: &mut JNIEnv, bridge: &Bridge, state: &CallbackState, ev: &Event) -> bool {
    let outcome = env.with_local_frame(8, |env| -> jni::errors::Result<bool> {
        let data = env.byte_array_from_slice(&ev.data)?;
        let message = env.new_string(&ev.message)?;
        let class = <&JClass>::from(bridge.class.as_obj());
        let args = [
            JValue::Object(state.handler.as_obj()).as_jni(),
            JValue::Int(ev.kind).as_jni(),
            JValue::Object(&data).as_jni(),
            JValue::Int(ev.code).as_jni(),
            JValue::Object(&message).as_jni(),
        ];
        // SAFETY: `dispatch` was resolved on `class` with DISPATCH_SIG and `args` match that signature.
        unsafe {
            env.call_static_method_unchecked(
                class,
                bridge.dispatch,
                ReturnType::Primitive(Primitive::Boolean),
                &args,
            )
        }?
        .z()
    });
    if env.exception_check().unwrap_or(true) {
        let _ = env.exception_clear();
        return false;
    }
    outcome.map_err(jni_failure).unwrap_or(false)
}
