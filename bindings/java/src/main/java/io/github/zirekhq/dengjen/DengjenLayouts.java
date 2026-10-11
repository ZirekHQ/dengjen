package io.github.zirekhq.dengjen;

import static java.lang.foreign.ValueLayout.ADDRESS;
import static java.lang.foreign.ValueLayout.JAVA_BYTE;
import static java.lang.foreign.ValueLayout.JAVA_FLOAT;
import static java.lang.foreign.ValueLayout.JAVA_INT;
import static java.lang.foreign.ValueLayout.JAVA_LONG;

import java.lang.foreign.GroupLayout;
import java.lang.foreign.MemoryLayout;
import java.lang.foreign.MemoryLayout.PathElement;

final class DengjenLayouts {
  static final GroupLayout EXTERN_ERROR =
      MemoryLayout.structLayout(
              JAVA_INT.withName("code"), MemoryLayout.paddingLayout(4), ADDRESS.withName("message"))
          .withName("ExternError");
  static final long EXTERN_ERROR_CODE_OFFSET =
      EXTERN_ERROR.byteOffset(PathElement.groupElement("code"));
  static final long EXTERN_ERROR_MESSAGE_OFFSET =
      EXTERN_ERROR.byteOffset(PathElement.groupElement("message"));

  static final GroupLayout PIPER_SYNTH_CONFIG =
      MemoryLayout.structLayout(
              JAVA_INT.withName("speaker"),
              JAVA_FLOAT.withName("length_scale"),
              JAVA_FLOAT.withName("noise_scale"),
              JAVA_FLOAT.withName("noise_w"))
          .withName("PiperSynthConfig");
  static final long PIPER_SYNTH_CONFIG_SPEAKER_OFFSET =
      PIPER_SYNTH_CONFIG.byteOffset(PathElement.groupElement("speaker"));
  static final long PIPER_SYNTH_CONFIG_LENGTH_SCALE_OFFSET =
      PIPER_SYNTH_CONFIG.byteOffset(PathElement.groupElement("length_scale"));
  static final long PIPER_SYNTH_CONFIG_NOISE_SCALE_OFFSET =
      PIPER_SYNTH_CONFIG.byteOffset(PathElement.groupElement("noise_scale"));
  static final long PIPER_SYNTH_CONFIG_NOISE_W_OFFSET =
      PIPER_SYNTH_CONFIG.byteOffset(PathElement.groupElement("noise_w"));

  static final GroupLayout AUDIO_INFO =
      MemoryLayout.structLayout(
              JAVA_INT.withName("sample_rate"),
              JAVA_INT.withName("num_channels"),
              JAVA_INT.withName("sample_width"))
          .withName("AudioInfo");
  static final long AUDIO_INFO_SAMPLE_RATE_OFFSET =
      AUDIO_INFO.byteOffset(PathElement.groupElement("sample_rate"));
  static final long AUDIO_INFO_NUM_CHANNELS_OFFSET =
      AUDIO_INFO.byteOffset(PathElement.groupElement("num_channels"));
  static final long AUDIO_INFO_SAMPLE_WIDTH_OFFSET =
      AUDIO_INFO.byteOffset(PathElement.groupElement("sample_width"));

  static final GroupLayout SYNTHESIS_EVENT =
      MemoryLayout.structLayout(
              JAVA_INT.withName("event_type"),
              MemoryLayout.paddingLayout(4),
              ADDRESS.withName("error_ptr"),
              JAVA_LONG.withName("len"),
              ADDRESS.withName("data"))
          .withName("SynthesisEvent");
  static final long SYNTHESIS_EVENT_TYPE_OFFSET =
      SYNTHESIS_EVENT.byteOffset(PathElement.groupElement("event_type"));
  static final long SYNTHESIS_EVENT_ERROR_PTR_OFFSET =
      SYNTHESIS_EVENT.byteOffset(PathElement.groupElement("error_ptr"));
  static final long SYNTHESIS_EVENT_LEN_OFFSET =
      SYNTHESIS_EVENT.byteOffset(PathElement.groupElement("len"));
  static final long SYNTHESIS_EVENT_DATA_OFFSET =
      SYNTHESIS_EVENT.byteOffset(PathElement.groupElement("data"));

  static final GroupLayout SYNTHESIS_PARAMS =
      MemoryLayout.structLayout(
              JAVA_INT.withName("struct_size"),
              JAVA_INT.withName("mode"),
              JAVA_BYTE.withName("rate"),
              JAVA_BYTE.withName("volume"),
              JAVA_BYTE.withName("pitch"),
              MemoryLayout.paddingLayout(1),
              JAVA_INT.withName("appended_silence_ms"),
              ADDRESS.withName("callback"),
              JAVA_BYTE.withName("nonblocking"),
              MemoryLayout.paddingLayout(7),
              ADDRESS.withName("user_data"))
          .withName("SynthesisParams");
  static final long SYNTHESIS_PARAMS_STRUCT_SIZE_OFFSET =
      SYNTHESIS_PARAMS.byteOffset(PathElement.groupElement("struct_size"));
  static final long SYNTHESIS_PARAMS_MODE_OFFSET =
      SYNTHESIS_PARAMS.byteOffset(PathElement.groupElement("mode"));
  static final long SYNTHESIS_PARAMS_RATE_OFFSET =
      SYNTHESIS_PARAMS.byteOffset(PathElement.groupElement("rate"));
  static final long SYNTHESIS_PARAMS_VOLUME_OFFSET =
      SYNTHESIS_PARAMS.byteOffset(PathElement.groupElement("volume"));
  static final long SYNTHESIS_PARAMS_PITCH_OFFSET =
      SYNTHESIS_PARAMS.byteOffset(PathElement.groupElement("pitch"));
  static final long SYNTHESIS_PARAMS_APPENDED_SILENCE_MS_OFFSET =
      SYNTHESIS_PARAMS.byteOffset(PathElement.groupElement("appended_silence_ms"));
  static final long SYNTHESIS_PARAMS_CALLBACK_OFFSET =
      SYNTHESIS_PARAMS.byteOffset(PathElement.groupElement("callback"));
  static final long SYNTHESIS_PARAMS_NONBLOCKING_OFFSET =
      SYNTHESIS_PARAMS.byteOffset(PathElement.groupElement("nonblocking"));
  static final long SYNTHESIS_PARAMS_USER_DATA_OFFSET =
      SYNTHESIS_PARAMS.byteOffset(PathElement.groupElement("user_data"));

  private DengjenLayouts() {}
}
