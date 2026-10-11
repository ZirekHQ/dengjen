package io.github.zirekhq.dengjen;

public enum SynthesisMode {
  LAZY(0),
  PARALLEL(1),
  REALTIME(2),
  BATCHED(3);

  private final int value;

  SynthesisMode(int value) {
    this.value = value;
  }

  int value() {
    return value;
  }
}
