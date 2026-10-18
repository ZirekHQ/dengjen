package io.github.zirekhq.dengjen;

public enum SttErrorCode {
  PANIC(-1),
  INVALID_HANDLE(-1000),
  INVALID_UTF8_SEQUENCE(20),
  NULL_POINTER(22),
  INVALID_CONFIGURATION(24),
  MISSING_MODEL_FILE(40),
  UNREADABLE_MODEL_FILE(41),
  CHECKSUM_MISMATCH(42),
  MODEL_LOAD_FAILED(43),
  INVALID_AUDIO(44),
  DECODE_FAILED(45),
  UNKNOWN(Integer.MIN_VALUE);

  private final int code;

  SttErrorCode(int code) {
    this.code = code;
  }

  public int code() {
    return code;
  }

  static SttErrorCode fromCode(int code) {
    for (SttErrorCode value : values()) {
      if (value.code == code) {
        return value;
      }
    }
    return UNKNOWN;
  }
}
