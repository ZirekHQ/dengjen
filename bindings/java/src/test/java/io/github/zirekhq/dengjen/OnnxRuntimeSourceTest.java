package io.github.zirekhq.dengjen;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

import java.io.IOException;
import java.net.URI;
import java.net.URL;
import java.net.URLClassLoader;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Map;
import java.util.Optional;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class OnnxRuntimeSourceTest {
  private static final String PREFIX = "dengjen.onnxruntime";
  private static final String SHA = "ab".repeat(32);
  private static final ClassLoader EMPTY = new URLClassLoader(new URL[0], null);

  private static Optional<OnnxRuntimeSource> resolve(Map<String, String> properties) {
    return OnnxRuntimeSource.resolve(PREFIX, properties::get, EMPTY, "linux-x86_64");
  }

  @Test
  void aPathIsUsedAsIs() {
    assertThat(resolve(Map.of(PREFIX + ".path", "/opt/ort/libonnxruntime.so")))
        .contains(new OnnxRuntimeSource.Local(Path.of("/opt/ort/libonnxruntime.so")));
  }

  @Test
  void aUrlWithItsDigestIsDownloadedUnderItsOwnFileName() {
    assertThat(
            resolve(
                Map.of(
                    PREFIX + ".url",
                    "https://example.org/ort/libonnxruntime.so.1.23.2",
                    PREFIX + ".sha256",
                    SHA.toUpperCase())))
        .contains(
            new OnnxRuntimeSource.Download(
                URI.create("https://example.org/ort/libonnxruntime.so.1.23.2"),
                SHA,
                "libonnxruntime.so.1.23.2"));
  }

  @Test
  void aFileNameCanBeGivenForUrlsThatHideIt() {
    assertThat(
            resolve(
                Map.of(
                    PREFIX + ".url", "https://example.org/download?id=7",
                    PREFIX + ".sha256", SHA,
                    PREFIX + ".file", "onnxruntime.dll")))
        .get()
        .extracting(source -> ((OnnxRuntimeSource.Download) source).fileName())
        .isEqualTo("onnxruntime.dll");
  }

  @Test
  void aUrlWithoutADigestIsRefused() {
    assertThatThrownBy(() -> resolve(Map.of(PREFIX + ".url", "https://example.org/libort.so")))
        .isInstanceOf(IllegalStateException.class)
        .hasMessageContaining(PREFIX + ".sha256");
  }

  @Test
  void aDigestWithoutAUrlIsRefused() {
    assertThatThrownBy(() -> resolve(Map.of(PREFIX + ".sha256", SHA)))
        .isInstanceOf(IllegalStateException.class)
        .hasMessageContaining(PREFIX + ".url");
  }

  @Test
  void aPathAndAUrlTogetherAreRefused() {
    assertThatThrownBy(
            () ->
                resolve(
                    Map.of(
                        PREFIX + ".path", "/x",
                        PREFIX + ".url", "https://example.org/libonnxruntime.so",
                        PREFIX + ".sha256", SHA)))
        .isInstanceOf(IllegalStateException.class)
        .hasMessageContaining("not both");
  }

  @Test
  void aMalformedDigestIsRefused() {
    assertThatThrownBy(
            () ->
                resolve(
                    Map.of(
                        PREFIX + ".url", "https://example.org/libonnxruntime.so",
                        PREFIX + ".sha256", "abc")))
        .isInstanceOf(IllegalArgumentException.class);
  }

  @Test
  void aFileNameWithAPathIsRefused() {
    assertThatThrownBy(
            () ->
                resolve(
                    Map.of(
                        PREFIX + ".url", "https://example.org/x",
                        PREFIX + ".sha256", SHA,
                        PREFIX + ".file", "../libonnxruntime.so")))
        .isInstanceOf(IllegalArgumentException.class);
  }

  @Test
  void otherSchemesAreRefused() {
    assertThatThrownBy(
            () ->
                resolve(
                    Map.of(
                        PREFIX + ".url",
                        "ftp://example.org/libonnxruntime.so",
                        PREFIX + ".sha256",
                        SHA)))
        .isInstanceOf(IllegalArgumentException.class);
  }

  @Test
  void nothingSetAndNothingPinnedLeavesItToTheNativeSide() {
    assertThat(resolve(Map.of())).isEmpty();
  }

  @Test
  void nothingSetUsesTheDefaultPinnedInTheJar(@TempDir Path classpath) throws IOException {
    Path pinned = Files.createDirectories(classpath.resolve("onnxruntime/" + PREFIX));
    Files.writeString(
        pinned.resolve("linux-x86_64.properties"),
        "url=https://github.com/ZirekHQ/dengjen/releases/download/onnxruntime-v1/libonnxruntime.so\n"
            + "sha256="
            + SHA
            + "\n");
    try (URLClassLoader loader = new URLClassLoader(new URL[] {classpath.toUri().toURL()}, null)) {
      assertThat(OnnxRuntimeSource.resolve(PREFIX, name -> null, loader, "linux-x86_64"))
          .get()
          .isInstanceOf(OnnxRuntimeSource.Download.class)
          .extracting(source -> ((OnnxRuntimeSource.Download) source).fileName())
          .isEqualTo("libonnxruntime.so");
      assertThat(OnnxRuntimeSource.resolve(PREFIX, name -> null, loader, "windows-x64")).isEmpty();
    }
  }

  @Test
  void userSettingsWinOverThePinnedDefault(@TempDir Path classpath) throws IOException {
    Path pinned = Files.createDirectories(classpath.resolve("onnxruntime/" + PREFIX));
    Files.writeString(
        pinned.resolve("linux-x86_64.properties"),
        "url=https://example.org/libonnxruntime.so\nsha256=" + SHA + "\n");
    try (URLClassLoader loader = new URLClassLoader(new URL[] {classpath.toUri().toURL()}, null)) {
      Map<String, String> properties = Map.of(PREFIX + ".path", "/mine/libonnxruntime.so");
      assertThat(OnnxRuntimeSource.resolve(PREFIX, properties::get, loader, "linux-x86_64"))
          .contains(new OnnxRuntimeSource.Local(Path.of("/mine/libonnxruntime.so")));
    }
  }
}
