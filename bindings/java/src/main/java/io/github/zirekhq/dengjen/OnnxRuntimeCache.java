package io.github.zirekhq.dengjen;

import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.channels.FileChannel;
import java.nio.channels.FileLock;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.nio.file.StandardOpenOption;
import java.security.DigestInputStream;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.time.Duration;
import java.util.HexFormat;
import java.util.Locale;
import java.util.function.UnaryOperator;

/**
 * Downloads an ONNX Runtime library once and keeps it under {@code <cache>/<sha256>/<file name>}.
 *
 * <p>The digest names the directory, so a file is only ever reused for the digest it was checked
 * against; the original file name is kept because other libraries link against it by name. The
 * download goes to a temporary file, is hashed while it streams, and is renamed into place only
 * when the digest matches. A file lock on the directory keeps two processes from downloading the
 * same file at once.
 */
final class OnnxRuntimeCache {
  static final String CACHE_DIR_PROPERTY = "dengjen.cache.dir";

  private static final Duration TIMEOUT = Duration.ofMinutes(5);

  private final Path root;

  OnnxRuntimeCache(Path root) {
    this.root = root;
  }

  /** The cache under {@value #CACHE_DIR_PROPERTY}, or the platform's per-user cache directory. */
  static OnnxRuntimeCache fromEnvironment(
      UnaryOperator<String> properties, UnaryOperator<String> environment) {
    return new OnnxRuntimeCache(defaultRoot(properties, environment).resolve("onnxruntime"));
  }

  static Path defaultRoot(UnaryOperator<String> properties, UnaryOperator<String> environment) {
    String override = properties.apply(CACHE_DIR_PROPERTY);
    if (override != null) {
      return Path.of(override);
    }
    String home = properties.apply("user.home");
    String os = String.valueOf(properties.apply("os.name")).toLowerCase(Locale.ROOT);
    if (os.startsWith("windows")) {
      String local = environment.apply("LOCALAPPDATA");
      return local != null ? Path.of(local, "dengjen", "cache") : Path.of(home, "dengjen", "cache");
    }
    if (os.startsWith("mac")) {
      return Path.of(home, "Library", "Caches", "dengjen");
    }
    String xdg = environment.apply("XDG_CACHE_HOME");
    return xdg != null && !xdg.isEmpty()
        ? Path.of(xdg, "dengjen")
        : Path.of(home, ".cache", "dengjen");
  }

  /** Returns the verified library for {@code source}, downloading it when the cache lacks it. */
  Path fetch(OnnxRuntimeSource.Download source) throws IOException {
    Path directory = Files.createDirectories(root.resolve(source.sha256()));
    Path target = directory.resolve(source.fileName());
    if (matches(target, source.sha256())) {
      return target;
    }
    try (FileChannel lockFile =
            FileChannel.open(
                directory.resolve(".lock"), StandardOpenOption.CREATE, StandardOpenOption.WRITE);
        FileLock ignored = lockFile.lock()) {
      // Another process may have finished the download while this one waited for the lock.
      if (matches(target, source.sha256())) {
        return target;
      }
      Path partial = Files.createTempFile(directory, "partial-", ".tmp");
      try {
        String actual = download(source, partial);
        if (!actual.equals(source.sha256())) {
          throw new IOException(
              "checksum mismatch for "
                  + source.url()
                  + ": expected sha256 "
                  + source.sha256()
                  + ", got "
                  + actual
                  + ". The file was not used.");
        }
        moveIntoPlace(partial, target);
      } finally {
        Files.deleteIfExists(partial);
      }
    }
    return target;
  }

  private static String download(OnnxRuntimeSource.Download source, Path partial)
      throws IOException {
    if ("file".equalsIgnoreCase(source.url().getScheme())) {
      try (InputStream in = Files.newInputStream(Path.of(source.url()))) {
        return copyHashing(in, partial);
      }
    }
    HttpClient client =
        HttpClient.newBuilder()
            .followRedirects(HttpClient.Redirect.NORMAL)
            .connectTimeout(Duration.ofSeconds(30))
            .build();
    HttpRequest request = HttpRequest.newBuilder(source.url()).timeout(TIMEOUT).GET().build();
    try {
      HttpResponse<InputStream> response =
          client.send(request, HttpResponse.BodyHandlers.ofInputStream());
      try (InputStream in = response.body()) {
        if (response.statusCode() != 200) {
          throw new IOException(
              "downloading " + source.url() + " failed with HTTP " + response.statusCode());
        }
        return copyHashing(in, partial);
      }
    } catch (InterruptedException e) {
      Thread.currentThread().interrupt();
      throw new IOException("interrupted while downloading " + source.url(), e);
    }
  }

  private static String copyHashing(InputStream in, Path partial) throws IOException {
    MessageDigest digest = sha256();
    try (DigestInputStream hashing = new DigestInputStream(in, digest);
        OutputStream out = Files.newOutputStream(partial)) {
      hashing.transferTo(out);
    }
    return HexFormat.of().formatHex(digest.digest());
  }

  private static boolean matches(Path file, String sha256) throws IOException {
    if (!Files.isRegularFile(file)) {
      return false;
    }
    try (InputStream in = Files.newInputStream(file)) {
      MessageDigest digest = sha256();
      try (DigestInputStream hashing = new DigestInputStream(in, digest)) {
        hashing.transferTo(OutputStream.nullOutputStream());
      }
      return HexFormat.of().formatHex(digest.digest()).equals(sha256);
    }
  }

  private static void moveIntoPlace(Path partial, Path target) throws IOException {
    try {
      Files.move(
          partial, target, StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE);
    } catch (AtomicMoveNotSupportedException e) {
      Files.move(partial, target, StandardCopyOption.REPLACE_EXISTING);
    }
  }

  private static MessageDigest sha256() {
    try {
      return MessageDigest.getInstance("SHA-256");
    } catch (NoSuchAlgorithmException e) {
      throw new IllegalStateException("SHA-256 unavailable", e);
    }
  }
}
