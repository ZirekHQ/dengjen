package io.github.zirekhq.dengjen.android;

import java.util.concurrent.locks.ReentrantReadWriteLock;
import java.util.function.LongConsumer;
import java.util.function.LongFunction;

final class RecognizerGuard {
  private static final int INVALID_HANDLE = -1000;
  private static final int OPERATION_ERROR = 19;

  private final ReentrantReadWriteLock lock = new ReentrantReadWriteLock();
  private volatile long handle;

  RecognizerGuard(long handle) {
    this.handle = handle;
  }

  <T> T use(LongFunction<T> op) {
    lock.readLock().lock();
    try {
      return op.apply(open());
    } finally {
      lock.readLock().unlock();
    }
  }

  void close(LongConsumer unload) {
    if (lock.getReadHoldCount() > 0) {
      throw new DengjenException(
          OPERATION_ERROR, "close() called from inside a Recognizer operation");
    }
    lock.writeLock().lock();
    try {
      long closing = handle;
      handle = 0;
      if (closing != 0) {
        unload.accept(closing);
      }
    } finally {
      lock.writeLock().unlock();
    }
  }

  private long open() {
    if (handle == 0) {
      throw new DengjenException(INVALID_HANDLE, "recognizer is closed");
    }
    return handle;
  }
}
