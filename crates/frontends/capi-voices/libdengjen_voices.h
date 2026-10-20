/*
 * Library-wide contracts.
 * - Every call taking an ExternError* needs a zero-initialized struct. On failure code != 0 and
 *   message must be released with libdengjenVoicesFreeString.
 * - Strings returned by the library are released with libdengjenVoicesFreeString.
 * - Do not call libdengjenVoicesClose while another call on the same handle is in flight.
 * - Install and fetch calls block; call them from a worker thread. The progress callback runs on
 *   the calling thread.
 * - libdengjenVoicesCancel affects only installs already running on that handle, is safe from any
 *   thread (including the progress callback), and is not remembered for installs that start later.
 *   It does not interrupt an install after its last data chunk has been read.
 */

#ifndef LIBDENGJEN_VOICES_H
#define LIBDENGJEN_VOICES_H

/* Generated with cbindgen:0.29.4 */

#include <stdarg.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>

#define VOICES_ABI_VERSION 1

#define VOICES_ERR_INVALID_UTF8 20

#define VOICES_ERR_NULL_POINTER 22

#define VOICES_ERR_INVALID_CONFIGURATION 24

#define VOICES_ERR_NETWORK 60

#define VOICES_ERR_CATALOGUE_PARSE 61

#define VOICES_ERR_UNKNOWN_VOICE 62

#define VOICES_ERR_INVALID_KEY 63

#define VOICES_ERR_UNSAFE_PATH 64

#define VOICES_ERR_DIGEST_MISMATCH 65

#define VOICES_ERR_SIZE_MISMATCH 66

#define VOICES_ERR_CANCELLED 67

#define VOICES_ERR_BUSY 68

#define VOICES_ERR_NOT_INSTALLED 69

#define VOICES_ERR_IO 70

#define VOICES_ERR_NO_CATALOGUE 71

typedef struct DengjenVoices DengjenVoices;

typedef struct VoicesConfig {
  uint32_t struct_size;
  const char *voices_dir;
  const char *model_type;
  const char *catalogue_url;
  const char *files_base_url;
} VoicesConfig;

typedef int32_t ErrorCode;
#define ErrorCode_SUCCESS 0
#define ErrorCode_PANIC -1
#define ErrorCode_INVALID_HANDLE -1000

typedef struct ExternError {
  ErrorCode code;
  char *message;
} ExternError;

typedef void (*VoicesProgressFn)(uint64_t done, uint64_t total, void *user_data);

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

uint32_t libdengjenVoicesAbiVersion(void);

struct DengjenVoices *libdengjenVoicesOpen(const struct VoicesConfig *config,
                                           struct ExternError *out_error);

int32_t libdengjenVoicesFetchCatalogue(const struct DengjenVoices *handle,
                                       struct ExternError *out_error);

int32_t libdengjenVoicesLoadCachedCatalogue(const struct DengjenVoices *handle,
                                            struct ExternError *out_error);

char *libdengjenVoicesListJson(const struct DengjenVoices *handle,
                               const char *language,
                               struct ExternError *out_error);

char *libdengjenVoicesInstalledJson(const struct DengjenVoices *handle,
                                    struct ExternError *out_error);

void libdengjenVoicesInstall(const struct DengjenVoices *handle,
                             const char *key,
                             VoicesProgressFn progress,
                             void *user_data,
                             struct ExternError *out_error);

void libdengjenVoicesCancel(const struct DengjenVoices *handle);

void libdengjenVoicesRemove(const struct DengjenVoices *handle,
                            const char *key,
                            struct ExternError *out_error);

void libdengjenVoicesFreeString(char *string_ptr);

void libdengjenVoicesClose(struct DengjenVoices *handle);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* LIBDENGJEN_VOICES_H */
