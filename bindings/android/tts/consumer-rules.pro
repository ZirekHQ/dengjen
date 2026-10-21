-keep class io.github.zirekhq.dengjen.android.Voice {
    native <methods>;
    static boolean dispatch(io.github.zirekhq.dengjen.android.SynthesisEventHandler, int, byte[], int, java.lang.String);
}
-keep interface io.github.zirekhq.dengjen.android.SynthesisEventHandler
-keep class io.github.zirekhq.dengjen.android.DengjenException { <init>(int, java.lang.String); }
