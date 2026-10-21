#include <exception>

extern "C" {
void dengjen_shim_record_error(const char* message);
int dengjen_shim_has_error();

const void* __real_SherpaOnnxCreateOfflineRecognizer(const void* config);
const void* __real_SherpaOnnxCreateOfflineStream(const void* recognizer);
void __real_SherpaOnnxAcceptWaveformOffline(const void* stream, int sample_rate,
                                            const float* samples, int n);
void __real_SherpaOnnxDecodeOfflineStream(const void* recognizer, const void* stream);
const char* __real_SherpaOnnxGetOfflineStreamResultAsJson(const void* stream);
}

namespace {

// After a caught exception the thread skips later sherpa calls until the error is taken, so a
// null stream from a failed call is never dereferenced.
template <typename R, typename F>
R guarded(R failure, F&& call) {
  if (dengjen_shim_has_error()) return failure;
  try {
    return call();
  } catch (const std::exception& e) {
    dengjen_shim_record_error(e.what());
  } catch (...) {
    dengjen_shim_record_error("unknown C++ exception");
  }
  return failure;
}

}  // namespace

extern "C" {

const void* __wrap_SherpaOnnxCreateOfflineRecognizer(const void* config) {
  return guarded<const void*>(nullptr, [&] { return __real_SherpaOnnxCreateOfflineRecognizer(config); });
}

const void* __wrap_SherpaOnnxCreateOfflineStream(const void* recognizer) {
  return guarded<const void*>(nullptr, [&] { return __real_SherpaOnnxCreateOfflineStream(recognizer); });
}

void __wrap_SherpaOnnxAcceptWaveformOffline(const void* stream, int sample_rate,
                                            const float* samples, int n) {
  guarded<int>(0, [&] {
    __real_SherpaOnnxAcceptWaveformOffline(stream, sample_rate, samples, n);
    return 0;
  });
}

void __wrap_SherpaOnnxDecodeOfflineStream(const void* recognizer, const void* stream) {
  guarded<int>(0, [&] {
    __real_SherpaOnnxDecodeOfflineStream(recognizer, stream);
    return 0;
  });
}

const char* __wrap_SherpaOnnxGetOfflineStreamResultAsJson(const void* stream) {
  return guarded<const char*>(nullptr, [&] { return __real_SherpaOnnxGetOfflineStreamResultAsJson(stream); });
}

}  // extern "C"
