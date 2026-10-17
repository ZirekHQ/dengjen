#ifndef LIBDENGJEN_STT_H
#define LIBDENGJEN_STT_H

/* Generated with cbindgen:0.29.4 */

#include <stdarg.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>

#define STT_ABI_VERSION 1

#define INVALID_UTF8_SEQUENCE 20

#define NULL_POINTER 22

#define INVALID_CONFIGURATION 24

#define MISSING_MODEL_FILE 40

#define UNREADABLE_MODEL_FILE 41

#define CHECKSUM_MISMATCH 42

#define MODEL_LOAD_FAILED 43

#define INVALID_AUDIO 44

#define DECODE_FAILED 45

typedef struct DengjenRecognizer DengjenRecognizer;

typedef struct SttConfig {
  uint32_t struct_size;
  int32_t num_threads;
} SttConfig;

typedef int32_t ErrorCode;
#define ErrorCode_SUCCESS 0
#define ErrorCode_PANIC -1
#define ErrorCode_INVALID_HANDLE -1000

typedef struct ExternError {
  ErrorCode code;
  char *message;
} ExternError;

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

uint32_t libdengjenSttAbiVersion(void);

struct DengjenRecognizer *libdengjenSttLoad(const char *model_dir,
                                            const struct SttConfig *config,
                                            const char *expected_sha256,
                                            struct ExternError *out_error);

char *libdengjenSttRecognize(const struct DengjenRecognizer *recognizer,
                             const float *samples,
                             size_t len,
                             int32_t sample_rate,
                             struct ExternError *out_error);

void libdengjenSttFreeString(char *string_ptr);

void libdengjenSttUnload(struct DengjenRecognizer *recognizer);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* LIBDENGJEN_STT_H */
