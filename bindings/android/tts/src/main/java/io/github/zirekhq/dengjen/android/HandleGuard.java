package io.github.zirekhq.dengjen.android;

import java.util.concurrent.locks.ReentrantLock;
import java.util.concurrent.locks.ReentrantReadWriteLock;
import java.util.function.LongConsumer;
import java.util.function.LongFunction;

final class HandleGuard {
  private static final int INVALID_HANDLE = -1000;
  private static final int OPERATION_ERROR = 19;

  private final ReentrantReadWriteLock lock = new ReentrantReadWriteLock();
  private final ReentrantLock cancelLock = new ReentrantLock();
  private volatile long handle;

  HandleGuard(long handle) {
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

  /** Skips the read-write lock so a realtime synthesis can be cancelled while close() waits for it. */
  void cancel(LongConsumer op) {
    cancelLock.lock();
    try {
      op.accept(open());
    } finally {
      cancelLock.unlock();
    }
  }

  void close(LongConsumer unload) {
    if (lock.getReadHoldCount() > 0) {
      throw new DengjenException(OPERATION_ERROR, "close() called from inside a Voice operation");
    }
    lock.writeLock().lock();
    try {
      cancelLock.lock();
      try {
        long closing = handle;
        handle = 0;
        if (closing != 0) {
          unload.accept(closing);
        }
      } finally {
        cancelLock.unlock();
      }
    } finally {
      lock.writeLock().unlock();
    }
  }

  private long open() {
    if (handle == 0) {
      throw new DengjenException(INVALID_HANDLE, "voice is closed");
    }
    return handle;
  }
}
