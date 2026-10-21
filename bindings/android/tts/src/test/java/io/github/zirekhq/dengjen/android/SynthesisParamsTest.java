package io.github.zirekhq.dengjen.android;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertThrows;

import org.junit.Test;

public class SynthesisParamsTest {
  @Test
  public void rateAbove255IsRejected() {
    DengjenException e =
        assertThrows(
            DengjenException.class, () -> new SynthesisParams(SynthesisMode.LAZY, 256, 100, 50, 0));
    assertEquals(24, e.code());
  }

  @Test
  public void negativePitchIsRejected() {
    assertThrows(
        DengjenException.class, () -> new SynthesisParams(SynthesisMode.LAZY, 10, 100, -1, 0));
  }

  @Test
  public void negativeSilenceIsRejected() {
    assertThrows(
        DengjenException.class, () -> new SynthesisParams(SynthesisMode.LAZY, 10, 100, 50, -1));
  }

  @Test
  public void boundaryValuesAreAccepted() {
    new SynthesisParams(SynthesisMode.LAZY, 0, 255, 0, 0);
  }
}
