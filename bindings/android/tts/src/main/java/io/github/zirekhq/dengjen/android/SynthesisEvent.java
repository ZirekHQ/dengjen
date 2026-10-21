package io.github.zirekhq.dengjen.android;

public record SynthesisEvent(EventType type, byte[] data, DengjenException error) {}
