package io.github.zirekhq.dengjen;

import java.io.IOException;
import java.io.UncheckedIOException;
import java.nio.file.Files;
import java.nio.file.Path;

final class TestFixtures {
  private TestFixtures() {}

  static Path syntheticPiperConfigPath(Path tempDir) {
    try {
      Path modelSrc =
          Path.of("../../crates/dengjen/models/piper/tests/fixtures/synthetic_piper_batch.onnx");
      Path modelDst = tempDir.resolve("model.onnx");
      Files.copy(modelSrc, modelDst);

      String configJson =
          """
          {
              "key": null,
              "language": {"code": "en-US"},
              "audio": {"sample_rate": 22050, "quality": null},
              "num_speakers": 1,
              "speaker_id_map": {"default": 0},
              "streaming": false,
              "espeak": {"voice": "en-us"},
              "inference": {"noise_scale": 0.667, "length_scale": 1.0, "noise_w": 0.8},
              "num_symbols": 8,
              "phoneme_map": {},
              "phoneme_id_map": {"^": [1], "$": [2], "_": [3], "t": [4]},
              "phoneme_type": "text",
              "hop_length": 256
          }
          """;
      Path configPath = tempDir.resolve("model.onnx.json");
      Files.writeString(configPath, configJson);
      return configPath;
    } catch (IOException e) {
      throw new UncheckedIOException(e);
    }
  }
}
