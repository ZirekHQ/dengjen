#include <cstddef>
#include <cstring>
#include <stdexcept>
#include <string>

namespace {
thread_local std::string last_error;
}

extern "C" {

void dengjen_shim_record_error(const char* message) { last_error = message ? message : "unknown"; }

int dengjen_shim_has_error() { return last_error.empty() ? 0 : 1; }

void dengjen_shim_clear_error() { last_error.clear(); }

size_t dengjen_shim_take_error(char* buf, size_t cap) {
  if (cap == 0) return 0;
  size_t n = last_error.size() < cap - 1 ? last_error.size() : cap - 1;
  std::memcpy(buf, last_error.data(), n);
  buf[n] = '\0';
  last_error.clear();
  return n;
}

int dengjen_shim_selftest_throw() {
  try {
    throw std::runtime_error("selftest exception");
  } catch (const std::exception& e) {
    dengjen_shim_record_error(e.what());
    return 1;
  }
}

}  // extern "C"
