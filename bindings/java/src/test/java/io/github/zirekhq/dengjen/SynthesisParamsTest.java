package io.github.zirekhq.dengjen;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

import org.junit.jupiter.api.Test;

class SynthesisParamsTest {
  @Test
  void rejectsAnOutOfRangeRate() {
    assertThatThrownBy(() -> new SynthesisParams(SynthesisMode.LAZY, 101, 0, 0, 0))
        .isInstanceOf(IllegalArgumentException.class);
  }

  @Test
  void rejectsOutOfRangeRateBelowZero() {
    assertThatThrownBy(() -> new SynthesisParams(SynthesisMode.LAZY, -1, 0, 0, 0))
        .isInstanceOf(IllegalArgumentException.class);
  }

  @Test
  void rejectsOutOfRangeVolume() {
    assertThatThrownBy(() -> new SynthesisParams(SynthesisMode.LAZY, 0, -1, 0, 0))
        .isInstanceOf(IllegalArgumentException.class);
    assertThatThrownBy(() -> new SynthesisParams(SynthesisMode.LAZY, 0, 101, 0, 0))
        .isInstanceOf(IllegalArgumentException.class);
  }

  @Test
  void rejectsOutOfRangePitch() {
    assertThatThrownBy(() -> new SynthesisParams(SynthesisMode.LAZY, 0, 0, -1, 0))
        .isInstanceOf(IllegalArgumentException.class);
    assertThatThrownBy(() -> new SynthesisParams(SynthesisMode.LAZY, 0, 0, 101, 0))
        .isInstanceOf(IllegalArgumentException.class);
  }

  @Test
  void acceptsBoundaryRateValues() {
    assertThat(new SynthesisParams(SynthesisMode.LAZY, 0, 0, 0, 0).rate()).isEqualTo(0);
    assertThat(new SynthesisParams(SynthesisMode.LAZY, 100, 0, 0, 0).rate()).isEqualTo(100);
  }

  @Test
  void acceptsBoundaryVolumeValues() {
    assertThat(new SynthesisParams(SynthesisMode.LAZY, 0, 0, 0, 0).volume()).isEqualTo(0);
    assertThat(new SynthesisParams(SynthesisMode.LAZY, 0, 100, 0, 0).volume()).isEqualTo(100);
  }

  @Test
  void acceptsBoundaryPitchValues() {
    assertThat(new SynthesisParams(SynthesisMode.LAZY, 0, 0, 0, 0).pitch()).isEqualTo(0);
    assertThat(new SynthesisParams(SynthesisMode.LAZY, 0, 0, 100, 0).pitch()).isEqualTo(100);
  }

  @Test
  void rejectsNegativeAppendedSilence() {
    assertThatThrownBy(() -> new SynthesisParams(SynthesisMode.LAZY, 0, 0, 0, -1))
        .isInstanceOf(IllegalArgumentException.class);
  }

  @Test
  void rejectsNullMode() {
    assertThatThrownBy(() -> new SynthesisParams(null, 0, 0, 0, 0))
        .isInstanceOf(NullPointerException.class);
  }

  @Test
  void batchedModeCarriesHeaderValue() {
    assertThat(SynthesisMode.BATCHED.value()).isEqualTo(3);
  }

  @Test
  void defaultsNonblockingToFalse() {
    var params = new SynthesisParams(SynthesisMode.LAZY, 10, 100, 50, 0);
    assertThat(params.nonblocking()).isFalse();
  }
}
