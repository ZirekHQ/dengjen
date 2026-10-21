package io.github.zirekhq.dengjen.android;

@FunctionalInterface
public interface SynthesisEventHandler {
  /** Return false to stop synthesis. An exception thrown here is swallowed and also stops it. Must return promptly. */
  boolean onEvent(SynthesisEvent event);
}
