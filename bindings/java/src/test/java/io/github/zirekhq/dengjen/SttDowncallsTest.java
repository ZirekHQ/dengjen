package io.github.zirekhq.dengjen;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;
import static org.junit.jupiter.api.Assumptions.assumeTrue;

import org.junit.jupiter.api.Test;

class SttDowncallsTest {
  private static final String MODEL_DIR = "dengjen.stt.model.dir";
  private static final String MODEL_SHA = "dengjen.stt.model.sha256";

  @Test
  void loadWithAnUnknownDirectoryReportsUnreadableModelFile() {
    assumeTrue(System.getProperty(SttNativeLoader.DIR_PROPERTY) != null);
    assertThatThrownBy(() -> Recognizer.load("/definitely/not/here", "0".repeat(64), 1))
        .isInstanceOfSatisfying(
            SttException.class,
            e -> assertThat(e.errorCode()).isEqualTo(SttErrorCode.UNREADABLE_MODEL_FILE));
  }

  @Test
  void aClosedRecognizerRejectsRecognitionAndClosesTwice() {
    assumeTrue(System.getProperty(SttNativeLoader.DIR_PROPERTY) != null);
    Recognizer closed = Recognizer.closedForTest();
    closed.close();
    closed.close();
    assertThatThrownBy(() -> closed.recognize(new float[1600], 16_000))
        .isInstanceOfSatisfying(
            SttException.class,
            e -> assertThat(e.errorCode()).isEqualTo(SttErrorCode.INVALID_HANDLE));
  }

  @Test
  void emptyAudioAndZeroRateAreInvalidAudio() {
    assumeTrue(System.getProperty(MODEL_DIR) != null && System.getProperty(MODEL_SHA) != null);
    try (Recognizer r =
        Recognizer.load(System.getProperty(MODEL_DIR), System.getProperty(MODEL_SHA), 1)) {
      assertThatThrownBy(() -> r.recognize(new float[0], 16_000))
          .isInstanceOfSatisfying(
              SttException.class,
              e -> assertThat(e.errorCode()).isEqualTo(SttErrorCode.INVALID_AUDIO));
      assertThatThrownBy(() -> r.recognize(new float[1600], 0))
          .isInstanceOfSatisfying(
              SttException.class,
              e -> assertThat(e.errorCode()).isEqualTo(SttErrorCode.INVALID_AUDIO));
    }
  }

  @Test
  void silenceRecognizesWithoutError() {
    assumeTrue(System.getProperty(MODEL_DIR) != null && System.getProperty(MODEL_SHA) != null);
    try (Recognizer r =
        Recognizer.load(System.getProperty(MODEL_DIR), System.getProperty(MODEL_SHA), 1)) {
      assertThat(r.recognize(new float[16_000], 16_000)).isNotNull();
    }
  }

  @Test
  void closeDuringRecognitionWaitsInsteadOfFreeingTheModel() throws Exception {
    assumeTrue(System.getProperty(MODEL_DIR) != null && System.getProperty(MODEL_SHA) != null);
    Recognizer r = Recognizer.load(System.getProperty(MODEL_DIR), System.getProperty(MODEL_SHA), 1);
    java.util.concurrent.atomic.AtomicReference<Object> outcome =
        new java.util.concurrent.atomic.AtomicReference<>();
    Thread worker =
        new Thread(
            () -> {
              try {
                outcome.set(r.recognize(new float[16_000 * 6], 16_000));
              } catch (SttException e) {
                outcome.set(e);
              }
            });
    worker.start();
    worker.join(300);
    r.close();
    worker.join(120_000);
    assertThat(worker.isAlive()).isFalse();
    assertThat(outcome.get()).isNotNull();
  }
}
