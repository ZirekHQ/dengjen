package io.github.zirekhq.dengjen.android;

public final class DengjenException extends RuntimeException {
  private final int code;

  public DengjenException(int code, String message) {
    super(message);
    this.code = code;
  }

  public int code() {
    return code;
  }
}
