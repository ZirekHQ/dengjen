package stt

/*
#cgo CFLAGS: -I${SRCDIR}/../../../crates/frontends/capi-stt
#cgo LDFLAGS: -L${SRCDIR}/../../../target/release -llibdengjen_stt
#include <stdlib.h>
#include "libdengjen_stt.h"
*/
import "C"
import (
	"runtime"
	"sync"
	"unsafe"
)

// init refuses to run against a native library built for a different ABI than the header these
// bindings were compiled with, instead of corrupting memory on the first call.
func init() {
	if C.libdengjenSttAbiVersion() != C.STT_ABI_VERSION {
		panic("stt: the native library's ABI version differs from the header these bindings were built with; build both from the same release")
	}
}

// Recognizer transcribes audio with a loaded model. It is safe for concurrent use.
type Recognizer struct {
	mu  sync.Mutex
	ptr *C.struct_DengjenRecognizer
}

// Load opens the model in modelDir after checking that its model file hashes to sha256.
func Load(modelDir, sha256 string, numThreads int32) (*Recognizer, error) {
	cDir := C.CString(modelDir)
	defer C.free(unsafe.Pointer(cDir))
	cSum := C.CString(sha256)
	defer C.free(unsafe.Pointer(cSum))

	cfg := C.struct_SttConfig{
		struct_size: C.uint32_t(unsafe.Sizeof(C.struct_SttConfig{})),
		num_threads: C.int32_t(numThreads),
	}
	var cErr C.struct_ExternError
	ptr := C.libdengjenSttLoad(cDir, &cfg, cSum, &cErr)
	if err := checkError(cErr); err != nil {
		return nil, err
	}
	r := &Recognizer{ptr: ptr}
	runtime.SetFinalizer(r, (*Recognizer).Close)
	return r, nil
}

// Recognize transcribes mono samples in [-1, 1] recorded at sampleRate.
func (r *Recognizer) Recognize(samples []float32, sampleRate int32) (string, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.ptr == nil {
		return "", ErrClosed
	}
	var first *C.float
	if len(samples) > 0 {
		first = (*C.float)(unsafe.Pointer(&samples[0]))
	}
	var cErr C.struct_ExternError
	text := C.libdengjenSttRecognize(r.ptr, first, C.size_t(len(samples)), C.int32_t(sampleRate), &cErr)
	if err := checkError(cErr); err != nil {
		return "", err
	}
	defer C.libdengjenSttFreeString(text)
	return C.GoString(text), nil
}

// Close releases the model. Closing twice is a no-op.
func (r *Recognizer) Close() error {
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.ptr == nil {
		return nil
	}
	C.libdengjenSttUnload(r.ptr)
	r.ptr = nil
	runtime.SetFinalizer(r, nil)
	return nil
}
