package io.github.zirekhq.dengjen;

public final class SttException extends RuntimeException {
  private final SttErrorCode errorCode;

  SttException(SttErrorCode errorCode, String message) {
    super(
        message != null && !message.isEmpty()
            ? message
            : errorCode.name() + " (no message from libdengjen-stt)");
    this.errorCode = errorCode;
  }

  public SttErrorCode errorCode() {
    return errorCode;
  }
}
