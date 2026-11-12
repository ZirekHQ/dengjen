/*
 * Library-wide contracts.
 * - Every call taking an ExternError* needs a zero-initialized struct. On failure code != 0 and
 *   message must be released with libdengjenAppFreeString.
 * - Strings returned by the library are released with libdengjenAppFreeString, sentence arrays
 *   with libdengjenAppFreeSentences.
 * - Every function is safe to call from any thread.
 */

#ifndef LIBDENGJEN_APP_H
#define LIBDENGJEN_APP_H

/* Generated with cbindgen:0.29.4 */

#include <stdarg.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>

/**
 * Bumped on any incompatible change to the exported functions or structs.
 */
#define APP_ABI_VERSION 1

#define APP_ERR_INVALID_UTF8 20

#define APP_ERR_UNKNOWN 21

#define APP_ERR_NULL_POINTER 22

#define APP_ERR_IO 70

#define APP_ERR_UNSUPPORTED_FORMAT 80

#define APP_ERR_NOT_TEXT 81

/**
 * One sentence: `[byte_start, byte_end)` in the UTF-8 text passed in, and
 * `[utf16_start, utf16_end)` in the same text as UTF-16.
 */
typedef struct AppSentence {
  size_t byte_start;
  size_t byte_end;
  size_t utf16_start;
  size_t utf16_end;
} AppSentence;

/**
 * A wrapper around error codes, which is represented identically to an i32 on the other side of
 * the FFI. Essentially exists to check that we don't accidentally reuse success/panic codes for
 * other things.
 */
typedef int32_t ErrorCode;
/**
 * The ErrorCode used for success.
 */
#define ErrorCode_SUCCESS 0
/**
 * The ErrorCode used for panics. It's unlikely you need to ever use this.
 */
#define ErrorCode_PANIC -1
/**
 * The ErrorCode used for handle map errors.
 */
#define ErrorCode_INVALID_HANDLE -1000

