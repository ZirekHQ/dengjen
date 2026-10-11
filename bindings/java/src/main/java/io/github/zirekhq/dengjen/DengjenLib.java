package io.github.zirekhq.dengjen;

import static java.lang.foreign.ValueLayout.ADDRESS;
import static java.lang.foreign.ValueLayout.JAVA_BOOLEAN;
import static java.lang.foreign.ValueLayout.JAVA_BYTE;
import static java.lang.foreign.ValueLayout.JAVA_FLOAT;
import static java.lang.foreign.ValueLayout.JAVA_INT;

import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.Linker;
import java.lang.foreign.SymbolLookup;
import java.lang.invoke.MethodHandle;

final class DengjenLib {
  private static final Linker LINKER = Linker.nativeLinker();
  private static final SymbolLookup LOOKUP = NativeLibraryLoader.load();

  private static MethodHandle handle(String symbol, FunctionDescriptor descriptor) {
    return LINKER.downcallHandle(
        LOOKUP
            .find(symbol)
            .orElseThrow(() -> new IllegalStateException("missing symbol: " + symbol)),
        descriptor);
  }

  /** Must equal {@code DENGJEN_ABI_VERSION} in {@code libdengjen.h}. */
  static final int EXPECTED_ABI_VERSION = 1;

  static {
    int actual;
    try {
      actual = (int) handle("libdengjenAbiVersion", FunctionDescriptor.of(JAVA_INT)).invokeExact();
    } catch (Throwable t) {
      throw new ExceptionInInitializerError(t);
    }
    if (actual != EXPECTED_ABI_VERSION) {
      String message = "native library ABI " + actual + " != bindings ABI " + EXPECTED_ABI_VERSION;
      throw new IllegalStateException(message);
    }
  }

  static final MethodHandle FREE_STRING =
      handle("libdengjenFreeString", FunctionDescriptor.ofVoid(ADDRESS));

  static final MethodHandle FREE_PIPER_SYNTH_CONFIG =
      handle("libdengjenFreePiperSynthConfig", FunctionDescriptor.ofVoid(ADDRESS));

  static final MethodHandle FREE_SYNTHESIS_EVENT =
      handle(
          "libdengjenFreeSynthesisEvent",
          FunctionDescriptor.ofVoid(DengjenLayouts.SYNTHESIS_EVENT));

  static final MethodHandle LOAD_VOICE_FROM_CONFIG_PATH =
      handle("libdengjenLoadVoiceFromConfigPath", FunctionDescriptor.of(ADDRESS, ADDRESS, ADDRESS));

  static final MethodHandle UNLOAD_DENGJEN_VOICE =
      handle("libdengjenUnloadDengjenVoice", FunctionDescriptor.ofVoid(ADDRESS));

  static final MethodHandle GET_AUDIO_INFO =
      handle("libdengjenGetAudioInfo", FunctionDescriptor.ofVoid(ADDRESS, ADDRESS, ADDRESS));

  static final MethodHandle GET_PIPER_DEFAULT_SYNTH_CONFIG =
      handle(
          "libdengjenGetPiperDefaultSynthConfig", FunctionDescriptor.of(ADDRESS, ADDRESS, ADDRESS));

  static final MethodHandle SET_PIPER_SYNTH_CONFIG =
      handle(
          "libdengjenSetPiperSynthConfig",
          FunctionDescriptor.ofVoid(ADDRESS, DengjenLayouts.PIPER_SYNTH_CONFIG, ADDRESS));

  static final MethodHandle SET_SYNTHESIS_PARAMETER =
      handle(
          "libdengjenSetSynthesisParameter",
          FunctionDescriptor.ofVoid(ADDRESS, ADDRESS, JAVA_FLOAT, ADDRESS));

  static final MethodHandle GET_SYNTHESIS_PARAMETER =
      handle(
          "libdengjenGetSynthesisParameter",
          FunctionDescriptor.of(JAVA_BOOLEAN, ADDRESS, ADDRESS, ADDRESS, ADDRESS));

  static final MethodHandle SPEAK =
      handle("libdengjenSpeak", FunctionDescriptor.ofVoid(ADDRESS, ADDRESS, ADDRESS, ADDRESS));

  static final MethodHandle CANCEL =
      handle("libdengjenCancel", FunctionDescriptor.ofVoid(ADDRESS, ADDRESS));

  static final MethodHandle SPEAK_TO_FILE =
      handle(
          "libdengjenSpeakToFile",
          FunctionDescriptor.of(JAVA_BYTE, ADDRESS, ADDRESS, ADDRESS, ADDRESS, ADDRESS));

  private DengjenLib() {}
}
