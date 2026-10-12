package io.github.zirekhq.dengjen;

final class Downcalls {
  private Downcalls() {}

  @FunctionalInterface
  interface Call {
    void run() throws Throwable;
  }

  static void run(String function, Call call) {
    try {
      call.run();
    } catch (Throwable t) {
      throw new IllegalStateException(function + " downcall failed", t);
    }
  }
}
