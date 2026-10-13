package io.github.zirekhq.dengjen;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;
import java.lang.invoke.MethodHandle;
import java.lang.invoke.MethodHandles;
import java.lang.invoke.MethodType;
import java.util.Optional;
import org.junit.jupiter.api.Test;

class OnnxRuntimeTest {

  private static String lastPath;
  private static MemorySegment lastOutError;

  private static String read(MemorySegment path) {
    return path.reinterpret(Long.MAX_VALUE).getString(0);
  }

  @SuppressWarnings("unused")
  private static void recordingSetter(MemorySegment path, MemorySegment outError) {
    lastPath = read(path);
    lastOutError = outError;
  }

  @SuppressWarnings("unused")
  private static void rejectingSetter(MemorySegment path, MemorySegment outError) {
    lastPath = read(path);
    outError
        .reinterpret(DengjenLayouts.EXTERN_ERROR.byteSize())
        .set(
            ValueLayout.JAVA_INT,
            DengjenLayouts.EXTERN_ERROR_CODE_OFFSET,
            ErrorCode.OPERATION_ERROR.code());
  }

  @SuppressWarnings("unused")
  private static void crashingSetter(MemorySegment path, MemorySegment outError) {
    throw new UnsupportedOperationException("boom: " + read(path) + " " + outError.byteSize());
  }

  private static Optional<MethodHandle> setter(String name) throws ReflectiveOperationException {
    return Optional.of(
        MethodHandles.lookup()
            .findStatic(
                OnnxRuntimeTest.class,
                name,
                MethodType.methodType(void.class, MemorySegment.class, MemorySegment.class)));
  }

  @Test
  void passesThePathAndAnOutErrorToTheNativeSetter() throws Exception {
    OnnxRuntime.apply("/opt/onnxruntime/libonnxruntime.so", setter("recordingSetter"));

    assertEquals("/opt/onnxruntime/libonnxruntime.so", lastPath);
    assertEquals(DengjenLayouts.EXTERN_ERROR.byteSize(), lastOutError.byteSize());
  }

  @Test
  void rejectsThePropertyWhenTheNativeLibraryEmbedsOnnxruntime() {
    Optional<MethodHandle> missing = Optional.empty();

    IllegalStateException error =
        assertThrows(IllegalStateException.class, () -> OnnxRuntime.apply("/x", missing));

    assertTrue(error.getMessage().contains(OnnxRuntime.PATH_PROPERTY));
  }

  @Test
  void surfacesAnErrorTheNativeSetterReports() throws Exception {
    Optional<MethodHandle> rejecting = setter("rejectingSetter");

    DengjenException error =
        assertThrows(DengjenException.class, () -> OnnxRuntime.apply("/x", rejecting));

    assertSame(ErrorCode.OPERATION_ERROR, error.errorCode());
    assertEquals("/x", lastPath);
  }

  @Test
  void wrapsAFailedDowncall() throws Exception {
    Optional<MethodHandle> crashing = setter("crashingSetter");

    IllegalStateException error =
        assertThrows(IllegalStateException.class, () -> OnnxRuntime.apply("/x", crashing));

    assertTrue(error.getMessage().contains("libdengjenSetOrtDylibPath"));
  }

  @Test
  void configureWithoutAPathLeavesTheNativeSideAlone() {
    OnnxRuntime.configure(null, Optional.empty());
  }

  @Test
  void configureAppliesThePathOnceAndIgnoresLaterCalls() throws Exception {
    OnnxRuntime.configure("/first", setter("recordingSetter"));
    OnnxRuntime.configure("/second", setter("rejectingSetter"));

    assertEquals("/first", lastPath);
  }
}
