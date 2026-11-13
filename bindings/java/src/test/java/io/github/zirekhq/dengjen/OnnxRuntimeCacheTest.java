package io.github.zirekhq.dengjen;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

import com.sun.net.httpserver.HttpServer;
import java.io.IOException;
import java.net.InetAddress;
import java.net.InetSocketAddress;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Map;
import java.util.concurrent.atomic.AtomicInteger;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class OnnxRuntimeCacheTest {
  private static final byte[] LIBRARY = "pretend onnxruntime".getBytes(StandardCharsets.UTF_8);
  private static final String LIBRARY_SHA = NativeLibraryLoader.sha256Hex(LIBRARY);

  private HttpServer server;
  private final AtomicInteger requests = new AtomicInteger();

  @BeforeEach
  void serve() throws IOException {
    server = HttpServer.create(new InetSocketAddress(InetAddress.getLoopbackAddress(), 0), 0);
    server.createContext(
        "/libonnxruntime.so.1",
        exchange -> {
          requests.incrementAndGet();
          exchange.sendResponseHeaders(200, LIBRARY.length);
          exchange.getResponseBody().write(LIBRARY);
          exchange.close();
        });
    server.createContext(
        "/missing",
        exchange -> {
          exchange.sendResponseHeaders(404, -1);
          exchange.close();
        });
    server.start();
  }

  @AfterEach
  void stop() {
    server.stop(0);
  }

  private URI url(String path) {
    return URI.create("http://127.0.0.1:" + server.getAddress().getPort() + path);
  }

  @Test
  void downloadsOnceAndKeepsTheFileUnderItsDigestAndName(@TempDir Path cache) throws IOException {
    OnnxRuntimeCache onnxRuntimeCache = new OnnxRuntimeCache(cache);
    var source =
        new OnnxRuntimeSource.Download(
            url("/libonnxruntime.so.1"), LIBRARY_SHA, "libonnxruntime.so.1");

    Path first = onnxRuntimeCache.fetch(source);
    Path second = onnxRuntimeCache.fetch(source);

    assertThat(first).isEqualTo(cache.resolve(LIBRARY_SHA).resolve("libonnxruntime.so.1"));
    assertThat(first).hasBinaryContent(LIBRARY);
    assertThat(second).isEqualTo(first);
    assertThat(requests).hasValue(1);
  }

  @Test
  void aDigestMismatchFailsAndLeavesNothingBehind(@TempDir Path cache) throws IOException {
    String wrong = "00".repeat(32);
    var source =
        new OnnxRuntimeSource.Download(url("/libonnxruntime.so.1"), wrong, "libonnxruntime.so.1");

    assertThatThrownBy(() -> new OnnxRuntimeCache(cache).fetch(source))
        .isInstanceOf(IOException.class)
        .hasMessageContaining("checksum mismatch")
        .hasMessageContaining(LIBRARY_SHA);
    try (var files = Files.list(cache.resolve(wrong))) {
      assertThat(files.map(p -> p.getFileName().toString())).containsOnly(".lock");
    }
  }

  @Test
  void aCorruptedCachedFileIsDownloadedAgain(@TempDir Path cache) throws IOException {
    Path cached =
        Files.createDirectories(cache.resolve(LIBRARY_SHA)).resolve("libonnxruntime.so.1");
    Files.writeString(cached, "truncated");
    var source =
        new OnnxRuntimeSource.Download(
            url("/libonnxruntime.so.1"), LIBRARY_SHA, "libonnxruntime.so.1");

    assertThat(new OnnxRuntimeCache(cache).fetch(source)).hasBinaryContent(LIBRARY);
    assertThat(requests).hasValue(1);
  }

  @Test
  void anHttpErrorNamesTheStatus(@TempDir Path cache) {
    var source = new OnnxRuntimeSource.Download(url("/missing"), LIBRARY_SHA, "libonnxruntime.so");

    assertThatThrownBy(() -> new OnnxRuntimeCache(cache).fetch(source))
        .isInstanceOf(IOException.class)
        .hasMessageContaining("HTTP 404");
  }

  @Test
  void aFileUrlIsCopiedAndChecked(@TempDir Path cache, @TempDir Path elsewhere) throws IOException {
    Path local = Files.write(elsewhere.resolve("onnxruntime.dll"), LIBRARY);
    var source = new OnnxRuntimeSource.Download(local.toUri(), LIBRARY_SHA, "onnxruntime.dll");

    assertThat(new OnnxRuntimeCache(cache).fetch(source)).hasBinaryContent(LIBRARY);
  }

  @Test
  void theCacheDirectoryFollowsThePlatform() {
    Map<String, String> linux = Map.of("os.name", "Linux", "user.home", "/home/a");
    Map<String, String> mac = Map.of("os.name", "Mac OS X", "user.home", "/Users/a");
    Map<String, String> windows = Map.of("os.name", "Windows 11", "user.home", "C:/Users/a");
    Map<String, String> none = Map.of();

    assertThat(OnnxRuntimeCache.defaultRoot(linux::get, none::get))
        .isEqualTo(Path.of("/home/a/.cache/dengjen"));
    assertThat(OnnxRuntimeCache.defaultRoot(linux::get, Map.of("XDG_CACHE_HOME", "/xdg")::get))
        .isEqualTo(Path.of("/xdg/dengjen"));
    assertThat(OnnxRuntimeCache.defaultRoot(mac::get, none::get))
        .isEqualTo(Path.of("/Users/a/Library/Caches/dengjen"));
    assertThat(OnnxRuntimeCache.defaultRoot(windows::get, Map.of("LOCALAPPDATA", "C:/L")::get))
        .isEqualTo(Path.of("C:/L/dengjen/cache"));
    assertThat(
            OnnxRuntimeCache.defaultRoot(
                Map.of(OnnxRuntimeCache.CACHE_DIR_PROPERTY, "/custom")::get, none::get))
        .isEqualTo(Path.of("/custom"));
  }
}
