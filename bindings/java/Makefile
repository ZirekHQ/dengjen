CARGO_MANIFEST := ../../Cargo.toml
# CI sets ort-dylib to test the library as published; the default links onnxruntime statically.
CARGO_FEATURES ?=

.PHONY: native test coverage fmt-check clean verify-native-jar

native:
	cargo build --release --manifest-path $(CARGO_MANIFEST) -p libdengjen $(if $(CARGO_FEATURES),--features $(CARGO_FEATURES))

test: native
	./gradlew test integrationTest e2e

# Proves the classifier-jar native-loading path (see NativeLibraryLoader) against the current
# platform's real native library, without any Maven Central publishing infrastructure.
verify-native-jar: native
	./gradlew nativeLoaderTest

# jacocoTestReport depends on test/integrationTest/e2e (see build.gradle.kts) and merges all
# three into the XML sonar.coverage.jacoco.xmlReportPaths (see ../../sonar-project.properties)
# reads directly.
coverage: native
	./gradlew jacocoTestReport

fmt-check:
	./gradlew spotlessCheck

clean:
	./gradlew clean
