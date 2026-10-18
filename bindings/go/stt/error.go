package stt

/*
#include "libdengjen_stt.h"
*/
import "C"
import (
	"fmt"
)

// Error is a libdengjen-stt failure. Match one with errors.Is against the sentinels below.
type Error struct {
	Code    int32
	Message string
}

func (e *Error) Error() string {
	if e.Message == "" {
		return fmt.Sprintf("libdengjen-stt error %d", e.Code)
	}
	return e.Message
}

// Is matches sentinel errors by code.
func (e *Error) Is(target error) bool {
	t, ok := target.(*Error)
	return ok && t.Code == e.Code
}

var (
	ErrInvalidConfiguration = &Error{Code: C.INVALID_CONFIGURATION}
	ErrMissingModelFile     = &Error{Code: C.MISSING_MODEL_FILE}
	ErrUnreadableModelFile  = &Error{Code: C.UNREADABLE_MODEL_FILE}
	ErrChecksumMismatch     = &Error{Code: C.CHECKSUM_MISMATCH}
	ErrModelLoadFailed      = &Error{Code: C.MODEL_LOAD_FAILED}
	ErrInvalidAudio         = &Error{Code: C.INVALID_AUDIO}
	ErrDecodeFailed         = &Error{Code: C.DECODE_FAILED}
	ErrClosed               = &Error{Code: C.ErrorCode_INVALID_HANDLE, Message: "recognizer is closed"}
)

func checkError(cErr C.struct_ExternError) error {
	if cErr.code == 0 {
		return nil
	}
	msg := C.GoString(cErr.message)
	if cErr.message != nil {
		C.libdengjenSttFreeString(cErr.message)
	}
	return &Error{Code: int32(cErr.code), Message: msg}
}
