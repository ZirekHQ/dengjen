CARGO_MANIFEST := ../../Cargo.toml
TARGET_DIR := ../../target/release
# sherpa-onnx's libraries sit in its prebuilt directory, which only exists once native-stt has run.
STT_LIB_PATH = $(abspath $(TARGET_DIR)):$(shell find $(abspath ../../target) -path '*sherpa-onnx-prebuilt/*/lib' -type d 2>/dev/null | paste -sd: -)
# The stt package links libdengjen-stt, which only test-stt builds.
PKGS := $(shell go list ./... | grep -v /stt)

.PHONY: native native-stt test test-stt test-race coverage vet fmt-check clean

native:
	cargo build --release --manifest-path $(CARGO_MANIFEST) -p libdengjen

native-stt:
	cargo build --release --manifest-path $(CARGO_MANIFEST) -p libdengjen-stt

# Needs DENGJEN_STT_MODEL_DIR and DENGJEN_STT_MODEL_SHA256 for the recognition cases.
# go test runs from the package directory, so the library paths must be absolute.
test-stt: native-stt
	LD_LIBRARY_PATH=$(STT_LIB_PATH):$$LD_LIBRARY_PATH \
	DYLD_LIBRARY_PATH=$(STT_LIB_PATH):$$DYLD_LIBRARY_PATH \
	go test ./stt/... -v

test: native
	LD_LIBRARY_PATH=$(TARGET_DIR):$$LD_LIBRARY_PATH \
	DYLD_LIBRARY_PATH=$(TARGET_DIR):$$DYLD_LIBRARY_PATH \
	go test $(PKGS) -v

# Writes coverage.out (gitignored) in the plain Go cover-profile format
# sonar.go.coverage.reportPaths (see ../../sonar-project.properties) reads directly.
coverage: native native-stt
	LD_LIBRARY_PATH=$(TARGET_DIR):$$LD_LIBRARY_PATH \
	DYLD_LIBRARY_PATH=$(TARGET_DIR):$$DYLD_LIBRARY_PATH \
	go test -coverprofile=coverage.out $(PKGS)
	LD_LIBRARY_PATH=$(STT_LIB_PATH):$$LD_LIBRARY_PATH \
	DYLD_LIBRARY_PATH=$(STT_LIB_PATH):$$DYLD_LIBRARY_PATH \
	go test -coverprofile=coverage-stt.out ./stt/...
	tail -n +2 coverage-stt.out >> coverage.out
	rm -f coverage-stt.out

test-race: native
	LD_LIBRARY_PATH=$(TARGET_DIR):$$LD_LIBRARY_PATH \
	DYLD_LIBRARY_PATH=$(TARGET_DIR):$$DYLD_LIBRARY_PATH \
	go test -race $(PKGS) -v

vet: native
	LD_LIBRARY_PATH=$(TARGET_DIR):$$LD_LIBRARY_PATH \
	DYLD_LIBRARY_PATH=$(TARGET_DIR):$$DYLD_LIBRARY_PATH \
	go vet $(PKGS)

fmt-check:
	@test -z "$$(gofmt -l .)" || (gofmt -l . && exit 1)

clean:
	go clean
