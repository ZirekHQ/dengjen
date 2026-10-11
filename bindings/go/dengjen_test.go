package dengjen

import (
	"errors"
	"os"
	"sync"
	"testing"
	"time"
)

func TestRegisterCallbackIssuesDistinctTokens(t *testing.T) {
	a := registerCallback(func(SynthesisEvent) bool { return true })
	b := registerCallback(func(SynthesisEvent) bool { return true })
	defer releaseCallback(a)
	defer releaseCallback(b)
	if a == b {
		t.Fatal("concurrent registrations share one token")
	}
}

func TestInvokeCallbackRecoversPanicAsStop(t *testing.T) {
	got := invokeCallback(func(SynthesisEvent) bool { panic("boom") }, SynthesisEvent{})
	if got {
		t.Fatal("panicking callback must request stop")
	}
}

func TestLoadVoiceReportsAnErrorForAMissingConfigPath(t *testing.T) {
	v, err := LoadVoice("/nonexistent-dengjen-go-binding-test.json")
	if v != nil {
		t.Fatalf("expected a nil voice for a missing config path, got non-nil")
	}
	if err == nil {
		t.Fatalf("expected an error for a missing config path, got nil")
	}
	t.Logf("got expected error: %v", err)
}

func syntheticPiperConfigPath(t *testing.T) string {
	t.Helper()
	dir := t.TempDir()
	modelSrc := "../../crates/dengjen/models/piper/tests/fixtures/synthetic_piper_batch.onnx"
	modelDst := dir + "/model.onnx"
	data, err := os.ReadFile(modelSrc)
	if err != nil {
		t.Fatalf("failed to read fixture %s: %v", modelSrc, err)
	}
	if err := os.WriteFile(modelDst, data, 0o644); err != nil {
		t.Fatalf("failed to copy fixture: %v", err)
	}
	configJSON := `{
		"key": null,
		"language": {"code": "en-US"},
		"audio": {"sample_rate": 22050, "quality": null},
		"num_speakers": 1,
		"speaker_id_map": {"default": 0},
		"streaming": false,
		"espeak": {"voice": "en-us"},
		"inference": {"noise_scale": 0.667, "length_scale": 1.0, "noise_w": 0.8},
		"num_symbols": 8,
		"phoneme_map": {},
		"phoneme_id_map": {"^": [1], "$": [2], "_": [3], "t": [4]},
		"phoneme_type": "text",
		"hop_length": 256
	}`
	configPath := dir + "/model.onnx.json"
	if err := os.WriteFile(configPath, []byte(configJSON), 0o644); err != nil {
		t.Fatalf("failed to write config: %v", err)
	}
	return configPath
}

func TestLoadVoiceAudioInfoAndClose(t *testing.T) {
	v, err := LoadVoice(syntheticPiperConfigPath(t))
	if err != nil {
		t.Fatalf("LoadVoice failed: %v", err)
	}
	info, err := v.AudioInfo()
	if err != nil {
		t.Fatalf("AudioInfo failed: %v", err)
	}
	if info.SampleRate != 22050 {
		t.Errorf("expected SampleRate 22050, got %d", info.SampleRate)
	}
	if info.NumChannels != 1 {
		t.Errorf("expected NumChannels 1, got %d", info.NumChannels)
	}
	if err := v.Close(); err != nil {
		t.Fatalf("Close failed: %v", err)
	}
	if err := v.Close(); err != nil {
		t.Fatalf("second Close failed: %v", err)
	}
}

func TestClosedVoiceMethodsReportErrVoiceClosed(t *testing.T) {
	v, err := LoadVoice(syntheticPiperConfigPath(t))
	if err != nil {
		t.Fatalf("LoadVoice failed: %v", err)
	}
	if err := v.Close(); err != nil {
		t.Fatalf("Close failed: %v", err)
	}
	_, err = v.AudioInfo()
	if !errors.Is(err, ErrVoiceClosed) {
		t.Fatalf("expected errors.Is(err, ErrVoiceClosed), got %v", err)
	}
}

func TestFFIErrorFallsBackToCodeWhenMessageIsEmpty(t *testing.T) {
	err := &FFIError{Code: 42}
	want := "libdengjen error 42"
	if got := err.Error(); got != want {
		t.Fatalf("Error() = %q, want %q", got, want)
	}
}

func TestSynthConfigRoundTrip(t *testing.T) {
	v, err := LoadVoice(syntheticPiperConfigPath(t))
	if err != nil {
		t.Fatalf("LoadVoice failed: %v", err)
	}
	defer v.Close()

	cfg, err := v.PiperDefaultSynthConfig()
	if err != nil {
		t.Fatalf("PiperDefaultSynthConfig failed: %v", err)
	}
	if cfg.NoiseScale != 0.667 {
		t.Errorf("expected default NoiseScale 0.667, got %v", cfg.NoiseScale)
	}

	cfg.LengthScale = 2.0
	if err := v.SetPiperSynthConfig(cfg); err != nil {
		t.Fatalf("SetPiperSynthConfig failed: %v", err)
	}

	if err := v.SetSynthesisParameter("noise_scale", 0.5); err != nil {
		t.Fatalf("SetSynthesisParameter failed: %v", err)
	}
	value, ok, err := v.SynthesisParameter("noise_scale")
	if err != nil {
		t.Fatalf("SynthesisParameter failed: %v", err)
	}
	if !ok {
		t.Fatalf("expected SynthesisParameter to report the key was found")
	}
	if value != 0.5 {
		t.Errorf("expected noise_scale 0.5 after SetSynthesisParameter, got %v", value)
	}

	_, ok, err = v.SynthesisParameter("no_such_key")
	if err != nil {
		t.Fatalf("SynthesisParameter for an unset key returned an error: %v", err)
	}
	if ok {
		t.Errorf("expected ok=false for a key that was never set")
	}
}

