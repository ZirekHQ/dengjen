package io.github.zirekhq.dengjen.android;

public record SynthesisParams(
    SynthesisMode mode, int rate, int volume, int pitch, int appendedSilenceMs) {
  private static final int INVALID_CONFIGURATION = 24;

  public SynthesisParams {
    requireByte("rate", rate);
    requireByte("volume", volume);
    requireByte("pitch", pitch);
    if (appendedSilenceMs < 0) {
      throw invalid("appendedSilenceMs", appendedSilenceMs);
    }
  }

  private static void requireByte(String name, int value) {
    if (value < 0 || value > 255) {
      throw invalid(name, value);
    }
  }

  private static DengjenException invalid(String name, int value) {
    return new DengjenException(INVALID_CONFIGURATION, name + " out of range: " + value);
  }
}
