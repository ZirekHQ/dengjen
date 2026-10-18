package io.github.zirekhq.dengjen;

import static java.lang.foreign.ValueLayout.ADDRESS;
import static java.lang.foreign.ValueLayout.JAVA_INT;
import static java.lang.foreign.ValueLayout.JAVA_LONG;

import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.Linker;
import java.lang.foreign.MemoryLayout;
import java.lang.foreign.SymbolLookup;
import java.lang.invoke.MethodHandle;

final class SttLib {
  /** Must equal {@code STT_ABI_VERSION} in {@code libdengjen_stt.h}. */
  static final int EXPECTED_ABI_VERSION = 1;

  private static final Linker LINKER = Linker.nativeLinker();
  private static final SymbolLookup LOOKUP = SttNativeLoader.load();

  private static MethodHandle handle(String symbol, FunctionDescriptor descriptor) {
    return LINKER.downcallHandle(
        LOOKUP
            .find(symbol)
            .orElseThrow(() -> new IllegalStateException("missing symbol: " + symbol)),
        descriptor);
  }

  static {
    int actual;
    try {
      actual =
          (int) handle("libdengjenSttAbiVersion", FunctionDescriptor.of(JAVA_INT)).invokeExact();
    } catch (Throwable t) {
      throw new ExceptionInInitializerError(t);
    }
    if (actual != EXPECTED_ABI_VERSION) {
      throw new IllegalStateException(
          "native library ABI " + actual + " != bindings ABI " + EXPECTED_ABI_VERSION);
    }
  }

  static final MethodHandle LOAD =
      handle(
          "libdengjenSttLoad", FunctionDescriptor.of(ADDRESS, ADDRESS, ADDRESS, ADDRESS, ADDRESS));

  static final MethodHandle RECOGNIZE =
      handle(
          "libdengjenSttRecognize",
          FunctionDescriptor.of(ADDRESS, ADDRESS, ADDRESS, JAVA_LONG, JAVA_INT, ADDRESS));

  static final MethodHandle FREE_STRING =
      handle("libdengjenSttFreeString", FunctionDescriptor.ofVoid(ADDRESS));

  static final MethodHandle UNLOAD =
      handle("libdengjenSttUnload", FunctionDescriptor.ofVoid(ADDRESS));

  static final MemoryLayout STT_CONFIG =
      MemoryLayout.structLayout(JAVA_INT.withName("struct_size"), JAVA_INT.withName("num_threads"));

  private SttLib() {}
}
