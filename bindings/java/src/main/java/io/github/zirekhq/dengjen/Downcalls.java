package io.github.zirekhq.dengjen;

import java.lang.invoke.MethodHandle;

final class Downcalls {
  private Downcalls() {}

  static void run(String function, MethodHandle downcall, Object... args) {
    try {
      downcall.invokeWithArguments(args);
    } catch (Throwable t) {
      throw new IllegalStateException(function + " downcall failed", t);
    }
  }
}
