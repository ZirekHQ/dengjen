package io.github.zirekhq.dengjen.android;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertThrows;

import org.junit.Test;

public class RecognizerTest {
  @Test
  public void zeroThreadsIsRejectedBeforeAnyNativeCall() {
    DengjenException e =
        assertThrows(DengjenException.class, () -> Recognizer.load("/nowhere", "00", 0));
    assertEquals(24, e.code());
  }

  @Test
  public void closingTwiceIsHarmless() {
    RecognizerGuard guard = new RecognizerGuard(7);
    guard.close(h -> {});
    guard.close(h -> {});
  }

  @Test
  public void useAfterCloseIsAnInvalidHandle() {
    RecognizerGuard guard = new RecognizerGuard(7);
    guard.close(h -> {});
    DengjenException e = assertThrows(DengjenException.class, () -> guard.use(h -> "x"));
    assertEquals(-1000, e.code());
  }
}
