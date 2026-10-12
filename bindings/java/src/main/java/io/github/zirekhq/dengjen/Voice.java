package io.github.zirekhq.dengjen;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;
import java.lang.ref.Reference;
import java.util.Optional;
import java.util.concurrent.locks.ReentrantLock;

public final class Voice implements AutoCloseable {
  private final ReentrantLock lock = new ReentrantLock();
  private MemorySegment ptr;

  private Voice(MemorySegment ptr) {
    this.ptr = ptr;
  }

  public static Voice load(String configPath) {
    try (Arena arena = Arena.ofConfined()) {
      MemorySegment cPath = arena.allocateFrom(configPath);
      MemorySegment outError = arena.allocate(DengjenLayouts.EXTERN_ERROR);
      MemorySegment ptr;
      try {
        ptr = (MemorySegment) DengjenLib.LOAD_VOICE_FROM_CONFIG_PATH.invokeExact(cPath, outError);
      } catch (Throwable t) {
        throw new IllegalStateException("libdengjenLoadVoiceFromConfigPath downcall failed", t);
      }
      ErrorChecks.checkAndThrow(outError);
      return new Voice(ptr);
    }
  }

  @Override
  public void close() {
    lock.lock();
    try {
      if (ptr == null) {
        return;
      }
      MemorySegment closingPtr = ptr;
      ptr = null;
      try {
        DengjenLib.UNLOAD_DENGJEN_VOICE.invokeExact(closingPtr);
      } catch (Throwable t) {
        throw new IllegalStateException("libdengjenUnloadDengjenVoice downcall failed", t);
      }
    } finally {
      lock.unlock();
    }
  }

  private MemorySegment requireOpenPtr() {
    lock.lock();
    try {
      if (ptr == null) {
        throw new DengjenException(ErrorCode.INVALID_HANDLE, "voice is closed");
      }
      return ptr;
    } finally {
      lock.unlock();
    }
  }

  public AudioInfo getAudioInfo() {
    MemorySegment voicePtr = requireOpenPtr();
    try (Arena arena = Arena.ofConfined()) {
      MemorySegment cInfo = arena.allocate(DengjenLayouts.AUDIO_INFO);
      MemorySegment outError = arena.allocate(DengjenLayouts.EXTERN_ERROR);
      try {
        DengjenLib.GET_AUDIO_INFO.invokeExact(voicePtr, cInfo, outError);
      } catch (Throwable t) {
        throw new IllegalStateException("libdengjenGetAudioInfo downcall failed", t);
      }
      ErrorChecks.checkAndThrow(outError);
      return new AudioInfo(
          cInfo.get(ValueLayout.JAVA_INT, DengjenLayouts.AUDIO_INFO_SAMPLE_RATE_OFFSET),
          cInfo.get(ValueLayout.JAVA_INT, DengjenLayouts.AUDIO_INFO_NUM_CHANNELS_OFFSET),
          cInfo.get(ValueLayout.JAVA_INT, DengjenLayouts.AUDIO_INFO_SAMPLE_WIDTH_OFFSET));
    } finally {

      Reference.reachabilityFence(this);
    }
  }

  public PiperSynthConfig getDefaultSynthConfig() {
    MemorySegment voicePtr = requireOpenPtr();
    try (Arena arena = Arena.ofConfined()) {
      MemorySegment outError = arena.allocate(DengjenLayouts.EXTERN_ERROR);
      MemorySegment cfgPtr;
      try {
        cfgPtr =
            (MemorySegment)
                DengjenLib.GET_PIPER_DEFAULT_SYNTH_CONFIG.invokeExact(voicePtr, outError);
      } catch (Throwable t) {
        throw new IllegalStateException("libdengjenGetPiperDefaultSynthConfig downcall failed", t);
      }
      ErrorChecks.checkAndThrow(outError);
      MemorySegment cfg = cfgPtr.reinterpret(DengjenLayouts.PIPER_SYNTH_CONFIG.byteSize());
      PiperSynthConfig result =
          new PiperSynthConfig(
              cfg.get(ValueLayout.JAVA_INT, DengjenLayouts.PIPER_SYNTH_CONFIG_SPEAKER_OFFSET),
              cfg.get(
                  ValueLayout.JAVA_FLOAT, DengjenLayouts.PIPER_SYNTH_CONFIG_LENGTH_SCALE_OFFSET),
              cfg.get(ValueLayout.JAVA_FLOAT, DengjenLayouts.PIPER_SYNTH_CONFIG_NOISE_SCALE_OFFSET),
              cfg.get(ValueLayout.JAVA_FLOAT, DengjenLayouts.PIPER_SYNTH_CONFIG_NOISE_W_OFFSET));
      try {
        DengjenLib.FREE_PIPER_SYNTH_CONFIG.invokeExact(cfgPtr);
      } catch (Throwable t) {
        throw new IllegalStateException("libdengjenFreePiperSynthConfig downcall failed", t);
      }
      return result;
    } finally {
      Reference.reachabilityFence(this);
    }
  }

