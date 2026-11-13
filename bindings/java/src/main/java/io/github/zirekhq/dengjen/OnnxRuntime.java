package io.github.zirekhq.dengjen;

import java.io.IOException;
import java.io.UncheckedIOException;
import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.invoke.MethodHandle;
import java.util.Optional;
import java.util.function.UnaryOperator;

/**
 * Points a dynamically linked native library at the onnxruntime the {@code dengjen.onnxruntime.*}
 * system properties name (see {@link OnnxRuntimeSource}), downloading it first when they give a
 * URL.
 */
final class OnnxRuntime {
  static final String PREFIX = "dengjen.onnxruntime";
  static final String PATH_PROPERTY = PREFIX + ".path";

  private static boolean configured;

  private OnnxRuntime() {}

  /**
   * Applies the configured onnxruntime once per process; without any setting (and no default pinned
   * in the jar) the native side falls back to {@code ORT_DYLIB_PATH} and the platform loader search
   * path.
   */
  static synchronized void configure() {
    if (!configured) {
      if (DengjenLib.SET_ORT_DYLIB_PATH.isEmpty() && !isExplicit(System::getProperty)) {
        // This native library embeds onnxruntime, so the pinned default has nothing to configure.
        return;
      }
      configure(resolvePath(), DengjenLib.SET_ORT_DYLIB_PATH);
    }
  }

  private static boolean isExplicit(UnaryOperator<String> properties) {
    return properties.apply(PREFIX + ".path") != null || properties.apply(PREFIX + ".url") != null;
  }

  private static String resolvePath() {
    String classifier =
        NativePlatform.classifier(System.getProperty("os.name"), System.getProperty("os.arch"));
    return resolveSource(
            System::getProperty, System::getenv, OnnxRuntime.class.getClassLoader(), classifier)
        .map(OnnxRuntime::localFile)
        .orElse(null);
  }

  /**
   * The source to apply. A jar's pinned default never overrides an {@code ORT_DYLIB_PATH} the user
   * already exported; an explicit {@code -D} setting still does.
   */
  static Optional<OnnxRuntimeSource> resolveSource(
      UnaryOperator<String> properties,
      UnaryOperator<String> environment,
      ClassLoader loader,
      String classifier) {
    boolean explicit =
        properties.apply(PREFIX + ".path") != null || properties.apply(PREFIX + ".url") != null;
    String envPath = environment.apply("ORT_DYLIB_PATH");
    if (!explicit && envPath != null && !envPath.isBlank()) {
      return Optional.empty();
    }
    return OnnxRuntimeSource.resolve(PREFIX, properties, loader, classifier);
  }

  /** The file for {@code source}, downloaded into the per-user cache when it is remote. */
  static String localFile(OnnxRuntimeSource source) {
    return switch (source) {
      case OnnxRuntimeSource.Local local -> local.file().toString();
      case OnnxRuntimeSource.Download download -> {
        try {
          yield OnnxRuntimeCache.fromEnvironment(System::getProperty, System::getenv)
              .fetch(download)
              .toString();
        } catch (IOException e) {
          throw new UncheckedIOException("cannot get ONNX Runtime from " + download.url(), e);
        }
      }
    };
  }

  static synchronized void configure(String path, Optional<MethodHandle> setter) {
    if (configured || path == null) {
      return;
    }
    apply(path, setter);
    configured = true;
  }

  static void apply(String path, Optional<MethodHandle> setter) {
    MethodHandle setPath =
        setter.orElseThrow(
            () ->
                new IllegalStateException(
                    "-D" + PATH_PROPERTY + " is set but this native library embeds onnxruntime"));
    try (Arena arena = Arena.ofConfined()) {
      MemorySegment outError = arena.allocate(DengjenLayouts.EXTERN_ERROR);
      try {
        setPath.invokeExact(arena.allocateFrom(path), outError);
      } catch (Throwable t) {
        throw new IllegalStateException("libdengjenSetOrtDylibPath downcall failed", t);
      }
      ErrorChecks.checkAndThrow(outError);
    }
  }
}
