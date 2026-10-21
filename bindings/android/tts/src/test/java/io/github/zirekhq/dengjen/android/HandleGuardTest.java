package io.github.zirekhq.dengjen.android;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertThrows;

import java.util.concurrent.CountDownLatch;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicInteger;
import org.junit.After;
import org.junit.Test;

public class HandleGuardTest {
  private final ExecutorService pool =
      Executors.newCachedThreadPool(
          r -> {
            Thread t = new Thread(r);
            t.setDaemon(true);
            return t;
          });

  @After
  public void tearDown() {
    pool.shutdownNow();
  }

  @Test(timeout = 20_000)
  public void cancelStillRunsWhileCloseIsWaiting() throws Exception {
    HandleGuard guard = new HandleGuard(7);
    CountDownLatch inUse = new CountDownLatch(1);
    CountDownLatch cancelled = new CountDownLatch(1);
    Future<?> speak =
        pool.submit(
            () ->
                guard.use(
                    h -> {
                      inUse.countDown();
                      await(cancelled);
                      return null;
                    }));
    inUse.await();
    Future<?> close = pool.submit(() -> guard.close(h -> {}));
    Thread.sleep(300);
    Future<?> cancel = pool.submit(() -> guard.cancel(h -> cancelled.countDown()));
    cancel.get(3, TimeUnit.SECONDS);
    speak.get(3, TimeUnit.SECONDS);
    close.get(3, TimeUnit.SECONDS);
  }

  @Test(timeout = 20_000)
  public void closeFromInsideAnOperationIsRejected() throws Exception {
    HandleGuard guard = new HandleGuard(7);
    Future<DengjenException> result =
        pool.submit(
            () ->
                assertThrows(
                    DengjenException.class,
                    () ->
                        guard.use(
                            h -> {
                              guard.close(x -> {});
                              return null;
                            })));
    assertEquals(19, result.get(3, TimeUnit.SECONDS).code());
  }

  @Test
  public void useAfterCloseIsRejectedAndUnloadRunsOnce() {
    HandleGuard guard = new HandleGuard(7);
    AtomicInteger unloads = new AtomicInteger();
    guard.close(h -> unloads.incrementAndGet());
    guard.close(h -> unloads.incrementAndGet());
    DengjenException e = assertThrows(DengjenException.class, () -> guard.use(h -> null));
    assertEquals(-1000, e.code());
    assertEquals(1, unloads.get());
  }

  private static void await(CountDownLatch latch) {
    try {
      latch.await();
    } catch (InterruptedException e) {
      Thread.currentThread().interrupt();
    }
  }
}
