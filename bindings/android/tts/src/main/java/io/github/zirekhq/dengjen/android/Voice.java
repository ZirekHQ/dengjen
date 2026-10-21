package io.github.zirekhq.dengjen.android;

import android.system.ErrnoException;
import android.system.Os;
public final class Voice implements AutoCloseable {
  private static final int OPERATION_ERROR = 19;
  private static boolean initialized;

  private final HandleGuard guard;

  private Voice(long handle) {
    this.guard = new HandleGuard(handle);
  }

  /**
   * Points the engine at the directory that contains {@code espeak-ng-data}. Must be called before
   * the first load or synthesis in the process; later calls have no effect.
   */
  public static void setEspeakDataDirectory(String parentOfEspeakNgData) {
    try {
      Os.setenv("DENGJEN_ESPEAKNG_DATA_DIRECTORY", parentOfEspeakNgData, true);
    } catch (ErrnoException e) {
      throw new DengjenException(OPERATION_ERROR, e.getMessage());
    }
  }

  public static Voice load(String configPath) {
    ensureInitialized();
    return new Voice(nativeLoad(configPath));
  }

  private static synchronized void ensureInitialized() {
    if (!initialized) {
      NativeLoader.loadLibrary("dengjen_tts_jni");
      nativeInit();
      initialized = true;
    }
  }

  public AudioInfo audioInfo() {
    int[] v = guard.use(Voice::nativeAudioInfo);
    return new AudioInfo(v[0], v[1], v[2]);
  }

  public void speak(String text, SynthesisParams p, SynthesisEventHandler handler) {
    guard.use(
        h -> {
          nativeSpeak(
              h,
              text,
              p.mode().value(),
              p.rate(),
              p.volume(),
              p.pitch(),
              p.appendedSilenceMs(),
              handler);
          return null;
        });
  }

  public boolean speakToFile(String text, SynthesisParams p, String outPath) {
    return guard.use(
        h ->
            nativeSpeakToFile(
                h,
                text,
                p.mode().value(),
                p.rate(),
                p.volume(),
                p.pitch(),
                p.appendedSilenceMs(),
                outPath));
  }

  /** Stops a realtime synthesis; has no effect in other modes. */
  public void cancel() {
    guard.cancel(Voice::nativeCancel);
  }

  /** Must not be called from inside a {@link SynthesisEventHandler}. */
  @Override
  public void close() {
    guard.close(Voice::nativeUnload);
  }

  static boolean dispatch(
      SynthesisEventHandler h, int type, byte[] data, int code, String message) {
    try {
      EventType t = EventType.fromValue(type);
      DengjenException error = t == EventType.ERROR ? new DengjenException(code, message) : null;
      return h.onEvent(new SynthesisEvent(t, data, error));
    } catch (Throwable t) {
      return false;
    }
  }

  private static native void nativeInit();

  private static native long nativeLoad(String configPath);

  private static native void nativeUnload(long handle);

  private static native int[] nativeAudioInfo(long handle);

  private static native void nativeSpeak(
      long handle,
      String text,
      int mode,
      int rate,
      int volume,
      int pitch,
      int silenceMs,
      SynthesisEventHandler handler);

  private static native boolean nativeSpeakToFile(
      long handle,
      String text,
      int mode,
      int rate,
      int volume,
      int pitch,
      int silenceMs,
      String outPath);

  private static native void nativeCancel(long handle);
}
