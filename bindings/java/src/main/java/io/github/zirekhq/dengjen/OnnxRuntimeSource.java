package io.github.zirekhq.dengjen;

import java.io.IOException;
import java.io.InputStream;
import java.io.UncheckedIOException;
import java.net.URI;
import java.nio.file.Path;
import java.util.Locale;
import java.util.Optional;
import java.util.Properties;
import java.util.function.UnaryOperator;
import java.util.regex.Pattern;

/**
 * Where an ONNX Runtime library comes from, read from system properties under one prefix.
 *
 * <ul>
 *   <li>{@code <prefix>.path}: a library file already on disk, used as is.
 *   <li>{@code <prefix>.url} with {@code <prefix>.sha256}: downloaded once into the cache and
 *       checked against the digest. A URL without a digest is refused, so a user who swaps in
 *       another build (a GPU one, a newer patch) has to say exactly which file they trust.
 *   <li>Neither: the default this jar pins in {@code onnxruntime/<prefix>/<classifier>.properties}
 *       (keys {@code url}, {@code sha256}, optional {@code file}), or nothing when the jar pins
 *       none, which leaves the native side to find ONNX Runtime the way it did before.
 * </ul>
 */
sealed interface OnnxRuntimeSource {
  Pattern SHA256 = Pattern.compile("[0-9a-f]{64}");
  Pattern FILE_NAME = Pattern.compile("[A-Za-z0-9][A-Za-z0-9._+-]*");

  /** A library file on disk. */
  record Local(Path file) implements OnnxRuntimeSource {}

  /** A library to download, verify against {@code sha256} and keep as {@code fileName}. */
  record Download(URI url, String sha256, String fileName) implements OnnxRuntimeSource {
    public Download {
      sha256 = sha256.toLowerCase(Locale.ROOT);
      if (!SHA256.matcher(sha256).matches()) {
        throw new IllegalArgumentException("not a SHA-256 hex digest: " + sha256);
      }
      if (!FILE_NAME.matcher(fileName).matches()) {
        throw new IllegalArgumentException("not a plain library file name: " + fileName);
      }
      String scheme = url.getScheme() == null ? "" : url.getScheme().toLowerCase(Locale.ROOT);
      if (!scheme.equals("https") && !scheme.equals("http") && !scheme.equals("file")) {
        throw new IllegalArgumentException("ONNX Runtime URL must be https, http or file: " + url);
      }
    }
  }

  /**
   * Resolves the source for {@code prefix} (for example {@code dengjen.onnxruntime}) from {@code
   * properties}, falling back to the jar's default for {@code classifier}.
   */
  static Optional<OnnxRuntimeSource> resolve(
      String prefix, UnaryOperator<String> properties, ClassLoader loader, String classifier) {
    String path = properties.apply(prefix + ".path");
    String url = properties.apply(prefix + ".url");
    String sha256 = properties.apply(prefix + ".sha256");
    if (path != null && url != null) {
      throw new IllegalStateException(
          "set either -D" + prefix + ".path or -D" + prefix + ".url, not both");
    }
    if (path != null) {
      return Optional.of(new Local(Path.of(path)));
    }
    if (url != null) {
      if (sha256 == null) {
        throw new IllegalStateException(
            "-D"
                + prefix
                + ".url needs -D"
                + prefix
                + ".sha256 with the SHA-256 of that file, so a changed or corrupted download is"
                + " never loaded");
      }
      URI uri = URI.create(url);
      String file = properties.apply(prefix + ".file");
      return Optional.of(new Download(uri, sha256, file != null ? file : lastSegment(uri)));
    }
    if (sha256 != null) {
      throw new IllegalStateException(
          "-D" + prefix + ".sha256 is set without -D" + prefix + ".url");
    }
    return pinnedDefault(prefix, loader, classifier);
  }

  private static Optional<OnnxRuntimeSource> pinnedDefault(
      String prefix, ClassLoader loader, String classifier) {
    String resource = "onnxruntime/" + prefix + "/" + classifier + ".properties";
    try (InputStream in = loader.getResourceAsStream(resource)) {
      if (in == null) {
        return Optional.empty();
      }
      Properties pinned = new Properties();
      pinned.load(in);
      String url = pinned.getProperty("url");
      String sha256 = pinned.getProperty("sha256");
      if (url == null || sha256 == null) {
        throw new IllegalStateException(resource + " needs both url and sha256");
      }
      URI uri = URI.create(url);
      return Optional.of(new Download(uri, sha256, pinned.getProperty("file", lastSegment(uri))));
    } catch (IOException e) {
      throw new UncheckedIOException("cannot read " + resource, e);
    }
  }

  private static String lastSegment(URI url) {
    String path = url.getPath() == null ? "" : url.getPath();
    return path.substring(path.lastIndexOf('/') + 1);
  }
}