func TestSpeakToFileWritesAWavFile(t *testing.T) {
	v, err := LoadVoice(syntheticPiperConfigPath(t))
	if err != nil {
		t.Fatalf("LoadVoice failed: %v", err)
	}
	defer v.Close()

	outPath := t.TempDir() + "/output.wav"
	params := SynthesisParams{Mode: SynthModeLazy, Rate: 10, Volume: 100, Pitch: 50}
	wrote, err := v.SpeakToFile("Test.", params, outPath)
	if err != nil {
		t.Fatalf("SpeakToFile failed: %v", err)
	}
	if !wrote {
		t.Fatalf("expected SpeakToFile to report success")
	}

	data, err := os.ReadFile(outPath)
	if err != nil {
		t.Fatalf("expected an output WAV file: %v", err)
	}

	const expectedLen = 44 + 8000*2
	if len(data) != expectedLen {
		t.Errorf("expected a %d-byte WAV file, got %d bytes", expectedLen, len(data))
	}
}

func TestSpeakLazyModeDeliversSpeechThenFinished(t *testing.T) {
	v, err := LoadVoice(syntheticPiperConfigPath(t))
	if err != nil {
		t.Fatalf("LoadVoice failed: %v", err)
	}
	defer v.Close()

	var events []EventType
	var totalBytes int
	params := SynthesisParams{Mode: SynthModeLazy, Rate: 10, Volume: 100, Pitch: 50}
	err = v.Speak("Test.", params, func(e SynthesisEvent) bool {
		events = append(events, e.Type)
		totalBytes += len(e.Data)
		if e.Err != nil {
			t.Errorf("unexpected event error: %v", e.Err)
		}
		return true
	})
	if err != nil {
		t.Fatalf("Speak failed: %v", err)
	}
	if len(events) == 0 || events[len(events)-1] != EventFinished {
		t.Fatalf("expected the last event to be EventFinished, got %v", events)
	}
	if totalBytes != 8000*2 {
		t.Errorf("expected 16000 total PCM bytes (8000 i16 samples), got %d", totalBytes)
	}
}

func TestSpeakOnEventReturningFalseStopsEarly(t *testing.T) {
	v, err := LoadVoice(syntheticPiperConfigPath(t))
	if err != nil {
		t.Fatalf("LoadVoice failed: %v", err)
	}
	defer v.Close()

	calls := 0
	params := SynthesisParams{Mode: SynthModeLazy, Rate: 10, Volume: 100, Pitch: 50}
	err = v.Speak("Test.", params, func(e SynthesisEvent) bool {
		calls++
		return false
	})
	if err != nil {
		t.Fatalf("Speak failed: %v", err)
	}
	if calls != 1 {
		t.Errorf("expected exactly 1 callback invocation after returning false, got %d", calls)
	}
}

func TestSpeakOnEventReturningFalseReleasesCallbackData(t *testing.T) {
	v, err := LoadVoice(syntheticPiperConfigPath(t))
	if err != nil {
		t.Fatalf("LoadVoice failed: %v", err)
	}
	defer v.Close()

	released := false
	prevHook := testCallbackDataReleased
	testCallbackDataReleased = func() { released = true }
	defer func() { testCallbackDataReleased = prevHook }()

	params := SynthesisParams{Mode: SynthModeLazy, Rate: 10, Volume: 100, Pitch: 50}
	err = v.Speak("Test.", params, func(e SynthesisEvent) bool {
		return false
	})
	if err != nil {
		t.Fatalf("Speak failed: %v", err)
	}

	if !released {
		t.Fatal("the callbackData for an early-stopped Speak call was never " +
			"released (testCallbackDataReleased hook never fired), leaking onEvent's closure")
	}
}

func TestCancelOnAVoiceWithNoActiveRealtimeStreamIsANoop(t *testing.T) {
	v, err := LoadVoice(syntheticPiperConfigPath(t))
	if err != nil {
		t.Fatalf("LoadVoice failed: %v", err)
	}
	defer v.Close()

	if err := v.Cancel(); err != nil {
		t.Fatalf("Cancel on an idle voice failed: %v", err)
	}
}

