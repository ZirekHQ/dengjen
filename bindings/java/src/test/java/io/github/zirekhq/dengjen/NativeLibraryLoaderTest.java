package io.github.zirekhq.dengjen;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class NativeLibraryLoaderTest {
  @Test
  void extractsAClasspathResourceToATempFile() throws IOException {
    var path =
        NativeLibraryLoader.extractResource(
            "fixture/hello.txt", NativeLibraryLoaderTest.class.getClassLoader());

    assertThat(Files.readString(path)).isEqualTo("hello native world\n");
  }

  @Test
  void failsClearlyWhenTheResourceIsMissing() {
    var classLoader = NativeLibraryLoaderTest.class.getClassLoader();

    assertThatThrownBy(
            () ->
                NativeLibraryLoader.extractResource(
                    "natives/does-not-exist/libdengjen.so", classLoader))
        .isInstanceOf(IllegalStateException.class)
        .hasMessageContaining("natives/does-not-exist/libdengjen.so");
  }

  @Test
  void reusesTheSameFileForIdenticalContent(@TempDir Path dir) throws IOException {
    var loader = NativeLibraryLoaderTest.class.getClassLoader();

    var first = NativeLibraryLoader.extractResource("fixture/hello.txt", loader, dir);
    var second = NativeLibraryLoader.extractResource("fixture/hello.txt", loader, dir);

    assertThat(second).isEqualTo(first);
    try (var files = Files.list(dir)) {
      assertThat(files).hasSize(1);
    }
  }

  @Test
  void replacesATamperedFileWithTheExpectedContent(@TempDir Path dir) throws IOException {
    var loader = NativeLibraryLoaderTest.class.getClassLoader();
    var path = NativeLibraryLoader.extractResource("fixture/hello.txt", loader, dir);
    Files.writeString(path, "tampered");

    var again = NativeLibraryLoader.extractResource("fixture/hello.txt", loader, dir);

    assertThat(Files.readString(again)).isEqualTo("hello native world\n");
  }

  @Test
  void failsWithIOExceptionWhenTheTargetDirectoryIsMissing(@TempDir Path dir) {
    var loader = NativeLibraryLoaderTest.class.getClassLoader();

    assertThatThrownBy(
            () ->
                NativeLibraryLoader.extractResource(
                    "fixture/hello.txt", loader, dir.resolve("missing")))
        .isInstanceOf(IOException.class);
  }
}
