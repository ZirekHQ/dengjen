package io.github.zirekhq.dengjen;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;

final class SynthesisParamsMarshaller {
  private SynthesisParamsMarshaller() {}

  static MemorySegment allocate(
      Arena arena, SynthesisParams params, MemorySegment callback, MemorySegment userData) {
    MemorySegment cParams = arena.allocate(DengjenLayouts.SYNTHESIS_PARAMS);
    cParams.set(
        ValueLayout.JAVA_INT,
        DengjenLayouts.SYNTHESIS_PARAMS_STRUCT_SIZE_OFFSET,
        (int) DengjenLayouts.SYNTHESIS_PARAMS.byteSize());
    cParams.set(
        ValueLayout.JAVA_INT, DengjenLayouts.SYNTHESIS_PARAMS_MODE_OFFSET, params.mode().value());
    cParams.set(
        ValueLayout.JAVA_BYTE, DengjenLayouts.SYNTHESIS_PARAMS_RATE_OFFSET, (byte) params.rate());
    cParams.set(
        ValueLayout.JAVA_BYTE,
        DengjenLayouts.SYNTHESIS_PARAMS_VOLUME_OFFSET,
        (byte) params.volume());
    cParams.set(
        ValueLayout.JAVA_BYTE, DengjenLayouts.SYNTHESIS_PARAMS_PITCH_OFFSET, (byte) params.pitch());
    cParams.set(
        ValueLayout.JAVA_INT,
        DengjenLayouts.SYNTHESIS_PARAMS_APPENDED_SILENCE_MS_OFFSET,
        params.appendedSilenceMs());
    cParams.set(ValueLayout.ADDRESS, DengjenLayouts.SYNTHESIS_PARAMS_CALLBACK_OFFSET, callback);
    cParams.set(
        ValueLayout.JAVA_BYTE,
        DengjenLayouts.SYNTHESIS_PARAMS_NONBLOCKING_OFFSET,
        (byte) (params.nonblocking() ? 1 : 0));
    cParams.set(ValueLayout.ADDRESS, DengjenLayouts.SYNTHESIS_PARAMS_USER_DATA_OFFSET, userData);
    return cParams;
  }
}
