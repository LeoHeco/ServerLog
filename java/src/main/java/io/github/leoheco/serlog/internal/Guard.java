package io.github.leoheco.serlog.internal;

import java.util.concurrent.Callable;

/// Marks code running inside a sink or a processor, so records it makes are dropped instead of looping back.
public final class Guard {
    /// Something that counts suppressed records.
    @FunctionalInterface
    public interface Counts {
        void suppressed();
    }

    private static final Counts NONE = () -> {};
    private static final ScopedValue<Counts> INSIDE = ScopedValue.newInstance();

    private Guard() {}

    /// Runs `fn` as serlog's own code, counting what it logs against `counts`.
    public static <T> T call(Counts counts, Callable<T> fn) throws Exception {
        return ScopedValue.where(INSIDE, counts == null ? NONE : counts).call(fn::call);
    }

    /// Runs `fn` as serlog's own code.
    public static void run(Counts counts, Runnable fn) {
        ScopedValue.where(INSIDE, counts == null ? NONE : counts).run(fn);
    }

    /// Counts a record made inside serlog's own code, which must then be dropped.
    public static boolean suppress() {
        if (!INSIDE.isBound()) return false;
        INSIDE.get().suppressed();
        return true;
    }
}
