package io.github.zirekhq.dengjen;

import static org.assertj.core.api.Assertions.assertThat;

import org.junit.jupiter.api.Test;

class SttErrorCodeTest {
  @Test
  void mapsEveryHeaderCode() {
    assertThat(SttErrorCode.fromCode(40)).isEqualTo(SttErrorCode.MISSING_MODEL_FILE);
    assertThat(SttErrorCode.fromCode(41)).isEqualTo(SttErrorCode.UNREADABLE_MODEL_FILE);
    assertThat(SttErrorCode.fromCode(42)).isEqualTo(SttErrorCode.CHECKSUM_MISMATCH);
    assertThat(SttErrorCode.fromCode(43)).isEqualTo(SttErrorCode.MODEL_LOAD_FAILED);
    assertThat(SttErrorCode.fromCode(44)).isEqualTo(SttErrorCode.INVALID_AUDIO);
    assertThat(SttErrorCode.fromCode(45)).isEqualTo(SttErrorCode.DECODE_FAILED);
    assertThat(SttErrorCode.fromCode(-1000)).isEqualTo(SttErrorCode.INVALID_HANDLE);
  }

  @Test
  void unknownCodesBecomeUnknown() {
    assertThat(SttErrorCode.fromCode(9999)).isEqualTo(SttErrorCode.UNKNOWN);
  }
}
