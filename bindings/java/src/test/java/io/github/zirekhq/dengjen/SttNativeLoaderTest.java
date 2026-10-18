package io.github.zirekhq.dengjen;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

import java.util.List;
import org.junit.jupiter.api.Test;

class SttNativeLoaderTest {
  @Test
  void dependenciesLoadBeforeTheLibraryThatNeedsThem() {
    List<String> manifest =
        List.of("libdengjen_stt.so", "libsherpa-onnx-c-api.so", "libonnxruntime.so.1.27.0");
    assertThat(SttNativeLoader.loadOrder(manifest))
        .containsExactly(
            "libonnxruntime.so.1.27.0", "libsherpa-onnx-c-api.so", "libdengjen_stt.so");
  }

  @Test
  void renamedSherpaRuntimeStillLoadsFirst() {
    List<String> manifest =
        List.of("libsherpa-onnx-c-api.so", "libonnxruntime-sherpa.so", "libdengjen_stt.so");
    assertThat(SttNativeLoader.loadOrder(manifest))
        .containsExactly(
            "libonnxruntime-sherpa.so", "libsherpa-onnx-c-api.so", "libdengjen_stt.so");
  }

  @Test
  void anUnrelatedFileIsRejected() {
    List<String> manifest = List.of("README.txt");
    assertThatThrownBy(() -> SttNativeLoader.loadOrder(manifest))
        .isInstanceOf(IllegalStateException.class);
  }
}
