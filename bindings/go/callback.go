package dengjen

/*
#include "libdengjen.h"
*/
import "C"
import (
	"runtime"
	"sync"
	"sync/atomic"
	"unsafe"
)

type EventType int32

const (
	EventSpeech   EventType = C.SYNTH_EVENT_SPEECH
	EventFinished EventType = C.SYNTH_EVENT_FINISHED
	EventError    EventType = C.SYNTH_EVENT_ERROR
)

type SynthesisEvent struct {
	Type EventType
	Data []byte
	Err  error
}

type callbackToken struct{ id uint64 }

var callbackSeq atomic.Uint64

type callbackEntry struct {
	fn     func(SynthesisEvent) bool
	pinner runtime.Pinner
}

var (
	callbackRegistryMu sync.Mutex
	callbackRegistry   = map[*callbackToken]*callbackEntry{}
)

func registerCallback(fn func(SynthesisEvent) bool) *callbackToken {
	token := &callbackToken{id: callbackSeq.Add(1)}
	entry := &callbackEntry{fn: fn}
	entry.pinner.Pin(token)

	callbackRegistryMu.Lock()
	callbackRegistry[token] = entry
	callbackRegistryMu.Unlock()
	return token
}

func releaseCallback(token *callbackToken) {
	callbackRegistryMu.Lock()
	entry, ok := callbackRegistry[token]
	delete(callbackRegistry, token)
	callbackRegistryMu.Unlock()
	if !ok {
		return
	}
	entry.pinner.Unpin()
	if testCallbackDataReleased != nil {
		testCallbackDataReleased()
	}
}

var testCallbackDataReleased func()

// invokeCallback reports false (stop) when the user callback panics, so the
// panic never unwinds across the Rust frames above.
func invokeCallback(fn func(SynthesisEvent) bool, ev SynthesisEvent) (wantsMore bool) {
	defer func() {
		if r := recover(); r != nil {
			wantsMore = false
		}
	}()
	return fn(ev)
}

//export goDengjenSpeechCallback
func goDengjenSpeechCallback(event C.struct_SynthesisEvent, userData unsafe.Pointer) C.uint8_t {
	token := (*callbackToken)(userData)
	callbackRegistryMu.Lock()
	entry := callbackRegistry[token]
	callbackRegistryMu.Unlock()
	var onEvent func(SynthesisEvent) bool
	if entry != nil {
		onEvent = entry.fn
	}

	goEvent := SynthesisEvent{Type: EventType(event.event_type)}
	switch event.event_type {
	case C.SYNTH_EVENT_SPEECH:
		if event.len > 0 {
			goEvent.Data = C.GoBytes(unsafe.Pointer(event.data), C.int(event.len))
		}
	case C.SYNTH_EVENT_ERROR:
		if event.error_ptr != nil {
			goEvent.Err = &FFIError{
				Code:    int32(event.error_ptr.code),
				Message: C.GoString(event.error_ptr.message),
			}
		}
	}

	C.libdengjenFreeSynthesisEvent(event)

	wantsMore := onEvent == nil || invokeCallback(onEvent, goEvent)

	terminal := event.event_type == C.SYNTH_EVENT_FINISHED || event.event_type == C.SYNTH_EVENT_ERROR
	if terminal || !wantsMore {
		releaseCallback(token)
	}
	if wantsMore {
		return 0
	}
	return 1
}
