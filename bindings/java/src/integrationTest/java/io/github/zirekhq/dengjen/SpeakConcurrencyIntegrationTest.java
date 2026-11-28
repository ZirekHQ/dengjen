package io.github.zirekhq.dengjen;

import static org.assertj.core.api.Assertions.assertThat;
import static org.awaitility.Awaitility.await;

import java.nio.file.Path;
import java.time.Duration;
import java.util.concurrent.atomic.AtomicInteger;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class SpeakConcurrencyIntegrationTest {
  private static final int THREADS = 6;
  private static final int CALLS_PER_THREAD = 100;

  @Test
  void concurrentBlockingAndNonblockingStreamsReleaseEveryCallExactlyOnce(@TempDir Path tempDir)
      throws Exception {
    try (Voice voice = Voice.load(TestFixtures.syntheticPiperConfigPath(tempDir).toString())) {
      SynthesisParams blocking = new SynthesisParams(SynthesisMode.LAZY, 10, 100, 50, 0);
      SynthesisParams nonblocking = new SynthesisParams(SynthesisMode.LAZY, 10, 100, 50, 0, true);
      int expected = THREADS * CALLS_PER_THREAD;

      AtomicInteger releases = new AtomicInteger();
      SpeakTrampoline.testCallReleased.set(releases::incrementAndGet);
      try {
        Thread[] threads = new Thread[THREADS];
        for (int t = 0; t < THREADS; t++) {
          SynthesisParams params = t % 2 == 0 ? nonblocking : blocking;
          threads[t] =
              new Thread(() -> {
                for (int i = 0; i < CALLS_PER_THREAD; i++) {

                  voice.speak("Test.", params, event -> event.data().length % 3 != 0);
                }
              });
          threads[t].start();
        }
        for (Thread thread : threads) {
          thread.join();
        }

        await()
            .atMost(Duration.ofSeconds(60))
            .pollInterval(Duration.ofMillis(20))
            .until(() -> releases.get() >= expected);
        assertThat(releases.get())
            .as("every speak() call must release its registration exactly once")
            .isEqualTo(expected);
      } finally {

        await().pollDelay(Duration.ofMillis(500)).until(() -> true);
        SpeakTrampoline.testCallReleased.set(null);
      }
    }
  }
}
