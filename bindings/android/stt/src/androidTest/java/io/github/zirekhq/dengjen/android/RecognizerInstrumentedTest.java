package io.github.zirekhq.dengjen.android;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertThrows;
import static org.junit.Assert.assertTrue;

import androidx.test.platform.app.InstrumentationRegistry;
import java.io.File;
import java.io.FileInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.Locale;
import java.util.Random;
import org.junit.Test;
import org.junit.runner.RunWith;
import androidx.test.ext.junit.runners.AndroidJUnit4;

@RunWith(AndroidJUnit4.class)
public class RecognizerInstrumentedTest {
  private static final File MODEL_DIR = new File("/data/local/tmp/dengjen-stt");
  private static final String CLIP_FILE = "/data/local/tmp/dengjen-stt/stt-clip.f32";
  private static final int CLIP_SAMPLE_RATE = 24_000;
  private static final String[] CLIP_WORDS = {"şêr", "komên mezin", "dijîn"};
  private static final int SAMPLE_RATE = 16_000;
  private static final int MODEL_LOAD_FAILED = 43;
  private static final int INVALID_AUDIO = 44;
  private static final int INVALID_HANDLE = -1000;

  private static String sha256(File file) throws IOException, NoSuchAlgorithmException {
    MessageDigest digest = MessageDigest.getInstance("SHA-256");
    try (InputStream in = new FileInputStream(file)) {
      byte[] chunk = new byte[1 << 16];
      for (int n = in.read(chunk); n >= 0; n = in.read(chunk)) {
        digest.update(chunk, 0, n);
      }
    }
    byte[] hash = digest.digest();
    StringBuilder hex = new StringBuilder();
    for (byte b : hash) {
      hex.append(String.format("%02x", b));
    }
    return hex.toString();
  }

  private static File garbageModelDir() throws IOException {
    File dir =
        new File(InstrumentationRegistry.getInstrumentation().getTargetContext().getCacheDir(), "garbage-model");
    assertTrue(dir.isDirectory() || dir.mkdirs());
    byte[] noise = new byte[4096];
    new Random(7).nextBytes(noise);
    Files.write(new File(dir, "model.int8.onnx").toPath(), noise);
    Files.write(new File(dir, "tokens.txt").toPath(), "<blk> 0\n".getBytes(StandardCharsets.UTF_8));
    return dir;
  }

  @Test
  public void garbageModelIsATypedFailureAndTheProcessSurvives() throws Exception {
    File dir = garbageModelDir();
    String checksum = sha256(new File(dir, "model.int8.onnx"));
    DengjenException e =
        assertThrows(DengjenException.class, () -> Recognizer.load(dir.getPath(), checksum, 1));
    assertEquals(MODEL_LOAD_FAILED, e.code());
  }

  @Test
  public void missingModelDirectoryIsATypedFailure() {
    assertThrows(
        DengjenException.class, () -> Recognizer.load("/data/local/tmp/nowhere", "00", 1));
  }

  @Test
  public void shortAudioIsATypedFailure() throws Exception {
    assertTrue("push the model to " + MODEL_DIR, new File(MODEL_DIR, "model.int8.onnx").exists());
    File model = new File(MODEL_DIR, "model.int8.onnx");
    try (Recognizer recognizer = Recognizer.load(MODEL_DIR.getPath(), sha256(model), 2)) {
      DengjenException e =
          assertThrows(DengjenException.class, () -> recognizer.recognize(new float[399], SAMPLE_RATE));
      assertEquals(INVALID_AUDIO, e.code());
    }
  }

  private static float[] readClip() throws IOException {
    byte[] bytes = Files.readAllBytes(new File(CLIP_FILE).toPath());
    float[] samples = new float[bytes.length / Float.BYTES];
    ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN).asFloatBuffer().get(samples);
    return samples;
  }

  @Test
  public void realModelTranscribesAKurmanjiClip() throws Exception {
    File model = new File(MODEL_DIR, "model.int8.onnx");
    assertTrue("push the model to " + MODEL_DIR, model.exists());
    assertTrue("push the clip to " + CLIP_FILE, new File(CLIP_FILE).exists());
    try (Recognizer recognizer = Recognizer.load(MODEL_DIR.getPath(), sha256(model), 2)) {
      String transcript = recognizer.recognize(readClip(), CLIP_SAMPLE_RATE).toLowerCase(Locale.ROOT);
      for (String word : CLIP_WORDS) {
        assertTrue(word + " missing from " + transcript, transcript.contains(word));
      }
    }
  }

  @Test
  public void realModelDecodesAndClosedRecognizerIsRejected() throws Exception {
    assertTrue("push the model to " + MODEL_DIR, new File(MODEL_DIR, "model.int8.onnx").exists());
    File model = new File(MODEL_DIR, "model.int8.onnx");
    Recognizer recognizer = Recognizer.load(MODEL_DIR.getPath(), sha256(model), 2);
    float[] quietNoise = new float[SAMPLE_RATE];
    Random random = new Random(3);
    for (int i = 0; i < quietNoise.length; i++) {
      quietNoise[i] = (random.nextFloat() - 0.5f) * 0.01f;
    }
    assertNotNull(recognizer.recognize(quietNoise, SAMPLE_RATE));
    recognizer.close();
    DengjenException e =
        assertThrows(DengjenException.class, () -> recognizer.recognize(quietNoise, SAMPLE_RATE));
    assertEquals(INVALID_HANDLE, e.code());
  }
}
