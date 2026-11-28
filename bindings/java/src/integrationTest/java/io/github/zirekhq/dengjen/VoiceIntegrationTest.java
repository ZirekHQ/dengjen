package io.github.zirekhq.dengjen;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatCode;
import static org.assertj.core.api.Assertions.assertThatThrownBy;
import static org.assertj.core.api.Assertions.within;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class VoiceIntegrationTest {
  @Test
  void loadReportsAnErrorForAMissingConfigPath() {
    assertThatThrownBy(() -> Voice.load("/nonexistent-dengjen-java-binding-test.json"))
        .isInstanceOf(DengjenException.class);
  }

  @Test
  void loadAudioInfoAndClose(@TempDir Path tempDir) {
    Voice voice = Voice.load(TestFixtures.syntheticPiperConfigPath(tempDir).toString());
    AudioInfo info = voice.getAudioInfo();
    assertThat(info.sampleRate()).isEqualTo(22050);
    assertThat(info.numChannels()).isEqualTo(1);

    voice.close();
    assertThatCode(voice::close).as("close must be idempotent").doesNotThrowAnyException();
  }

  @Test
  void synthConfigRoundTrip(@TempDir Path tempDir) {
    try (Voice voice = Voice.load(TestFixtures.syntheticPiperConfigPath(tempDir).toString())) {
      PiperSynthConfig cfg = voice.getDefaultSynthConfig();
      assertThat(cfg.noiseScale()).isCloseTo(0.667f, within(0.0001f));

      voice.setSynthConfig(
          new PiperSynthConfig(cfg.speaker(), 2.0f, cfg.noiseScale(), cfg.noiseW()));

      voice.setSynthesisParameter("noise_scale", 0.5f);
      Optional<Float> value = voice.getSynthesisParameter("noise_scale");
      assertThat(value).isPresent();
      assertThat(value.get()).isCloseTo(0.5f, within(0.0001f));

      assertThat(voice.getSynthesisParameter("no_such_key")).isEmpty();
    }
  }

  @Test
  void speakToFileWritesAWavFile(@TempDir Path tempDir) throws IOException {
    try (Voice voice = Voice.load(TestFixtures.syntheticPiperConfigPath(tempDir).toString())) {
      Path outPath = tempDir.resolve("output.wav");
      SynthesisParams params = new SynthesisParams(SynthesisMode.LAZY, 10, 100, 50, 0);
      boolean wrote = voice.speakToFile("Test.", params, outPath.toString());
      assertThat(wrote).isTrue();

      byte[] data = Files.readAllBytes(outPath);

      assertThat(data).hasSize(44 + 8000 * 2);
    }
  }

  @Test
  void speakLazyModeDeliversSpeechThenFinished(@TempDir Path tempDir) {
    try (Voice voice = Voice.load(TestFixtures.syntheticPiperConfigPath(tempDir).toString())) {
      List<EventType> events = new ArrayList<>();
      int[] totalBytes = {0};
      SynthesisParams params = new SynthesisParams(SynthesisMode.LAZY, 10, 100, 50, 0);
      voice.speak(
          "Test.",
          params,
          event -> {
            events.add(event.type());
            totalBytes[0] += event.data().length;
            assertThat(event.error()).isNull();
            return true;
          });
      assertThat(events).isNotEmpty();
      assertThat(events).last().isEqualTo(EventType.FINISHED);
      assertThat(totalBytes[0]).isEqualTo(8000 * 2);
    }
  }

  @Test
  void speakHandlerReturningFalseStopsEarly(@TempDir Path tempDir) {
    try (Voice voice = Voice.load(TestFixtures.syntheticPiperConfigPath(tempDir).toString())) {
      int[] calls = {0};
      SynthesisParams params = new SynthesisParams(SynthesisMode.LAZY, 10, 100, 50, 0);
      voice.speak(
          "Test.",
          params,
          event -> {
            calls[0]++;
            return false;
          });
      assertThat(calls[0]).isEqualTo(1);
    }
  }

  @Test
  void speakHandlerReturningFalseReleasesTheCallRegistration(@TempDir Path tempDir) {
    try (Voice voice = Voice.load(TestFixtures.syntheticPiperConfigPath(tempDir).toString())) {
      boolean[] released = {false};
      Runnable prevHook = SpeakTrampoline.testCallReleased.get();
      SpeakTrampoline.testCallReleased.set(() -> released[0] = true);
      try {
        SynthesisParams params = new SynthesisParams(SynthesisMode.LAZY, 10, 100, 50, 0);
        voice.speak("Test.", params, event -> false);
      } finally {
        SpeakTrampoline.testCallReleased.set(prevHook);
      }
      assertThat(released[0])
          .as(
              "the registry entry for an early-stopped speak() call was never released, leaking the handler")
          .isTrue();
    }
  }

  @Test
  void speakHandlerThrowingAnErrorDoesNotCrossTheNativeBoundaryAndReleasesTheRegistration(
      @TempDir Path tempDir) {
    try (Voice voice = Voice.load(TestFixtures.syntheticPiperConfigPath(tempDir).toString())) {
      boolean[] released = {false};
      Runnable prevHook = SpeakTrampoline.testCallReleased.get();
      SpeakTrampoline.testCallReleased.set(() -> released[0] = true);
      try {
        SynthesisParams params = new SynthesisParams(SynthesisMode.LAZY, 10, 100, 50, 0);

        assertThatCode(() ->
                voice.speak(
                    "Test.",
                    params,
                    event -> {
                      throw new AssertionError("boom");
                    }))
            .doesNotThrowAnyException();
      } finally {
        SpeakTrampoline.testCallReleased.set(prevHook);
      }
      assertThat(released[0])
          .as("the registry entry for a call whose handler threw an Error was never released")
          .isTrue();
    }
  }

  @Test
  void speakReleasesTheCallRegistrationWhenMarshallingThrows(@TempDir Path tempDir) {
    try (Voice voice = Voice.load(TestFixtures.syntheticPiperConfigPath(tempDir).toString())) {
      int[] releases = {0};
      Runnable prevHook = SpeakTrampoline.testCallReleased.get();
      SpeakTrampoline.testCallReleased.set(() -> releases[0]++);
      try {
        SynthesisParams params = new SynthesisParams(SynthesisMode.LAZY, 10, 100, 50, 0);

        assertThatThrownBy(() -> voice.speak(null, params, event -> true))
            .isInstanceOf(NullPointerException.class);
      } finally {
        SpeakTrampoline.testCallReleased.set(prevHook);
      }
      assertThat(releases[0])
          .as(
              "a speak() call that threw before reaching the downcall left its handler pinned in the registry")
          .isEqualTo(1);
    }
  }

  @Test
  void cancelOnAnIdleVoiceIsANoop(@TempDir Path tempDir) {
    try (Voice voice = Voice.load(TestFixtures.syntheticPiperConfigPath(tempDir).toString())) {
      assertThatCode(voice::cancel).doesNotThrowAnyException();
    }
  }

  @Test
  void speakRealtimeModeThenCancelDoesNotDeadlock(@TempDir Path tempDir)
      throws InterruptedException {
    try (Voice voice = Voice.load(TestFixtures.syntheticPiperConfigPath(tempDir).toString())) {
      SynthesisParams params = new SynthesisParams(SynthesisMode.REALTIME, 10, 100, 50, 0, true);
      voice.speak("Test.", params, event -> true);

      Thread canceller = new Thread(voice::cancel);
      canceller.start();
      canceller.join(5000);
      assertThat(canceller.isAlive())
          .as("cancel() did not return within 5s -- possible deadlock")
          .isFalse();
    }
  }
}
