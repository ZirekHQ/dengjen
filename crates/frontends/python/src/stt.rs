use crate::DengjenException;
use dengjen_stt::{verify_model, Recognizer as Engine, RecognizerConfig, SttError as EngineError};
use pyo3::create_exception;
use pyo3::prelude::*;
use pyo3::types::PyTuple;
use std::path::Path;
use std::sync::{Arc, Mutex};

create_exception!(
    pydengjen,
    SttError,
    DengjenException,
    "Raised by the speech recognizer; `.code` carries the libdengjen-stt error code."
);

const INVALID_CONFIGURATION: i32 = 24;
const INVALID_HANDLE: i32 = -1000;
const PANIC: i32 = -1;

fn code_of(error: &EngineError) -> i32 {
    match error {
        EngineError::MissingModelFile(_) => 40,
        EngineError::Unreadable(_) => 41,
        EngineError::ChecksumMismatch(_) => 42,
        EngineError::ModelLoad(_) => 43,
        EngineError::InvalidAudio(_) => 44,
        EngineError::Decode => 45,
    }
}

fn stt_error(code: i32, message: impl Into<String>) -> PyErr {
    Python::attach(|py| {
        let err = SttError::new_err(message.into());
        // `.code` is best effort: the exception message already carries the failure.
        let _ = err.value(py).setattr("code", code);
        err
    })
}

fn engine_error(error: &EngineError) -> PyErr {
    stt_error(code_of(error), error.to_string())
}

/// Offline speech recognizer backed by libdengjen-stt.
#[pyclass(weakref, module = "pydengjen", frozen)]
pub struct Recognizer {
    engine: Mutex<Option<Arc<Engine>>>,
}

#[pymethods]
impl Recognizer {
    #[new]
    #[pyo3(signature = (model_dir, sha256, num_threads = 2))]
    fn new(py: Python<'_>, model_dir: &str, sha256: &str, num_threads: i32) -> PyResult<Self> {
        if num_threads < 1 {
            return Err(stt_error(
                INVALID_CONFIGURATION,
                format!("num_threads is {num_threads}, but it must be at least 1"),
            ));
        }
        let (dir, sha) = (model_dir.to_owned(), sha256.to_owned());
        let engine = py
            .detach(move || {
                verify_model(Path::new(&dir), &sha)?;
                Engine::new(Path::new(&dir), RecognizerConfig { num_threads })
            })
            .map_err(|e| engine_error(&e))?;
        Ok(Self {
            engine: Mutex::new(Some(Arc::new(engine))),
        })
    }

    /// Transcribes mono float samples in [-1, 1] recorded at `sample_rate`.
    fn recognize(&self, py: Python<'_>, samples: Vec<f32>, sample_rate: i32) -> PyResult<String> {
        // The lock covers only the clone: holding it across `detach` would block a second Python
        // thread on the mutex while it holds the GIL the first thread needs back.
        let engine = self
            .engine
            .lock()
            .map_err(|_| stt_error(PANIC, "recognizer lock poisoned"))?
            .clone()
            .ok_or_else(|| stt_error(INVALID_HANDLE, "recognizer is closed"))?;
        py.detach(|| engine.recognize(&samples, sample_rate))
            .map_err(|e| engine_error(&e))
    }

    /// Releases the model once running recognitions finish. Safe to call more than once.
    fn close(&self) {
        if let Ok(mut guard) = self.engine.lock() {
            guard.take();
        }
    }

    fn __enter__(slf: Py<Self>) -> Py<Self> {
        slf
    }

    #[pyo3(signature = (*_args))]
    fn __exit__(&self, _args: &Bound<'_, PyTuple>) {
        self.close();
    }
}
