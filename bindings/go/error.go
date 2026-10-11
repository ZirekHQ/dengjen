package dengjen

/*
#include "libdengjen.h"
*/
import "C"
import (
	"fmt"
)

type FFIError struct {
	Code    int32
	Message string
}

func (e *FFIError) Error() string {
	if e.Message == "" {
		return fmt.Sprintf("libdengjen error %d", e.Code)
	}
	return e.Message
}

type ErrorCode int32

const (
	ErrorCodePanic                ErrorCode = C.ErrorCode_PANIC
	ErrorCodeInvalidHandle        ErrorCode = C.ErrorCode_INVALID_HANDLE
	ErrorCodeInvalidSynthesisMode ErrorCode = C.INVALID_SYNTHESIS_MODE
	ErrorCodeFailedToLoadResource ErrorCode = C.FAILED_TO_LOAD_RESOURCE
	ErrorCodePhonemizationError   ErrorCode = C.PHONEMIZATION_ERROR
	ErrorCodeOperationError       ErrorCode = C.OPERATION_ERROR
	ErrorCodeInvalidUTF8Sequence  ErrorCode = C.INVALID_UTF8_SEQUENCE
	ErrorCodeUnknownError         ErrorCode = C.UNKNOWN_ERROR
	ErrorCodeNullPointer          ErrorCode = C.NULL_POINTER
	ErrorCodeInferenceError       ErrorCode = C.INFERENCE_ERROR
	ErrorCodeInvalidConfiguration ErrorCode = C.INVALID_CONFIGURATION
	ErrorCodeUnsupportedOperation ErrorCode = C.UNSUPPORTED_OPERATION
)

// ErrorCode returns the libdengjen code, or ErrorCodeUnknownError when the
// raw code is not one the header defines.
func (e *FFIError) ErrorCode() ErrorCode {
	switch c := ErrorCode(e.Code); c {
	case ErrorCodePanic, ErrorCodeInvalidHandle, ErrorCodeInvalidSynthesisMode,
		ErrorCodeFailedToLoadResource, ErrorCodePhonemizationError, ErrorCodeOperationError,
		ErrorCodeInvalidUTF8Sequence, ErrorCodeUnknownError, ErrorCodeNullPointer,
		ErrorCodeInferenceError, ErrorCodeInvalidConfiguration, ErrorCodeUnsupportedOperation:
		return c
	default:
		return ErrorCodeUnknownError
	}
}

var ErrVoiceClosed = &FFIError{Code: -2, Message: "voice is closed"}

func checkError(cErr C.struct_ExternError) error {
	if cErr.code == 0 {
		return nil
	}
	msg := C.GoString(cErr.message)
	if cErr.message != nil {
		C.libdengjenFreeString(cErr.message)
	}
	return &FFIError{Code: int32(cErr.code), Message: msg}
}
