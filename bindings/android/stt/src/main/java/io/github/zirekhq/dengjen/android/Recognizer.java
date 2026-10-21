package io.github.zirekhq.dengjen.android;

public final class Recognizer implements AutoCloseable {
  private static final int INVALID_CONFIGURATION = 24;

  private final RecognizerGuard guard;

  private Recognizer(long handle) {
    this.guard = new RecognizerGuard(handle);
  }

  /**
   * Loads the model in {@code modelDir} after checking the model file against {@code
   * expectedSha256}. Blocks while the model loads, so call it from a worker thread.
   */
  public static Recognizer load(String modelDir, String expectedSha256, int threads) {
    if (threads < 1) {
      throw new DengjenException(INVALID_CONFIGURATION, "threads must be at least 1: " + threads);
    }
    NativeLoader.loadLibrary("dengjen_stt_jni");
    return new Recognizer(nativeLoad(modelDir, expectedSha256, threads));
  }

  /**
   * Transcribes mono samples recorded at {@code sampleRate} Hz. Blocks until decoding ends and
   * cannot be cancelled, so call it from a worker thread.
   */
  public String recognize(float[] samples, int sampleRate) {
    return guard.use(handle -> nativeRecognize(handle, samples, sampleRate));
  }

  @Override
  public void close() {
    guard.close(Recognizer::nativeUnload);
  }

  private static native long nativeLoad(String modelDir, String expectedSha256, int threads);

  private static native String nativeRecognize(long handle, float[] samples, int sampleRate);

  private static native void nativeUnload(long handle);
}
