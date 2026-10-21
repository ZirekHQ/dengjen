use std::any::Any;
use std::panic::{catch_unwind, AssertUnwindSafe};

pub const PANIC_CODE: i32 = -1;
pub const OPERATION_ERROR: i32 = 19;

#[derive(Debug, PartialEq, Eq)]
pub struct Failure {
    pub code: i32,
    pub message: String,
}

pub fn guarded<T>(f: impl FnOnce() -> Result<T, Failure>) -> Result<T, Failure> {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|payload| {
        Err(Failure {
            code: PANIC_CODE,
            message: panic_message(payload.as_ref()),
        })
    })
}

pub fn jni_failure(e: jni::errors::Error) -> Failure {
    Failure {
        code: OPERATION_ERROR,
        message: e.to_string(),
    }
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_ok_through() {
        assert_eq!(guarded(|| Ok(7)), Ok(7));
    }

    #[test]
    fn passes_failure_through() {
        let f = Failure {
            code: 23,
            message: "x".into(),
        };
        assert_eq!(
            guarded::<()>(|| Err(f)),
            Err(Failure {
                code: 23,
                message: "x".into()
            })
        );
    }

    #[test]
    fn converts_str_panic() {
        let r = guarded::<()>(|| panic!("boom"));
        assert_eq!(
            r,
            Err(Failure {
                code: PANIC_CODE,
                message: "boom".into()
            })
        );
    }

    #[test]
    fn converts_string_panic() {
        let r = guarded::<()>(|| panic!("{}", String::from("fmt")));
        assert_eq!(
            r,
            Err(Failure {
                code: PANIC_CODE,
                message: "fmt".into()
            })
        );
    }
}
