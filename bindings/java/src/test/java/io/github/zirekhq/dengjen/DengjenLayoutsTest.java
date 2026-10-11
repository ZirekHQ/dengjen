package io.github.zirekhq.dengjen;

import static org.assertj.core.api.Assertions.assertThat;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.regex.Pattern;
import org.junit.jupiter.api.Test;

class DengjenLayoutsTest {
  @Test
  void externErrorMatchesTheCStructLayout() {
    assertThat(DengjenLayouts.EXTERN_ERROR.byteSize()).isEqualTo(16);
    assertThat(DengjenLayouts.EXTERN_ERROR_CODE_OFFSET).isEqualTo(0);
    assertThat(DengjenLayouts.EXTERN_ERROR_MESSAGE_OFFSET).isEqualTo(8);
  }

  @Test
  void piperSynthConfigMatchesTheCStructLayout() {
    assertThat(DengjenLayouts.PIPER_SYNTH_CONFIG.byteSize()).isEqualTo(16);
    assertThat(DengjenLayouts.PIPER_SYNTH_CONFIG_SPEAKER_OFFSET).isEqualTo(0);
    assertThat(DengjenLayouts.PIPER_SYNTH_CONFIG_LENGTH_SCALE_OFFSET).isEqualTo(4);
    assertThat(DengjenLayouts.PIPER_SYNTH_CONFIG_NOISE_SCALE_OFFSET).isEqualTo(8);
    assertThat(DengjenLayouts.PIPER_SYNTH_CONFIG_NOISE_W_OFFSET).isEqualTo(12);
  }

  @Test
  void audioInfoMatchesTheCStructLayout() {
    assertThat(DengjenLayouts.AUDIO_INFO.byteSize()).isEqualTo(12);
    assertThat(DengjenLayouts.AUDIO_INFO_SAMPLE_RATE_OFFSET).isEqualTo(0);
    assertThat(DengjenLayouts.AUDIO_INFO_NUM_CHANNELS_OFFSET).isEqualTo(4);
    assertThat(DengjenLayouts.AUDIO_INFO_SAMPLE_WIDTH_OFFSET).isEqualTo(8);
  }

  @Test
  void synthesisEventMatchesTheCStructLayout() {
    assertThat(DengjenLayouts.SYNTHESIS_EVENT.byteSize()).isEqualTo(32);
    assertThat(DengjenLayouts.SYNTHESIS_EVENT_TYPE_OFFSET).isEqualTo(0);
    assertThat(DengjenLayouts.SYNTHESIS_EVENT_ERROR_PTR_OFFSET).isEqualTo(8);
    assertThat(DengjenLayouts.SYNTHESIS_EVENT_LEN_OFFSET).isEqualTo(16);
    assertThat(DengjenLayouts.SYNTHESIS_EVENT_DATA_OFFSET).isEqualTo(24);
  }

  @Test
  void synthesisParamsMatchesTheCStructLayout() {
    assertThat(DengjenLayouts.SYNTHESIS_PARAMS.byteSize()).isEqualTo(40);
    assertThat(DengjenLayouts.SYNTHESIS_PARAMS_STRUCT_SIZE_OFFSET).isZero();
    assertThat(DengjenLayouts.SYNTHESIS_PARAMS_MODE_OFFSET).isEqualTo(4);
    assertThat(DengjenLayouts.SYNTHESIS_PARAMS_RATE_OFFSET).isEqualTo(8);
    assertThat(DengjenLayouts.SYNTHESIS_PARAMS_VOLUME_OFFSET).isEqualTo(9);
    assertThat(DengjenLayouts.SYNTHESIS_PARAMS_PITCH_OFFSET).isEqualTo(10);
    assertThat(DengjenLayouts.SYNTHESIS_PARAMS_APPENDED_SILENCE_MS_OFFSET).isEqualTo(12);
    assertThat(DengjenLayouts.SYNTHESIS_PARAMS_CALLBACK_OFFSET).isEqualTo(16);
    assertThat(DengjenLayouts.SYNTHESIS_PARAMS_NONBLOCKING_OFFSET).isEqualTo(24);
    assertThat(DengjenLayouts.SYNTHESIS_PARAMS_USER_DATA_OFFSET).isEqualTo(32);
  }

  @Test
  void expectedAbiVersionMatchesTheCHeader() throws IOException {
    String header = Files.readString(Path.of("../../crates/frontends/capi/libdengjen.h"));
    var matcher = Pattern.compile("#define DENGJEN_ABI_VERSION (\\d+)").matcher(header);
    assertThat(matcher.find()).isTrue();
    assertThat(matcher.group(1)).isEqualTo(String.valueOf(DengjenLib.EXPECTED_ABI_VERSION));
  }
}
