package io.github.zirekhq.dengjen;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.invoke.MethodHandle;

/** Points a dynamically linked native library at the onnxruntime named by a system property. */
final class OnnxRuntime {
  static final String PATH_PROPERTY = "dengjen.onnxruntime.path";

  private static boolean configured;

  private OnnxRuntime() {}

  /**
   * Applies {@value #PATH_PROPERTY} once per process; without the property the native side falls
   * back to {@code ORT_DYLIB_PATH} and the platform loader search path.
   */
  static synchronized void configure() {
    String path = System.getProperty(PATH_PROPERTY);
    if (configured || path == null) {
      return;
    }
    MethodHandle setPath =
        DengjenLib.SET_ORT_DYLIB_PATH.orElseThrow(
            () ->
                new IllegalStateException(
                    "-D" + PATH_PROPERTY + " is set but this native library embeds onnxruntime"));
    try (Arena arena = Arena.ofConfined()) {
      MemorySegment outError = arena.allocate(DengjenLayouts.EXTERN_ERROR);
      try {
        setPath.invokeExact(arena.allocateFrom(path), outError);
      } catch (Throwable t) {
        throw new IllegalStateException("libdengjenSetOrtDylibPath downcall failed", t);
      }
      ErrorChecks.checkAndThrow(outError);
    }
    configured = true;
  }
}
