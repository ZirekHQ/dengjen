package io.github.zirekhq.dengjen;

import java.io.IOException;
import java.io.InputStream;
import java.io.UncheckedIOException;
import java.lang.foreign.Arena;
import java.lang.foreign.SymbolLookup;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.FileSystems;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.nio.file.attribute.FileAttribute;
import java.nio.file.attribute.PosixFilePermissions;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.Arrays;
import java.util.HexFormat;

final class NativeLibraryLoader {
  private NativeLibraryLoader() {}

  /** Classpath resource names use `/` on every platform, unlike {@code File.separator}. */
  private static final String RESOURCE_PATH_DELIMITER = "/";

  private static final String OWNER_ONLY_MODE = "rwx------";

  private static final FileAttribute<?>[] OWNER_ONLY_PERMISSIONS =
      FileSystems.getDefault().supportedFileAttributeViews().contains("posix")
          ? new FileAttribute<?>[] {
            PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString("rw-------"))
          }
          : new FileAttribute<?>[0];

  static SymbolLookup load() {
    String override = System.getProperty("dengjen.native.library.path");
    if (override != null) {
      return SymbolLookup.libraryLookup(Path.of(override), Arena.global());
    }
    String classifier =
        NativePlatform.classifier(System.getProperty("os.name"), System.getProperty("os.arch"));
    String libraryName = System.mapLibraryName("libdengjen");

    String resourcePath = String.join(RESOURCE_PATH_DELIMITER, "natives", classifier, libraryName);
    Path extracted;
    try {
      extracted = extractResource(resourcePath, NativeLibraryLoader.class.getClassLoader());
    } catch (IOException e) {
      throw new UncheckedIOException("failed to extract native library " + resourcePath, e);
    }
    return SymbolLookup.libraryLookup(extracted, Arena.global());
  }

  static Path extractResource(String resourcePath, ClassLoader loader) throws IOException {
    return extractResource(resourcePath, loader, extractionDirectory());
  }

  static Path extractResource(String resourcePath, ClassLoader loader, Path directory)
      throws IOException {
    byte[] content = readResource(resourcePath, loader);
    String hash = sha256Hex(content);
    Path target = directory.resolve(hash + "-" + resourcePath.replace('/', '_'));
    if (Files.isRegularFile(target) && sha256Hex(Files.readAllBytes(target)).equals(hash)) {
      return target;
    }
    writeAtomically(content, directory, target);
    return target;
  }

  /**
   * Extracts the resource under its original file name, so the loader resolves sibling libraries.
   */
  static Path extractInto(String resourcePath, ClassLoader loader, Path directory, String fileName)
      throws IOException {
    byte[] content = readResource(resourcePath, loader);
    Path target = directory.resolve(fileName);
    if (Files.isRegularFile(target) && Arrays.equals(Files.readAllBytes(target), content)) {
      return target;
    }
    writeAtomically(content, directory, target);
    return target;
  }

  static Path privateSubdirectory(String name) throws IOException {
    Path directory = extractionDirectory().resolve(name);
    FileAttribute<?>[] dirAttrs =
        OWNER_ONLY_PERMISSIONS.length == 0
            ? new FileAttribute<?>[0]
            : new FileAttribute<?>[] {
              PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString(OWNER_ONLY_MODE))
            };
    Files.createDirectories(directory, dirAttrs);
    return directory;
  }

  private static byte[] readResource(String resourcePath, ClassLoader loader) throws IOException {
    try (InputStream in = loader.getResourceAsStream(resourcePath)) {
      if (in == null) {
        throw new IllegalStateException(
            "no native library found on the classpath at '"
                + resourcePath
                + "' -- add a runtimeOnly dependency on the matching "
                + "io.github.zirekhq:dengjen-tts:<version>:<classifier> "
                + "artifact, or set -Ddengjen.native.library.path to a library you built "
                + "yourself");
      }
      return in.readAllBytes();
    }
  }

  private static void writeAtomically(byte[] content, Path directory, Path target)
      throws IOException {
    Path partial = Files.createTempFile(directory, "partial-", ".tmp", OWNER_ONLY_PERMISSIONS);
    try {
      Files.write(partial, content);
      moveIntoPlace(partial, target);
    } catch (IOException | RuntimeException e) {
      Files.deleteIfExists(partial);
      throw e;
    }
  }

  private static Path extractionDirectory() throws IOException {
    String user = System.getProperty("user.name", "unknown").replaceAll("[^A-Za-z0-9_.-]", "_");
    Path directory = Path.of(System.getProperty("java.io.tmpdir"), "dengjen-native-" + user);
    FileAttribute<?>[] dirAttrs =
        OWNER_ONLY_PERMISSIONS.length == 0
            ? new FileAttribute<?>[0]
            : new FileAttribute<?>[] {
              PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString(OWNER_ONLY_MODE))
            };
    Files.createDirectories(directory, dirAttrs);
    if (OWNER_ONLY_PERMISSIONS.length > 0
        && (!ownedByCurrentUser(directory)
            || !Files.getPosixFilePermissions(directory)
                .equals(PosixFilePermissions.fromString(OWNER_ONLY_MODE)))) {
      throw new IOException(
          "refusing to extract native library into " + directory + ": not private to " + user);
    }
    return directory;
  }

  private static void moveIntoPlace(Path partial, Path target) throws IOException {
    try {
      Files.move(
          partial, target, StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE);
    } catch (AtomicMoveNotSupportedException e) {
      Files.move(partial, target, StandardCopyOption.REPLACE_EXISTING);
    }
  }

  private static boolean ownedByCurrentUser(Path directory) throws IOException {
    Path probe = Files.createTempFile(directory, "owner-", ".probe", OWNER_ONLY_PERMISSIONS);
    try {
      return Files.getOwner(directory).equals(Files.getOwner(probe));
    } finally {
      Files.deleteIfExists(probe);
    }
  }

  static String sha256Hex(byte[] content) {
    try {
      return HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(content));
    } catch (NoSuchAlgorithmException e) {
      throw new IllegalStateException("SHA-256 unavailable", e);
    }
  }
}
