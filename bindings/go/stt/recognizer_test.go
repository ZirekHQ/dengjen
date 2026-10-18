package stt

import (
	"errors"
	"os"
	"testing"
)

const zeroSum = "0000000000000000000000000000000000000000000000000000000000000000"

func TestLoadUnknownModelDirIsAnUnreadableModelError(t *testing.T) {
	_, err := Load("/definitely/not/here", zeroSum, 1)
	if !errors.Is(err, ErrUnreadableModelFile) {
		t.Fatalf("want ErrUnreadableModelFile, got %v", err)
	}
}

func TestLoadRejectsZeroThreads(t *testing.T) {
	_, err := Load("/definitely/not/here", zeroSum, 0)
	if !errors.Is(err, ErrInvalidConfiguration) {
		t.Fatalf("want ErrInvalidConfiguration, got %v", err)
	}
}

func TestClosedRecognizerAndDoubleClose(t *testing.T) {
	r := &Recognizer{}
	if err := r.Close(); err != nil {
		t.Fatalf("close of an empty recognizer: %v", err)
	}
	if err := r.Close(); err != nil {
		t.Fatalf("second close: %v", err)
	}
	if _, err := r.Recognize(make([]float32, 1600), 16000); !errors.Is(err, ErrClosed) {
		t.Fatalf("want ErrClosed, got %v", err)
	}
}

func loadModel(t *testing.T) *Recognizer {
	t.Helper()
	dir, sum := os.Getenv("DENGJEN_STT_MODEL_DIR"), os.Getenv("DENGJEN_STT_MODEL_SHA256")
	if dir == "" || sum == "" {
		t.Skip("needs DENGJEN_STT_MODEL_DIR and DENGJEN_STT_MODEL_SHA256")
	}
	r, err := Load(dir, sum, 1)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { r.Close() })
	return r
}

func TestInvalidAudioIsTyped(t *testing.T) {
	r := loadModel(t)
	if _, err := r.Recognize(nil, 16000); !errors.Is(err, ErrInvalidAudio) {
		t.Fatalf("empty audio: want ErrInvalidAudio, got %v", err)
	}
	if _, err := r.Recognize(make([]float32, 1600), 0); !errors.Is(err, ErrInvalidAudio) {
		t.Fatalf("zero rate: want ErrInvalidAudio, got %v", err)
	}
}

func TestSilenceRecognizesWithoutError(t *testing.T) {
	r := loadModel(t)
	if _, err := r.Recognize(make([]float32, 16000), 16000); err != nil {
		t.Fatalf("silence: %v", err)
	}
}