/**
 * Represents an error that occured within rust, storing both an error code, and additional data
 * that may be used by the caller.
 *
 * Misuse of this type can cause numerous issues, so please read the entire documentation before
 * usage.
 *
 * ## Rationale
 *
 * This library encourages a pattern of taking a `&mut ExternError` as the final parameter for
 * functions exposed over the FFI. This is an "out parameter" which we use to write error/success
 * information that occurred during the function's execution.
 *
 * To be clear, this means instances of `ExternError` will be created on the other side of the FFI,
 * and passed (by mutable reference) into Rust.
 *
 * While this pattern is not particularly ergonomic in Rust (although hopefully this library
 * helps!), it offers two main benefits over something more ergonomic (which might be `Result`
 * shaped).
 *
 * 1. It avoids defining a large number of `Result`-shaped types in the FFI consumer, as would
 *    be required with something like an `struct ExternResult<T> { ok: *mut T, err:... }`
 *
 * 2. It offers additional type safety over `struct ExternResult { ok: *mut c_void, err:... }`,
 *    which helps avoid memory safety errors. It also can offer better performance for returning
 *    primitives and repr(C) structs (no boxing required).
 *
 * It also is less tricky to use properly than giving consumers a `get_last_error()` function, or
 * similar.
 *
 * ## Caveats
 *
 * Note that the order of the fields is `code` (an i32) then `message` (a `*mut c_char`), getting
 * this wrong on the other side of the FFI will cause memory corruption and crashes.
 *
 * The fields are public largely for documentation purposes, but you should use
 * [`ExternError::new_error`] or [`ExternError::success`] to create these.
 *
 * ## Layout/fields
 *
 * This struct's field are not `pub` (mostly so that we can soundly implement `Send`, but also so
 * that we can verify rust users are constructing them appropriately), the fields, their types, and
 * their order are *very much* a part of the public API of this type. Consumers on the other side
 * of the FFI will need to know its layout.
 *
 * If this were a C struct, it would look like
 *
 * ```c,no_run
 * struct ExternError {
 *     int32_t code;
 *     char *message; // note: nullable
 * };
 * ```
 *
 * In rust, there are two fields, in this order: `code: ErrorCode`, and `message: *mut c_char`.
 * Note that ErrorCode is a `#[repr(transparent)]` wrapper around an `i32`, so the first property
 * is equivalent to an `i32`.
 *
 * #### The `code` field.
 *
 * This is the error code, 0 represents success, all other values represent failure. If the `code`
 * field is nonzero, there should always be a message, and if it's zero, the message will always be
 * null.
 *
 * #### The `message` field.
 *
 * This is a null-terminated C string containing some amount of additional information about the
 * error. If the `code` property is nonzero, there should always be an error message. Otherwise,
 * this should will be null.
 *
 * This string (when not null) is allocated on the rust heap (using this crate's
 * [`rust_string_to_c`]), and must be freed on it as well. Critically, if there are multiple rust
 * packages using being used in the same application, it *must be freed on the same heap that
 * allocated it*, or you will corrupt both heaps.
 *
 * Typically, this object is managed on the other side of the FFI (on the "FFI consumer"), which
 * means you must expose a function to release the resources of `message` which can be done easily
 * using the [`define_string_destructor!`] macro provided by this crate.
 *
 * If, for some reason, you need to release the resources directly, you may call
 * `ExternError::release()`. Note that you probably do not need to do this, and it's
 * intentional that this is not called automatically by implementing `drop`.
 *
 * ## Example
 *
 * ```rust,no_run
 * use ffi_support::{ExternError, ErrorCode};
 *
 * #[derive(Debug)]
 * pub enum MyError {
 *     IllegalFoo(String),
 *     InvalidBar(i64),
 *     // ...
 * }
 *
 * // Putting these in a module is obviously optional, but it allows documentation, and helps
 * // avoid accidental reuse.
 * pub mod error_codes {
 *     // note: -1 and 0 are reserved by ffi_support
 *     pub const ILLEGAL_FOO: i32 = 1;
 *     pub const INVALID_BAR: i32 = 2;
 *     // ...
 * }
 *
 * fn get_code(e: &MyError) -> ErrorCode {
 *     match e {
 *         MyError::IllegalFoo(_) => ErrorCode::new(error_codes::ILLEGAL_FOO),
 *         MyError::InvalidBar(_) => ErrorCode::new(error_codes::INVALID_BAR),
 *         // ...
 *     }
 * }
 *
 * impl From<MyError> for ExternError {
 *     fn from(e: MyError) -> ExternError {
 *         ExternError::new_error(get_code(&e), format!("{:?}", e))
 *     }
 * }
 * ```
 */
typedef struct ExternError {
  ErrorCode code;
  char *message;
} ExternError;

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

/**
 * The ABI version this library was built with.
 */
uint32_t libdengjenAppAbiVersion(void);

/**
 * Splits `text` into sentences and returns them as an array of `*out_len` items, or null when
 * there are none or on error. Free a non-null result with `libdengjenAppFreeSentences`.
 *
 * # Safety
 * `text` must be null or a NUL-terminated string; `out_len` must be a valid pointer.
 */
struct AppSentence *libdengjenAppSentences(const char *text,
                                           size_t *out_len,
                                           struct ExternError *out_error);

/**
 * # Safety
 * `sentences` and `len` must be a pointer and length returned together by
 * `libdengjenAppSentences`, passed here at most once. Null is ignored.
 */
void libdengjenAppFreeSentences(struct AppSentence *sentences, size_t len);

/**
 * Reads a `.txt`, `.md`, `.markdown`, `.htm`, `.html` or `.xhtml` file and returns its plain
 * text, or null on error. Free a non-null result with `libdengjenAppFreeString`.
 *
 * # Safety
 * `path` must be null or a NUL-terminated UTF-8 string.
 */
char *libdengjenAppTextFromFile(const char *path, struct ExternError *out_error);

/**
 * # Safety
 * `string` must be null or a string returned by this library, passed here at most once.
 */
void libdengjenAppFreeString(char *string);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* LIBDENGJEN_APP_H */
