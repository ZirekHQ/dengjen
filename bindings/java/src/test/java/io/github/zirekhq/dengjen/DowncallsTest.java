package io.github.zirekhq.dengjen;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

import java.lang.invoke.MethodHandle;
import java.lang.invoke.MethodHandles;
import java.lang.invoke.MethodType;
import java.util.concurrent.atomic.AtomicBoolean;
import org.junit.jupiter.api.Test;

class DowncallsTest {

  @Test
  void runsTheDowncallWithItsArguments() throws ReflectiveOperationException {
    AtomicBoolean ran = new AtomicBoolean();
    MethodHandle set =
        MethodHandles.lookup()
            .findVirtual(
                AtomicBoolean.class, "set", MethodType.methodType(void.class, boolean.class))
            .bindTo(ran);
    Downcalls.run("libdengjenFoo", set, true);
    assertThat(ran).isTrue();
  }

  @Test
  void wrapsAFailureWithTheFunctionNameAndKeepsTheCause() {
    Exception cause = new Exception("boom");
    MethodHandle fails = MethodHandles.throwException(void.class, Exception.class);
    assertThatThrownBy(() -> Downcalls.run("libdengjenFoo", fails, cause))
        .isInstanceOf(IllegalStateException.class)
        .hasMessage("libdengjenFoo downcall failed")
        .hasCause(cause);
  }
}
