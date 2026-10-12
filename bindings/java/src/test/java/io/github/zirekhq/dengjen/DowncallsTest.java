package io.github.zirekhq.dengjen;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

import java.util.concurrent.atomic.AtomicBoolean;
import org.junit.jupiter.api.Test;

class DowncallsTest {

  @Test
  void runsTheCall() {
    AtomicBoolean ran = new AtomicBoolean();
    Downcalls.run("libdengjenFoo", () -> ran.set(true));
    assertThat(ran).isTrue();
  }

  @Test
  void wrapsAFailureWithTheFunctionNameAndKeepsTheCause() {
    Throwable cause = new Exception("boom");
    assertThatThrownBy(
            () ->
                Downcalls.run(
                    "libdengjenFoo",
                    () -> {
                      throw cause;
                    }))
        .isInstanceOf(IllegalStateException.class)
        .hasMessage("libdengjenFoo downcall failed")
        .hasCause(cause);
  }
}
