package io.github.zirekhq.dengjen;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

import java.io.IOException;
import java.net.URL;
import java.net.URLClassLoader;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class SttNativeExtractionTest {
  @Test
  void extractsEveryManifestEntryUnderItsOwnName(@TempDir Path classpath) throws IOException {
    Path natives = Files.createDirectories(classpath.resolve("natives/linux-x86_64"));
    Files.writeString(natives.resolve("MANIFEST"), "libonnxruntime.so.1.28.2\nlibdengjen_stt.so\n");
    Files.write(
        natives.resolve("libonnxruntime.so.1.28.2"), "ort".getBytes(StandardCharsets.UTF_8));
    Files.write(natives.resolve("libdengjen_stt.so"), "stt".getBytes(StandardCharsets.UTF_8));

    try (URLClassLoader loader = new URLClassLoader(new URL[] {classpath.toUri().toURL()}, null)) {
      Path directory = SttNativeLoader.extractManifest(loader, "linux-x86_64");
      assertThat(directory.resolve("libonnxruntime.so.1.28.2")).hasContent("ort");
      assertThat(directory.resolve("libdengjen_stt.so")).hasContent("stt");
    }
  }

  @Test
  void aMissingManifestNamesTheDependencyToAdd(@TempDir Path classpath) throws IOException {
    try (URLClassLoader loader = new URLClassLoader(new URL[] {classpath.toUri().toURL()}, null)) {
      assertThatThrownBy(() -> SttNativeLoader.extractManifest(loader, "linux-x86_64"))
          .isInstanceOf(IllegalStateException.class)
          .hasMessageContaining("dengjen-stt-natives");
    }
  }

  @Test
  void sameFileNamesWithDifferentBytesExtractToDifferentDirectories(@TempDir Path root)
      throws IOException {
    Path first = extractWith(root.resolve("v1"), "one");
    Path second = extractWith(root.resolve("v2"), "two");

    assertThat(first).isNotEqualTo(second);
    assertThat(first.resolve("libdengjen_stt.so")).hasContent("one");
    assertThat(second.resolve("libdengjen_stt.so")).hasContent("two");
  }

  @Test
  void aDirectoryWithoutTheSttLibraryFailsWithAClearMessage(@TempDir Path empty) {
    String previous = System.getProperty(SttNativeLoader.DIR_PROPERTY);
    System.setProperty(SttNativeLoader.DIR_PROPERTY, empty.toString());
    try {
      assertThatThrownBy(SttNativeLoader::load)
          .isInstanceOf(IllegalStateException.class)
          .hasMessageContaining("dengjen_stt");
    } finally {
      if (previous == null) {
        System.clearProperty(SttNativeLoader.DIR_PROPERTY);
      } else {
        System.setProperty(SttNativeLoader.DIR_PROPERTY, previous);
      }
    }
  }

  private static Path extractWith(Path classpath, String content) throws IOException {
    Path natives = Files.createDirectories(classpath.resolve("natives/linux-x86_64"));
    Files.writeString(natives.resolve("MANIFEST"), "libdengjen_stt.so\n");
    Files.writeString(natives.resolve("libdengjen_stt.so"), content);
    try (URLClassLoader loader = new URLClassLoader(new URL[] {classpath.toUri().toURL()}, null)) {
      return SttNativeLoader.extractManifest(loader, "linux-x86_64");
    }
  }
}
