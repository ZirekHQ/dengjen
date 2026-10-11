package io.github.zirekhq.dengjen;

import static org.assertj.core.api.Assertions.assertThat;
import static org.junit.jupiter.api.Assumptions.assumeTrue;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class RealVoiceE2ETest {
  @Test
  void synthesizesRealAudioFromARealVoice(@TempDir Path tempDir) throws IOException {
    String configPath = System.getenv("DENGJEN_KOKORO_TEST_VOICE_CONFIG");
    String reason =
        "SKIPPED, no real voice exercised: set DENGJEN_KOKORO_TEST_VOICE_CONFIG to a real Kokoro"
            + " voice config (set DENGJEN_REQUIRE_REAL_VOICE=1 to make this a failure)";
    if ("1".equals(System.getenv("DENGJEN_REQUIRE_REAL_VOICE"))) {
      assertThat(configPath).as("DENGJEN_REQUIRE_REAL_VOICE=1 but %s", reason).isNotNull();
    }
    assumeTrue(configPath != null, reason);

    try (Voice voice = Voice.load(configPath)) {
      Path outPath = tempDir.resolve("output.wav");
      SynthesisParams params = new SynthesisParams(SynthesisMode.LAZY, 10, 100, 50, 0);
      boolean wrote = voice.speakToFile("Hello, world!", params, outPath.toString());
      assertThat(wrote).isTrue();

      byte[] data = Files.readAllBytes(outPath);
      assertThat(data)
          .as("expected a WAV file with audio samples beyond the header")
          .hasSizeGreaterThan(44);
    }
  }
}
