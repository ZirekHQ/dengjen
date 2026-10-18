package io.github.zirekhq.dengjen;

import java.io.IOException;
import java.io.UncheckedIOException;
import java.lang.foreign.Arena;
import java.lang.foreign.SymbolLookup;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Comparator;
import java.util.List;

final class SttNativeLoader {
  static final String DIR_PROPERTY = "dengjen.stt.native.dir";
  private static final String STT = "dengjen_stt";
  private static final String SHERPA = "sherpa-onnx-c-api";
  private static final String RUNTIME = "onnxruntime";

  private SttNativeLoader() {}

  /** Loads the libraries dependencies-first and returns the lookup of the STT library. */
  static SymbolLookup load() {
    String override = System.getProperty(DIR_PROPERTY);
    Path directory = override != null ? Path.of(override) : extractFromClasspath();
    SymbolLookup stt = null;
    for (String name : loadOrder(listLibraries(directory))) {
      SymbolLookup lookup = SymbolLookup.libraryLookup(directory.resolve(name), Arena.global());
      if (name.contains(STT)) {
        stt = lookup;
      }
    }
    if (stt == null) {
      throw new IllegalStateException("no " + STT + " library in " + directory);
    }
    return stt;
  }

  /** Orders library file names so each loads after the libraries it links against. */
  static List<String> loadOrder(List<String> names) {
    names.forEach(SttNativeLoader::requireKnown);
    return names.stream().sorted(Comparator.comparingInt(SttNativeLoader::rank)).toList();
  }

  private static int rank(String name) {
    if (name.contains(RUNTIME)) {
      return 0;
    }
    return name.contains(SHERPA) ? 1 : 2;
  }

  private static boolean isKnown(String name) {
    return name.contains(RUNTIME) || name.contains(SHERPA) || name.contains(STT);
  }

  private static void requireKnown(String name) {
    if (!isKnown(name)) {
      throw new IllegalStateException("unexpected file in the STT natives: " + name);
    }
  }

  private static List<String> listLibraries(Path directory) {
    try (var files = Files.list(directory)) {
      return files.map(p -> p.getFileName().toString()).filter(SttNativeLoader::isKnown).toList();
    } catch (IOException e) {
      throw new UncheckedIOException("cannot list " + directory, e);
    }
  }

  private static Path extractFromClasspath() {
    String classifier =
        NativePlatform.classifier(System.getProperty("os.name"), System.getProperty("os.arch"));
    return extractManifest(SttNativeLoader.class.getClassLoader(), classifier);
  }

  static Path extractManifest(ClassLoader loader, String classifier) {
    String base = "natives/" + classifier + "/";
    try (var manifest = loader.getResourceAsStream(base + "MANIFEST")) {
      if (manifest == null) {
        throw new IllegalStateException(
            "no STT natives on the classpath at '"
                + base
                + "' -- add a runtimeOnly dependency on io.github.zirekhq:dengjen-stt-natives:<version>:"
                + classifier
                + ", or set -D"
                + DIR_PROPERTY
                + " to a directory you built yourself");
      }
      List<String> names =
          new String(manifest.readAllBytes(), StandardCharsets.UTF_8)
              .lines()
              .map(String::strip)
              .filter(name -> !name.isEmpty())
              .toList();
      Path directory =
          NativeLibraryLoader.privateSubdirectory("stt-" + contentKey(base, names, loader));
      for (String name : names) {
        NativeLibraryLoader.extractInto(base + name, loader, directory, name);
      }
      return directory;
    } catch (IOException e) {
      throw new UncheckedIOException("failed to extract the STT natives", e);
    }
  }

  /**
   * Digest of every entry's name and bytes, so two versions with equal file names never share a
   * directory.
   */
  private static String contentKey(String base, List<String> names, ClassLoader loader)
      throws IOException {
    StringBuilder entries = new StringBuilder();
    for (String name : names) {
      try (var in = loader.getResourceAsStream(base + name)) {
        if (in == null) {
          throw new IllegalStateException(
              "STT natives manifest lists a missing file: " + base + name);
        }
        entries
            .append(name)
            .append(':')
            .append(NativeLibraryLoader.sha256Hex(in.readAllBytes()))
            .append('\n');
      }
    }
    return NativeLibraryLoader.sha256Hex(entries.toString().getBytes(StandardCharsets.UTF_8));
  }
}