  public void setSynthConfig(PiperSynthConfig config) {
    MemorySegment voicePtr = requireOpenPtr();
    try (Arena arena = Arena.ofConfined()) {
      MemorySegment cfg = arena.allocate(DengjenLayouts.PIPER_SYNTH_CONFIG);
      cfg.set(
          ValueLayout.JAVA_INT, DengjenLayouts.PIPER_SYNTH_CONFIG_SPEAKER_OFFSET, config.speaker());
      cfg.set(
          ValueLayout.JAVA_FLOAT,
          DengjenLayouts.PIPER_SYNTH_CONFIG_LENGTH_SCALE_OFFSET,
          config.lengthScale());
      cfg.set(
          ValueLayout.JAVA_FLOAT,
          DengjenLayouts.PIPER_SYNTH_CONFIG_NOISE_SCALE_OFFSET,
          config.noiseScale());
      cfg.set(
          ValueLayout.JAVA_FLOAT,
          DengjenLayouts.PIPER_SYNTH_CONFIG_NOISE_W_OFFSET,
          config.noiseW());
      MemorySegment outError = arena.allocate(DengjenLayouts.EXTERN_ERROR);
      try {
        DengjenLib.SET_PIPER_SYNTH_CONFIG.invokeExact(voicePtr, cfg, outError);
      } catch (Throwable t) {
        throw new IllegalStateException("libdengjenSetPiperSynthConfig downcall failed", t);
      }
      ErrorChecks.checkAndThrow(outError);
    } finally {
      Reference.reachabilityFence(this);
    }
  }

  public void setSynthesisParameter(String key, float value) {
    MemorySegment voicePtr = requireOpenPtr();
    try (Arena arena = Arena.ofConfined()) {
      MemorySegment cKey = arena.allocateFrom(key);
      MemorySegment outError = arena.allocate(DengjenLayouts.EXTERN_ERROR);
      try {
        DengjenLib.SET_SYNTHESIS_PARAMETER.invokeExact(voicePtr, cKey, value, outError);
      } catch (Throwable t) {
        throw new IllegalStateException("libdengjenSetSynthesisParameter downcall failed", t);
      }
      ErrorChecks.checkAndThrow(outError);
    } finally {
      Reference.reachabilityFence(this);
    }
  }

  public Optional<Float> getSynthesisParameter(String key) {
    MemorySegment voicePtr = requireOpenPtr();
    try (Arena arena = Arena.ofConfined()) {
      MemorySegment cKey = arena.allocateFrom(key);
      MemorySegment outValue = arena.allocate(ValueLayout.JAVA_FLOAT);
      MemorySegment outError = arena.allocate(DengjenLayouts.EXTERN_ERROR);
      boolean found;
      try {
        found =
            (boolean)
                DengjenLib.GET_SYNTHESIS_PARAMETER.invokeExact(voicePtr, cKey, outValue, outError);
      } catch (Throwable t) {
        throw new IllegalStateException("libdengjenGetSynthesisParameter downcall failed", t);
      }
      ErrorChecks.checkAndThrow(outError);
      return found ? Optional.of(outValue.get(ValueLayout.JAVA_FLOAT, 0)) : Optional.empty();
    } finally {
      Reference.reachabilityFence(this);
    }
  }

  public boolean speakToFile(String text, SynthesisParams params, String outFilename) {
    MemorySegment voicePtr = requireOpenPtr();
    try (Arena arena = Arena.ofConfined()) {
      MemorySegment cText = arena.allocateFrom(text);
      MemorySegment cOutFilename = arena.allocateFrom(outFilename);
      MemorySegment cParams =
          SynthesisParamsMarshaller.allocate(arena, params, MemorySegment.NULL, MemorySegment.NULL);
      MemorySegment outError = arena.allocate(DengjenLayouts.EXTERN_ERROR);
      byte wrote;
      try {
        wrote =
            (byte)
                DengjenLib.SPEAK_TO_FILE.invokeExact(
                    voicePtr, cText, cParams, cOutFilename, outError);
      } catch (Throwable t) {
        throw new IllegalStateException("libdengjenSpeakToFile downcall failed", t);
      }
      ErrorChecks.checkAndThrow(outError);
      return wrote != 0;
    } finally {
      Reference.reachabilityFence(this);
    }
  }

  public void speak(String text, SynthesisParams params, SynthesisEventHandler handler) {
    MemorySegment voicePtr = requireOpenPtr();
    SpeakTrampoline trampoline = SpeakTrampoline.create(handler);
    try (Arena arena = Arena.ofConfined()) {
      MemorySegment cText = arena.allocateFrom(text);
      MemorySegment cParams =
          SynthesisParamsMarshaller.allocate(
              arena, params, SpeakTrampoline.stubPointer(), trampoline.userData());
      MemorySegment outError = arena.allocate(DengjenLayouts.EXTERN_ERROR);
      try {
        DengjenLib.SPEAK.invokeExact(voicePtr, cText, cParams, outError);
      } catch (Throwable t) {
        throw new IllegalStateException("libdengjenSpeak downcall failed", t);
      }
      ErrorChecks.checkAndThrow(outError);
    } catch (RuntimeException | Error t) {

      trampoline.release();
      throw t;
    } finally {
      Reference.reachabilityFence(this);
    }
  }

  public void cancel() {
    MemorySegment voicePtr = requireOpenPtr();
    try (Arena arena = Arena.ofConfined()) {
      MemorySegment outError = arena.allocate(DengjenLayouts.EXTERN_ERROR);
      try {
        DengjenLib.CANCEL.invokeExact(voicePtr, outError);
      } catch (Throwable t) {
        throw new IllegalStateException("libdengjenCancel downcall failed", t);
      }
      ErrorChecks.checkAndThrow(outError);
    } finally {
      Reference.reachabilityFence(this);
    }
  }
}
