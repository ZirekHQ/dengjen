package io.github.zirekhq.dengjen;

import java.util.Objects;

public record SynthesisParams(
    SynthesisMode mode,
    int rate,
    int volume,
    int pitch,
    int appendedSilenceMs,
    boolean nonblocking) {

  public SynthesisParams {
    Objects.requireNonNull(mode, "mode");
    if (appendedSilenceMs < 0) {
      throw new IllegalArgumentException(
          "appendedSilenceMs must be >= 0, got " + appendedSilenceMs);
    }
    requireRange(rate, "rate");
    requireRange(volume, "volume");
    requireRange(pitch, "pitch");
  }

  public SynthesisParams(
      SynthesisMode mode, int rate, int volume, int pitch, int appendedSilenceMs) {
    this(mode, rate, volume, pitch, appendedSilenceMs, false);
  }

  private static void requireRange(int value, String name) {
    if (value < 0 || value > 100) {
      throw new IllegalArgumentException(name + " must be 0-100, got " + value);
    }
  }
}