func TestSpeakRealtimeModeThenCancelDoesNotPanicOrDeadlock(t *testing.T) {
	v, err := LoadVoice(syntheticPiperConfigPath(t))
	if err != nil {
		t.Fatalf("LoadVoice failed: %v", err)
	}
	defer v.Close()

	events := make(chan SynthesisEvent, 64)
	done := make(chan error, 1)
	params := SynthesisParams{Mode: SynthModeRealtime, Rate: 10, Volume: 100, Pitch: 50, Nonblocking: true}
	err = v.Speak("Test.", params, func(e SynthesisEvent) bool {
		events <- e
		return true
	})
	if err != nil {
		t.Fatalf("Speak (nonblocking realtime) failed: %v", err)
	}
	go func() { done <- v.Cancel() }()

	select {
	case err := <-done:
		if err != nil {
			t.Fatalf("Cancel failed: %v", err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("Cancel did not return within 5s — possible deadlock")
	}

	deadline := time.After(5 * time.Second)
	for {
		select {
		case e := <-events:
			if e.Type == EventFinished || e.Type == EventError {
				return
			}
		case <-deadline:
			t.Fatal("no terminal event delivered within 5s after Cancel")
		}
	}
}

func TestFFIErrorCodeMapsEveryHeaderCode(t *testing.T) {
	cases := map[int32]ErrorCode{
		-1:    ErrorCodePanic,
		-1000: ErrorCodeInvalidHandle,
		16:    ErrorCodeInvalidSynthesisMode,
		17:    ErrorCodeFailedToLoadResource,
		18:    ErrorCodePhonemizationError,
		19:    ErrorCodeOperationError,
		20:    ErrorCodeInvalidUTF8Sequence,
		21:    ErrorCodeUnknownError,
		22:    ErrorCodeNullPointer,
		23:    ErrorCodeInferenceError,
		24:    ErrorCodeInvalidConfiguration,
		25:    ErrorCodeUnsupportedOperation,
	}
	for raw, want := range cases {
		if got := (&FFIError{Code: raw}).ErrorCode(); got != want || int32(got) != raw {
			t.Errorf("code %d mapped to %d, want %d", raw, got, want)
		}
	}
}

func TestFFIErrorCodeFallsBackToUnknown(t *testing.T) {
	if got := (&FFIError{Code: 999}).ErrorCode(); got != ErrorCodeUnknownError {
		t.Fatalf("got %d, want ErrorCodeUnknownError", got)
	}
	if got := ErrVoiceClosed.ErrorCode(); got != ErrorCodeUnknownError {
		t.Fatalf("ErrVoiceClosed mapped to %d, want ErrorCodeUnknownError", got)
	}
}

func TestConcurrentSpeakDeliversEachCallbackItsOwnEvents(t *testing.T) {
	v, err := LoadVoice(syntheticPiperConfigPath(t))
	if err != nil {
		t.Fatalf("LoadVoice failed: %v", err)
	}
	defer v.Close()

	const workers = 8
	params := SynthesisParams{Mode: SynthModeLazy, Rate: 10, Volume: 100, Pitch: 50}
	var wg sync.WaitGroup
	totals := make([]int, workers)
	finished := make([]int, workers)
	errs := make([]error, workers)
	for i := 0; i < workers; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			errs[i] = v.Speak("Test.", params, func(e SynthesisEvent) bool {
				totals[i] += len(e.Data)
				if e.Type == EventFinished {
					finished[i]++
				}
				return true
			})
		}(i)
	}
	wg.Wait()

	for i := 0; i < workers; i++ {
		if errs[i] != nil {
			t.Fatalf("worker %d Speak failed: %v", i, errs[i])
		}
		if totals[i] != 8000*2 || finished[i] != 1 {
			t.Errorf("worker %d got %d bytes and %d finished events, want 16000 and 1",
				i, totals[i], finished[i])
		}
	}
}

func TestSpeakWithInvalidModeSurfacesTheErrorAndReleasesCallback(t *testing.T) {
	v, err := LoadVoice(syntheticPiperConfigPath(t))
	if err != nil {
		t.Fatalf("LoadVoice failed: %v", err)
	}
	defer v.Close()

	var mu sync.Mutex
	released := 0
	prevHook := testCallbackDataReleased
	testCallbackDataReleased = func() { mu.Lock(); released++; mu.Unlock() }
	defer func() { testCallbackDataReleased = prevHook }()

	var eventErr error
	params := SynthesisParams{Mode: 99, Rate: 10, Volume: 100, Pitch: 50}
	err = v.Speak("Test.", params, func(e SynthesisEvent) bool {
		if e.Type == EventError {
			eventErr = e.Err
		}
		return true
	})
	surfaced := err
	if surfaced == nil {
		surfaced = eventErr
	}
	var ffi *FFIError
	if !errors.As(surfaced, &ffi) {
		t.Fatalf("expected an *FFIError from Speak or an error event, got %v", surfaced)
	}
	if ffi.ErrorCode() != ErrorCodeInvalidSynthesisMode {
		t.Errorf("expected ErrorCodeInvalidSynthesisMode, got %d", ffi.Code)
	}
	mu.Lock()
	defer mu.Unlock()
	if released != 1 {
		t.Errorf("expected the callback to be released exactly once, got %d", released)
	}
}
