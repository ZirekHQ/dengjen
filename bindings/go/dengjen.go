package dengjen

/*
#cgo CFLAGS: -I${SRCDIR}/../../crates/frontends/capi
#cgo LDFLAGS: -L${SRCDIR}/../../target/release -llibdengjen
#include "libdengjen.h"
*/
import "C"

import "fmt"

// init refuses to run against a native library built for a different ABI than the header these
// bindings were compiled with, instead of corrupting memory on the first call.
func init() {
	got, want := uint32(C.libdengjenAbiVersion()), uint32(C.DENGJEN_ABI_VERSION)
	if got != want {
		panic(fmt.Sprintf("dengjen: native library ABI version %d does not match the bindings' %d", got, want))
	}
}
