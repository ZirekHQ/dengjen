/* Generated with cbindgen:0.29.4 */

#include <stdarg.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>

#define PIPER_SYNTH_CONFIG_NO_SPEAKER UINT32_MAX

#define DENGJEN_ABI_VERSION 1

#define INVALID_SYNTHESIS_MODE 16

#define FAILED_TO_LOAD_RESOURCE 17

#define PHONEMIZATION_ERROR 18

#define OPERATION_ERROR 19

#define INVALID_UTF8_SEQUENCE 20

#define UNKNOWN_ERROR 21

#define NULL_POINTER 22

#define INFERENCE_ERROR 23

#define INVALID_CONFIGURATION 24

#define UNSUPPORTED_OPERATION 25

#define SYNTH_EVENT_SPEECH 0

#define SYNTH_EVENT_FINISHED 1

#define SYNTH_EVENT_ERROR 2

#define SYNTH_MODE_LAZY 0

#define SYNTH_MODE_PARALLEL 1

#define SYNTH_MODE_REALTIME 2

#define SYNTH_MODE_BATCHED 3

typedef struct DengjenVoice DengjenVoice;

typedef struct PiperSynthConfig {
  uint32_t speaker;
  float length_scale;
  float noise_scale;
  float noise_w;
} PiperSynthConfig;

typedef int32_t ErrorCode;
#define ErrorCode_SUCCESS 0
#define ErrorCode_PANIC -1
#define ErrorCode_INVALID_HANDLE -1000

typedef struct ExternError {
  ErrorCode code;
  char *message;
} ExternError;

typedef struct SynthesisEvent {
  int32_t event_type;
  struct ExternError *error_ptr;
  int64_t len;
  uint8_t *data;
} SynthesisEvent;

typedef const char *FfiStr;

typedef struct AudioInfo {
  uint32_t sample_rate;
  uint32_t num_channels;
  uint32_t sample_width;
} AudioInfo;

typedef struct SynthesisParams {
  uint32_t struct_size;
  int32_t mode;
  uint8_t rate;
  uint8_t volume;
  uint8_t pitch;
  uint32_t appended_silence_ms;
  uint8_t (*callback)(struct SynthesisEvent, void*);
  uint8_t nonblocking;
  void *user_data;
} SynthesisParams;

typedef uint8_t (*SpeechSynthesisCallback)(struct SynthesisEvent, void*);

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

uint32_t libdengjenAbiVersion(void);

void libdengjenFreeString(char *string_ptr);

void libdengjenFreePiperSynthConfig(struct PiperSynthConfig *synth_config);

void libdengjenFreeSynthesisEvent(struct SynthesisEvent event);

struct DengjenVoice *libdengjenLoadVoiceFromConfigPath(FfiStr config_path_ptr,
                                                       struct ExternError *out_error);

void libdengjenUnloadDengjenVoice(struct DengjenVoice *voice_ptr);

void libdengjenGetAudioInfo(struct DengjenVoice *voice_ptr,
                            struct AudioInfo *audio_info_ptr,
                            struct ExternError *out_error);

struct PiperSynthConfig *libdengjenGetPiperDefaultSynthConfig(struct DengjenVoice *voice_ptr,
                                                              struct ExternError *out_error);

void libdengjenSetPiperSynthConfig(struct DengjenVoice *voice_ptr,
                                   struct PiperSynthConfig synth_config,
                                   struct ExternError *out_error);

void libdengjenSetSynthesisParameter(struct DengjenVoice *voice_ptr,
                                     FfiStr key_ptr,
                                     float value,
                                     struct ExternError *out_error);

bool libdengjenGetSynthesisParameter(struct DengjenVoice *voice_ptr,
                                     FfiStr key_ptr,
                                     float *out_value_ptr,
                                     struct ExternError *out_error);

void libdengjenSpeak(struct DengjenVoice *voice_ptr,
                     FfiStr text_ptr,
                     const struct SynthesisParams *params_ptr,
                     struct ExternError *out_error);

void libdengjenCancel(struct DengjenVoice *voice_ptr, struct ExternError *out_error);

uint8_t libdengjenSpeakToFile(struct DengjenVoice *voice_ptr,
                              FfiStr text_ptr,
                              const struct SynthesisParams *params_ptr,
                              FfiStr out_filename_ptr,
                              struct ExternError *out_error);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus
