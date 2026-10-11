package dengjen

/*
#include <stdlib.h>
#include "libdengjen.h"

// _cgo_export.h (declaring the //export'd goDengjenSpeechCallback) is itself
// generated from this preamble, so #include-ing it here is a chicken-and-egg
// problem. Redeclare the prototype instead, matching the signature exactly.
extern uint8_t goDengjenSpeechCallback(struct SynthesisEvent event, void *userData);
*/
import "C"
import (
	"runtime"
	"unsafe"
)

const (
	SynthModeLazy     int32 = C.SYNTH_MODE_LAZY
	SynthModeParallel int32 = C.SYNTH_MODE_PARALLEL
	SynthModeRealtime int32 = C.SYNTH_MODE_REALTIME
	SynthModeBatched  int32 = C.SYNTH_MODE_BATCHED
)

type SynthesisParams struct {
	Mode              int32
	Rate              uint8
	Volume            uint8
	Pitch             uint8
	AppendedSilenceMs uint32
	Nonblocking       bool
}

func boolToUint8(b bool) C.uint8_t {
	if b {
		return 1
	}
	return 0
}

// newCParams allocates the parameter struct in C memory. The caller sets `callback` and
// `user_data` where needed and frees it with C.free.
func newCParams(params SynthesisParams) *C.struct_SynthesisParams {
	cParams := (*C.struct_SynthesisParams)(C.calloc(1, C.sizeof_struct_SynthesisParams))
	cParams.struct_size = C.uint32_t(C.sizeof_struct_SynthesisParams)
	cParams.mode = C.int32_t(params.Mode)
	cParams.rate = C.uint8_t(params.Rate)
	cParams.volume = C.uint8_t(params.Volume)
	cParams.pitch = C.uint8_t(params.Pitch)
	cParams.appended_silence_ms = C.uint32_t(params.AppendedSilenceMs)
	cParams.nonblocking = boolToUint8(params.Nonblocking)
	return cParams
}

func (v *Voice) SpeakToFile(text string, params SynthesisParams, outFilename string) (bool, error) {
	if v.ptr == nil {
		return false, ErrVoiceClosed
	}
	cText := C.CString(text)
	defer C.free(unsafe.Pointer(cText))
	cOutFilename := C.CString(outFilename)
	defer C.free(unsafe.Pointer(cOutFilename))

	cParams := newCParams(params)
	defer C.free(unsafe.Pointer(cParams))
	var cErr C.struct_ExternError
	wrote := C.libdengjenSpeakToFile(v.ptr, C.FfiStr(cText), cParams, C.FfiStr(cOutFilename), &cErr)
	if err := checkError(cErr); err != nil {
		return false, err
	}
	runtime.KeepAlive(v)
	return wrote != 0, nil
}

func (v *Voice) Speak(text string, params SynthesisParams, onEvent func(SynthesisEvent) bool) error {
	if v.ptr == nil {
		return ErrVoiceClosed
	}
	cText := C.CString(text)
	defer C.free(unsafe.Pointer(cText))

	token := registerCallback(onEvent)

	cParams := newCParams(params)
	defer C.free(unsafe.Pointer(cParams))
	cParams.callback = C.SpeechSynthesisCallback(C.goDengjenSpeechCallback)
	cParams.user_data = unsafe.Pointer(token)

	var cErr C.struct_ExternError
	C.libdengjenSpeak(v.ptr, C.FfiStr(cText), cParams, &cErr)
	if err := checkError(cErr); err != nil {

		releaseCallback(token)
		return err
	}
	runtime.KeepAlive(v)
	return nil
}

func (v *Voice) Cancel() error {
	if v.ptr == nil {
		return ErrVoiceClosed
	}
	var cErr C.struct_ExternError
	C.libdengjenCancel(v.ptr, &cErr)
	runtime.KeepAlive(v)
	return checkError(cErr)
}
