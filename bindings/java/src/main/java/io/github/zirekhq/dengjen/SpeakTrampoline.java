package io.github.zirekhq.dengjen;

import java.lang.foreign.Arena;
import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.Linker;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;
import java.lang.invoke.MethodHandle;
import java.lang.invoke.MethodHandles;
import java.lang.invoke.MethodType;
import java.util.Arrays;
import java.util.Map;
import java.util.Objects;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.atomic.AtomicLong;
import java.util.concurrent.atomic.AtomicReference;

final class SpeakTrampoline {

  static final AtomicReference<Runnable> testCallReleased = new AtomicReference<>();

  private static final Map<Long, SpeakTrampoline> REGISTRY = new ConcurrentHashMap<>();

  private static final AtomicLong NEXT_ID = new AtomicLong(1);

  private static final MemorySegment STUB;

  static {
    MethodHandle invokeHandle;
    try {
      invokeHandle =
          MethodHandles.lookup()
              .findStatic(
                  SpeakTrampoline.class,
                  "invoke",
                  MethodType.methodType(byte.class, MemorySegment.class, MemorySegment.class));
    } catch (ReflectiveOperationException e) {
      throw new ExceptionInInitializerError(e);
    }
    STUB =
        Linker.nativeLinker()
            .upcallStub(
                invokeHandle,
                FunctionDescriptor.of(
                    ValueLayout.JAVA_BYTE, DengjenLayouts.SYNTHESIS_EVENT, ValueLayout.ADDRESS),
                Arena.global());
  }

  private final SynthesisEventHandler handler;
  private final long id;

  private SpeakTrampoline(SynthesisEventHandler handler, long id) {
    this.handler = handler;
    this.id = id;
  }

  static SpeakTrampoline create(SynthesisEventHandler handler) {
    SpeakTrampoline trampoline = new SpeakTrampoline(handler, NEXT_ID.getAndIncrement());
    REGISTRY.put(trampoline.id, trampoline);
    return trampoline;
  }

  static MemorySegment stubPointer() {
    return STUB;
  }

  MemorySegment userData() {
    return MemorySegment.ofAddress(id);
  }

  void release() {
    if (REGISTRY.remove(id) != null) {
      Runnable hook = testCallReleased.get();
      if (hook != null) {
        hook.run();
      }
    }
  }

  static byte invoke(MemorySegment eventSegment, MemorySegment userData) {
    SpeakTrampoline trampoline = null;
    boolean freed = false;
    try {
      trampoline = REGISTRY.get(userData.address());
      DecodedEvent decoded = decodeEvent(eventSegment);

      freeEventOrThrow(eventSegment);
      freed = true;

      if (trampoline == null) {
        return 1;
      }

      boolean wantsMore =
          trampoline.handler.onEvent(
              new SynthesisEvent(decoded.type(), decoded.data(), decoded.error()));

      boolean terminal = decoded.type() == EventType.FINISHED || decoded.type() == EventType.ERROR;
      if (terminal || !wantsMore) {
        trampoline.release();
      }
      return (byte) (wantsMore ? 0 : 1);
    } catch (
        @SuppressWarnings("java:S1181")
        Throwable t) {

      if (!freed) {
        tryFreeEvent(eventSegment);
      }
      if (trampoline != null) {
        trampoline.release();
      }
      return 1;
    }
  }

  record DecodedEvent(EventType type, byte[] data, DengjenException error) {
    @Override
    public boolean equals(Object obj) {
      if (this == obj) {
        return true;
      }
      if (!(obj instanceof DecodedEvent other)) {
        return false;
      }
      return type == other.type
          && Arrays.equals(data, other.data)
          && Objects.equals(error, other.error);
    }

    @Override
    public int hashCode() {
      return Objects.hash(type, Arrays.hashCode(data), error);
    }

    @Override
    public String toString() {
      return "DecodedEvent[type="
          + type
          + ", data="
          + Arrays.toString(data)
          + ", error="
          + error
          + "]";
    }
  }

  private static DecodedEvent decodeEvent(MemorySegment eventSegment) {
    int eventType =
        eventSegment.get(ValueLayout.JAVA_INT, DengjenLayouts.SYNTHESIS_EVENT_TYPE_OFFSET);
    long len = eventSegment.get(ValueLayout.JAVA_LONG, DengjenLayouts.SYNTHESIS_EVENT_LEN_OFFSET);
    MemorySegment dataPtr =
        eventSegment.get(ValueLayout.ADDRESS, DengjenLayouts.SYNTHESIS_EVENT_DATA_OFFSET);
    MemorySegment errorPtr =
        eventSegment.get(ValueLayout.ADDRESS, DengjenLayouts.SYNTHESIS_EVENT_ERROR_PTR_OFFSET);

    EventType type = EventType.fromValue(eventType);
    byte[] data =
        (type == EventType.SPEECH && len > 0)
            ? dataPtr.reinterpret(len).toArray(ValueLayout.JAVA_BYTE)
            : new byte[0];
    DengjenException error = null;
    if (type == EventType.ERROR && !errorPtr.equals(MemorySegment.NULL)) {
      MemorySegment err = errorPtr.reinterpret(DengjenLayouts.EXTERN_ERROR.byteSize());
      int code = err.get(ValueLayout.JAVA_INT, DengjenLayouts.EXTERN_ERROR_CODE_OFFSET);
      MemorySegment messagePtr =
          err.get(ValueLayout.ADDRESS, DengjenLayouts.EXTERN_ERROR_MESSAGE_OFFSET);
      error = new DengjenException(ErrorCode.fromCode(code), ErrorChecks.readMessage(messagePtr));
    }
    return new DecodedEvent(type, data, error);
  }

  private static void freeEventOrThrow(MemorySegment eventSegment) {
    try {
      DengjenLib.FREE_SYNTHESIS_EVENT.invokeExact(eventSegment);
    } catch (Throwable t) {
      throw new IllegalStateException("libdengjenFreeSynthesisEvent downcall failed", t);
    }
  }

  // Best-effort: called only while invoke() is already unwinding from another failure, so a
  // downcall failure here would only replace one unrecoverable-for-this-event problem with
  // another.
  private static void tryFreeEvent(MemorySegment eventSegment) {
    try {
      DengjenLib.FREE_SYNTHESIS_EVENT.invokeExact(eventSegment);
    } catch (Throwable freeFailure) {
      // Ignored on purpose; see the note above.
    }
  }
}
