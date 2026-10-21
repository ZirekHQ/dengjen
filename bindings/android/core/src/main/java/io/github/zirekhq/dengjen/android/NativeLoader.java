package io.github.zirekhq.dengjen.android;

public final class NativeLoader {
  private static boolean onnxLoaded;

  private NativeLoader() {}

  /** Loads ONNX Runtime before {@code name} so the dynamic loader resolves it by soname. */
  public static synchronized void loadLibrary(String name) {
    if (!onnxLoaded) {
      System.loadLibrary("onnxruntime");
      onnxLoaded = true;
    }
    System.loadLibrary(name);
  }
}
