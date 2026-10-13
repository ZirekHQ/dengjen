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

  @SuppressWarnings("unused")
  private static void recordingSetter(MemorySegment path, MemorySegment outError) {
    lastPath = path.reinterpret(Long.MAX_VALUE).getString(0);
  }

  @SuppressWarnings("unused")
  private static void rejectingSetter(MemorySegment path, MemorySegment outError) {
    outError
        .reinterpret(DengjenLayouts.EXTERN_ERROR.byteSize())
        .set(
            ValueLayout.JAVA_INT,
            DengjenLayouts.EXTERN_ERROR_CODE_OFFSET,
            ErrorCode.OPERATION_ERROR.code());
  }

  @SuppressWarnings("unused")
  private static void crashingSetter(MemorySegment path, MemorySegment outError) {
    throw new UnsupportedOperationException("boom");
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
  void passesThePathToTheNativeSetter() throws Exception {
    OnnxRuntime.apply("/opt/onnxruntime/libonnxruntime.so", setter("recordingSetter"));

    assertEquals("/opt/onnxruntime/libonnxruntime.so", lastPath);
  }

  @Test
  void rejectsThePropertyWhenTheNativeLibraryEmbedsOnnxruntime() {
    IllegalStateException error =
        assertThrows(IllegalStateException.class, () -> OnnxRuntime.apply("/x", Optional.empty()));

    assertTrue(error.getMessage().contains(OnnxRuntime.PATH_PROPERTY));
  }

  @Test
  void surfacesAnErrorTheNativeSetterReports() throws Exception {
    DengjenException error =
        assertThrows(
            DengjenException.class, () -> OnnxRuntime.apply("/x", setter("rejectingSetter")));

    assertSame(ErrorCode.OPERATION_ERROR, error.errorCode());
  }

  @Test
  void wrapsAFailedDowncall() throws Exception {
    IllegalStateException error =
        assertThrows(
            IllegalStateException.class, () -> OnnxRuntime.apply("/x", setter("crashingSetter")));

    assertTrue(error.getMessage().contains("libdengjenSetOrtDylibPath"));
  }
}
