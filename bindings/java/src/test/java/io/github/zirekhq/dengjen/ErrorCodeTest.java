package io.github.zirekhq.dengjen;

import static org.assertj.core.api.Assertions.assertThat;

import org.junit.jupiter.api.Test;

class ErrorCodeTest {
  @Test
  void everyConstantMatchesLibdengjenHeader() {
    assertThat(ErrorCode.PANIC.code()).isEqualTo(-1);
    assertThat(ErrorCode.INVALID_HANDLE.code()).isEqualTo(-1000);
    assertThat(ErrorCode.INVALID_SYNTHESIS_MODE.code()).isEqualTo(16);
    assertThat(ErrorCode.FAILED_TO_LOAD_RESOURCE.code()).isEqualTo(17);
    assertThat(ErrorCode.PHONEMIZATION_ERROR.code()).isEqualTo(18);
    assertThat(ErrorCode.OPERATION_ERROR.code()).isEqualTo(19);
    assertThat(ErrorCode.INVALID_UTF8_SEQUENCE.code()).isEqualTo(20);
    assertThat(ErrorCode.UNKNOWN_ERROR.code()).isEqualTo(21);
    assertThat(ErrorCode.NULL_POINTER.code()).isEqualTo(22);
    assertThat(ErrorCode.INFERENCE_ERROR.code()).isEqualTo(23);
    assertThat(ErrorCode.INVALID_CONFIGURATION.code()).isEqualTo(24);
    assertThat(ErrorCode.UNSUPPORTED_OPERATION.code()).isEqualTo(25);
    assertThat(ErrorCode.values()).hasSize(12);
  }

  @Test
  void everyConstantRoundTripsThroughFromCode() {
    for (ErrorCode value : ErrorCode.values()) {
      assertThat(ErrorCode.fromCode(value.code())).isEqualTo(value);
    }
  }

  @Test
  void mapsKnownCodes() {
    assertThat(ErrorCode.fromCode(22)).isEqualTo(ErrorCode.NULL_POINTER);
    assertThat(ErrorCode.fromCode(-1)).isEqualTo(ErrorCode.PANIC);
    assertThat(ErrorCode.fromCode(-1000)).isEqualTo(ErrorCode.INVALID_HANDLE);
    assertThat(ErrorCode.fromCode(25)).isEqualTo(ErrorCode.UNSUPPORTED_OPERATION);
  }

  @Test
  void fallsBackToUnknownForAnUnrecognizedCode() {
    assertThat(ErrorCode.fromCode(999)).isEqualTo(ErrorCode.UNKNOWN_ERROR);
  }

  @Test
  void codeRoundTrips() {
    assertThat(ErrorCode.NULL_POINTER.code()).isEqualTo(22);
  }
}
