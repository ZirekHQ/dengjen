package dengjen

/*
#cgo CFLAGS: -I${SRCDIR}/../../crates/frontends/capi
#cgo LDFLAGS: -L${SRCDIR}/../../target/release -llibdengjen
#include "libdengjen.h"
*/
import "C"

// init refuses to run against a native library built for a different ABI than the header these
// bindings were compiled with, instead of corrupting memory on the first call.
func init() {
	if C.libdengjenAbiVersion() != C.DENGJEN_ABI_VERSION {
		panic("dengjen: the native library's ABI version differs from the header these bindings were built with; build both from the same release")
	}
}
