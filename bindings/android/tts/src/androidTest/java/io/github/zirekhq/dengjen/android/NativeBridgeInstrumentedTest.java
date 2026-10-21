package io.github.zirekhq.dengjen.android;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertThrows;
import static org.junit.Assert.assertTrue;

import androidx.test.ext.junit.runners.AndroidJUnit4;
import java.io.File;
import java.util.concurrent.atomic.AtomicInteger;
import org.junit.BeforeClass;
import org.junit.Test;
import org.junit.runner.RunWith;

@RunWith(AndroidJUnit4.class)
public class NativeBridgeInstrumentedTest {
  private static final File VOICE = new File("/data/local/tmp/dengjen-voice/model.onnx.json");
  private static final String ESPEAK_DATA = "/data/local/tmp/dengjen-voice";
  private static final SynthesisParams LAZY =
      new SynthesisParams(SynthesisMode.LAZY, 10, 100, 50, 0);

  @BeforeClass
  public static void configureEspeak() {
    Voice.setEspeakDataDirectory(ESPEAK_DATA);
  }

  @Test
  public void callbackFromNativeThreadReachesJava() {
    AtomicInteger type = new AtomicInteger(-1);
    boolean kept =
        SelfTest.deliverFromNativeThread(
            e -> {
              type.set(e.type().value());
              return true;
            });
    assertTrue(kept);
    assertEquals(EventType.SPEECH.value(), type.get());
  }

  @Test
  public void handlerThatThrowsStopsDelivery() {
    boolean kept =
        SelfTest.deliverFromNativeThread(
            e -> {
              throw new IllegalStateException("handler failure");
            });
    assertFalse(kept);
  }

  @Test
  public void localRefsDoNotOverflowOnJavaThread() {
    assertEquals(10_000, SelfTest.deliverMany(e -> true, 10_000));
  }

  @Test
  public void panicBecomesException() {
    DengjenException e = assertThrows(DengjenException.class, SelfTest::panic);
    assertEquals(-1, e.code());
  }

  @Test
  public void missingVoiceIsATypedFailure() {
    assertThrows(DengjenException.class, () -> Voice.load("/nonexistent/config.json"));
  }

  @Test
  public void nulInTextIsATypedFailureAndClosedVoiceIsRejected() {
    assertTrue("push a voice to " + VOICE + " (see README)", VOICE.exists());
    Voice voice = Voice.load(VOICE.getPath());
    assertThrows(DengjenException.class, () -> voice.speak("a\0b", LAZY, e -> true));
    voice.close();
    DengjenException e = assertThrows(DengjenException.class, voice::audioInfo);
    assertEquals(-1000, e.code());
  }

  @Test
  public void synthesizesWithRealVoice() {
    assertTrue("push a voice to " + VOICE + " (see README)", VOICE.exists());
    try (Voice voice = Voice.load(VOICE.getPath())) {
      assertTrue(voice.audioInfo().sampleRate() > 0);
      AtomicInteger bytes = new AtomicInteger();
      AtomicInteger finished = new AtomicInteger();
      voice.speak(
          "Hello.",
          LAZY,
          e -> {
            if (e.type() == EventType.SPEECH) bytes.addAndGet(e.data().length);
            if (e.type() == EventType.FINISHED) finished.incrementAndGet();
            return true;
          });
      assertTrue(bytes.get() > 0);
      assertEquals(1, finished.get());
    }
  }
}
