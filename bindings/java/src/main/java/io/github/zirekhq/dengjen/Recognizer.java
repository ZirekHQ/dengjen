package io.github.zirekhq.dengjen;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;
import java.util.concurrent.locks.ReentrantLock;

/** Offline speech recognizer backed by libdengjen-stt. */
public final class Recognizer implements AutoCloseable {
  private static final long NUM_THREADS_OFFSET = 4;

  private final ReentrantLock lock = new ReentrantLock();
  private MemorySegment ptr;

  private Recognizer(MemorySegment ptr) {
    this.ptr = ptr;
  }

  static Recognizer closedForTest() {
    return new Recognizer(MemorySegment.NULL);
  }

  /** Loads the model in {@code modelDir} after checking it hashes to {@code sha256}. */
  public static Recognizer load(String modelDir, String sha256, int numThreads) {
    try (Arena arena = Arena.ofConfined()) {
      MemorySegment config = arena.allocate(SttLib.STT_CONFIG);
      config.set(ValueLayout.JAVA_INT, 0, (int) SttLib.STT_CONFIG.byteSize());
      config.set(ValueLayout.JAVA_INT, NUM_THREADS_OFFSET, numThreads);
      MemorySegment outError = arena.allocate(DengjenLayouts.EXTERN_ERROR);
      MemorySegment ptr;
      try {
        ptr =
            (MemorySegment)
                SttLib.LOAD.invokeExact(
                    arena.allocateFrom(modelDir), config, arena.allocateFrom(sha256), outError);
      } catch (Throwable t) {
        throw new IllegalStateException("libdengjenSttLoad downcall failed", t);
      }
      checkAndThrow(outError);
      return new Recognizer(ptr);
    }
  }

  /**
   * Transcribes mono samples in [-1, 1] recorded at {@code sampleRate}. A concurrent {@link
   * #close()} waits for the decode to finish.
   */
  public String recognize(float[] samples, int sampleRate) {
    lock.lock();
    try {
      if (ptr == null) {
        throw new SttException(SttErrorCode.INVALID_HANDLE, "recognizer is closed");
      }
      return recognizeLocked(ptr, samples, sampleRate);
    } finally {
      lock.unlock();
    }
  }

  private static String recognizeLocked(
      MemorySegment recognizerPtr, float[] samples, int sampleRate) {
    try (Arena arena = Arena.ofConfined()) {
      MemorySegment buffer = arena.allocateFrom(ValueLayout.JAVA_FLOAT, samples);
      MemorySegment outError = arena.allocate(DengjenLayouts.EXTERN_ERROR);
      MemorySegment text;
      try {
        text =
            (MemorySegment)
                SttLib.RECOGNIZE.invokeExact(
                    recognizerPtr, buffer, (long) samples.length, sampleRate, outError);
      } catch (Throwable t) {
        throw new IllegalStateException("libdengjenSttRecognize downcall failed", t);
      }
      checkAndThrow(outError);
      return readAndFree(text);
    }
  }

  @Override
  public void close() {
    lock.lock();
    try {
      if (ptr == null) {
        return;
      }
      MemorySegment closing = ptr;
      ptr = null;
      if (closing.equals(MemorySegment.NULL)) {
        return;
      }
      try {
        SttLib.UNLOAD.invokeExact(closing);
      } catch (Throwable t) {
        throw new IllegalStateException("libdengjenSttUnload downcall failed", t);
      }
    } finally {
      lock.unlock();
    }
  }

  private static void checkAndThrow(MemorySegment externError) {
    int code = externError.get(ValueLayout.JAVA_INT, DengjenLayouts.EXTERN_ERROR_CODE_OFFSET);
    if (code == 0) {
      return;
    }
    MemorySegment messagePtr =
        externError.get(ValueLayout.ADDRESS, DengjenLayouts.EXTERN_ERROR_MESSAGE_OFFSET);
    throw new SttException(SttErrorCode.fromCode(code), readAndFree(messagePtr));
  }

  private static String readAndFree(MemorySegment stringPtr) {
    if (stringPtr.equals(MemorySegment.NULL)) {
      return null;
    }
    String value = stringPtr.reinterpret(Long.MAX_VALUE).getString(0);
    try {
      SttLib.FREE_STRING.invokeExact(stringPtr);
    } catch (Throwable t) {
      throw new IllegalStateException("libdengjenSttFreeString downcall failed", t);
    }
    return value;
  }
}
