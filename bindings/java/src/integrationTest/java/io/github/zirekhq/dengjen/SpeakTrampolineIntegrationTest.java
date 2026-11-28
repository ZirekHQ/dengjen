package io.github.zirekhq.dengjen;

import static org.assertj.core.api.Assertions.assertThat;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;
import org.junit.jupiter.api.Test;

class SpeakTrampolineIntegrationTest {
  @Test
  void invokeStopsTheStreamWhenNoTrampolineIsRegisteredForTheEventsUserData() {
    try (Arena arena = Arena.ofConfined()) {
      MemorySegment event = arena.allocate(DengjenLayouts.SYNTHESIS_EVENT);

      event.set(ValueLayout.JAVA_INT, DengjenLayouts.SYNTHESIS_EVENT_TYPE_OFFSET, 1);
      event.set(ValueLayout.JAVA_LONG, DengjenLayouts.SYNTHESIS_EVENT_LEN_OFFSET, 0L);

      event.set(
          ValueLayout.ADDRESS,
          DengjenLayouts.SYNTHESIS_EVENT_DATA_OFFSET,
          MemorySegment.ofAddress(1));
      event.set(
          ValueLayout.ADDRESS, DengjenLayouts.SYNTHESIS_EVENT_ERROR_PTR_OFFSET, MemorySegment.NULL);

      MemorySegment unregisteredUserData = MemorySegment.ofAddress(Long.MAX_VALUE);

      byte result = SpeakTrampoline.invoke(event, unregisteredUserData);

      assertThat(result)
          .as("no registered trampoline for this event -- the stream must be told to stop")
          .isEqualTo((byte) 1);
    }
  }

  @Test
  void invokeTreatsAZeroLengthSpeechEventAsCarryingNoPayload() {
    try (Arena arena = Arena.ofConfined()) {
      MemorySegment event = arena.allocate(DengjenLayouts.SYNTHESIS_EVENT);

      event.set(ValueLayout.JAVA_INT, DengjenLayouts.SYNTHESIS_EVENT_TYPE_OFFSET, 0);
      event.set(ValueLayout.JAVA_LONG, DengjenLayouts.SYNTHESIS_EVENT_LEN_OFFSET, 0L);
      event.set(
          ValueLayout.ADDRESS,
          DengjenLayouts.SYNTHESIS_EVENT_DATA_OFFSET,
          MemorySegment.ofAddress(1));
      event.set(
          ValueLayout.ADDRESS, DengjenLayouts.SYNTHESIS_EVENT_ERROR_PTR_OFFSET, MemorySegment.NULL);

      SynthesisEvent[] received = new SynthesisEvent[1];
      SpeakTrampoline trampoline =
          SpeakTrampoline.create(event2 -> {
            received[0] = event2;
            return true;
          });
      try {
        byte result = SpeakTrampoline.invoke(event, trampoline.userData());
        assertThat(result)
            .as("SPEECH with wantsMore=true keeps the stream going")
            .isEqualTo((byte) 0);
        assertThat(received[0].data()).isEmpty();
      } finally {
        trampoline.release();
      }
    }
  }

  @Test
  void invokeTreatsAnErrorEventWithANullErrorPointerAsCarryingNoError() {
    try (Arena arena = Arena.ofConfined()) {
      MemorySegment event = arena.allocate(DengjenLayouts.SYNTHESIS_EVENT);

      event.set(ValueLayout.JAVA_INT, DengjenLayouts.SYNTHESIS_EVENT_TYPE_OFFSET, 2);
      event.set(ValueLayout.JAVA_LONG, DengjenLayouts.SYNTHESIS_EVENT_LEN_OFFSET, 0L);
      event.set(
          ValueLayout.ADDRESS,
          DengjenLayouts.SYNTHESIS_EVENT_DATA_OFFSET,
          MemorySegment.ofAddress(1));
      event.set(
          ValueLayout.ADDRESS, DengjenLayouts.SYNTHESIS_EVENT_ERROR_PTR_OFFSET, MemorySegment.NULL);

      SynthesisEvent[] received = new SynthesisEvent[1];
      SpeakTrampoline trampoline =
          SpeakTrampoline.create(event2 -> {
            received[0] = event2;
            return true;
          });
      boolean[] released = {false};
      Runnable prevHook = SpeakTrampoline.testCallReleased.get();
      SpeakTrampoline.testCallReleased.set(() -> released[0] = true);
      try {
        byte result = SpeakTrampoline.invoke(event, trampoline.userData());

        assertThat(result)
            .as("ERROR with wantsMore=true keeps the stream going")
            .isEqualTo((byte) 0);
        assertThat(received[0].error()).isNull();
        assertThat(released[0])
            .as("ERROR is terminal -- invoke() must release on its own")
            .isTrue();
      } finally {
        SpeakTrampoline.testCallReleased.set(prevHook);
        trampoline.release();
      }
    }
  }

  @Test
  void invokeSurvivesAnUndersizedEventSegmentAndStillStopsTheStream() {
    try (Arena arena = Arena.ofConfined()) {

      MemorySegment tooSmall = arena.allocate(1);
      MemorySegment unregisteredUserData = MemorySegment.ofAddress(Long.MAX_VALUE - 1);

      byte result = SpeakTrampoline.invoke(tooSmall, unregisteredUserData);

      assertThat(result)
          .as("a malformed event must degrade to \"stop the stream\", never propagate or crash")
          .isEqualTo((byte) 1);
    }
  }
}
