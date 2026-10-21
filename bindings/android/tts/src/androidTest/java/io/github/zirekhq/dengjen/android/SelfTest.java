package io.github.zirekhq.dengjen.android;

final class SelfTest {
  private SelfTest() {}

  static {
    NativeLoader.loadLibrary("dengjen_tts_jni");
  }

  static native boolean deliverFromNativeThread(SynthesisEventHandler handler);

  static native int deliverMany(SynthesisEventHandler handler, int n);

  static native void panic();
}
