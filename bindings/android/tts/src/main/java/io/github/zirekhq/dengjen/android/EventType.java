package io.github.zirekhq.dengjen.android;

public enum EventType {
  SPEECH(0),
  FINISHED(1),
  ERROR(2);

  private final int value;

  EventType(int value) {
    this.value = value;
  }

  public int value() {
    return value;
  }

  public static EventType fromValue(int value) {
    for (EventType t : values()) {
      if (t.value == value) {
        return t;
      }
    }
    throw new IllegalArgumentException("unknown event type " + value);
  }
}
