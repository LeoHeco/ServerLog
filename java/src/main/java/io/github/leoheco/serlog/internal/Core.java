package io.github.leoheco.serlog.internal;

import io.github.leoheco.serlog.Entry;
import io.github.leoheco.serlog.LogRecord;
import io.github.leoheco.serlog.SerlogException;
import io.github.leoheco.serlog.Sink;
import io.github.leoheco.serlog.Status;
import io.github.leoheco.serlog.Theme;
import io.github.leoheco.serlog.alarm.Alarm;
import io.github.leoheco.serlog.alarm.Incident;
import io.github.leoheco.serlog.alarm.Incident.Phase;
import io.github.leoheco.serlog.internal.Breaker.Tell;
import io.github.leoheco.serlog.layout.Layout;
import java.util.List;
import java.util.Map;
import java.util.concurrent.atomic.AtomicLong;
import java.util.concurrent.locks.ReentrantLock;

/// The sink, its look, its policy and its health, shared by the callers and the queue. Calls into the sink are
/// serialised, so a sink need not be thread-safe.
public final class Core implements Guard.Counts {
    final String name;
    final String kind;
    final Sink sink;
    final Layout layout;
    final Theme theme;
    final boolean panic;
    final Alarm alarm;
    final int retries;
    final Breaker breaker;
    final AtomicLong dropped = new AtomicLong();
    private final AtomicLong suppressed = new AtomicLong();
    private final ReentrantLock lock = new ReentrantLock();

    Core(String name, Sink sink, Layout layout, Theme theme, Object policy, int retries, double probe) {
        this.name = name;
        this.sink = sink;
        this.kind = sink.kind();
        this.layout = layout;
        this.theme = theme;
        this.panic = "panic".equals(policy);
        this.alarm = policy instanceof Alarm a ? a : null;
        this.retries = retries;
        this.breaker = new Breaker(probe, now());
    }

    static double now() {
        return System.nanoTime() / 1e9;
    }

    private static void sleep(double secs) {
        try {
            Thread.sleep((long) (secs * 1000));
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
        }
    }

    @Override
    public void suppressed() {
        suppressed.incrementAndGet();
    }

    boolean up() {
        return breaker.health() == Status.Health.UP;
    }

    private Tell tell() {
        return alarm != null ? Tell.REPORT : Tell.SILENT;
    }

    Entry entry(LogRecord rec, Map<Layout, Map<Theme, String>> cache) {
        return new Entry(rec, layout, theme, cache);
    }

    private void batch(List<Entry> entries) throws Exception {
        lock.lock();
        try {
            if (entries.size() == 1) sink.emit(entries.getFirst());
            else sink.emitBatch(entries);
        } finally {
            lock.unlock();
        }
    }

    /// Writes in the caller, retrying without pause; `panic` throws, the other policies count or tell.
    void write(Entry entry) {
        if (!panic && !up() && !breaker.admit(now())) return;
        for (var n = 0; ; n++) {
            try {
                Guard.call(this, () -> {
                    batch(List.of(entry));
                    return null;
                });
                break;
            } catch (Exception e) {
                if (SerlogException.retryable(e) && n < retries) continue;
                if (panic) throw new SerlogException("sink `" + name + "` failed: " + SerlogException.message(e), e);
                failed(e, tell());
                return;
            }
        }
        passed();
    }

    private void attempt(List<Entry> entries, boolean pause) throws Exception {
        for (var n = 0; ; n++) {
            try {
                batch(entries);
                return;
            } catch (Exception e) {
                if (!SerlogException.retryable(e) || n >= retries) throw e;
                if (pause) sleep(1 << Math.min(n, 5));
            }
        }
    }

    /// Writes a batch from the queue, waiting out failures until it goes through or is refused.
    void drain(List<LogRecord> buf) {
        while (!buf.isEmpty()) {
            if (!up()) {
                sleep(breaker.wait(now()));
                breaker.admit(now());
            }
            var probing = !up();
            try {
                attempt(buf.stream().map(r -> entry(r, null)).toList(), !probing);
                passed();
                buf.clear();
            } catch (Exception e) {
                if (panic) AlarmTask.crash("sink `" + name + "` failed: " + SerlogException.message(e));
                failed(e, tell());
                if (!SerlogException.retryable(e)) {
                    dropped.addAndGet(buf.size());
                    buf.clear();
                }
            }
        }
    }

    /// Writes an alarm record straight to the sink; a failure rides in the incident being delivered.
    void deliver(LogRecord rec) throws Exception {
        if (!up() && !breaker.admit(now())) throw new SerlogException("down");
        try {
            Guard.call(this, () -> {
                batch(List.of(entry(rec, null)));
                lock.lock();
                try {
                    sink.flush();
                } finally {
                    lock.unlock();
                }
                return null;
            });
        } catch (Exception e) {
            failed(e, Tell.FOLDED);
            throw e;
        }
        passed();
    }

    /// Flushes the sink in the caller.
    void flush() {
        if (!up()) return;
        try {
            Guard.call(this, () -> {
                lock.lock();
                try {
                    sink.flush();
                } finally {
                    lock.unlock();
                }
                return null;
            });
        } catch (Exception e) {
            failed(e, tell());
        }
    }

    /// Closes the sink; a failure here has nobody left to tell.
    void close() {
        try {
            Guard.call(this, () -> {
                lock.lock();
                try {
                    sink.close();
                } finally {
                    lock.unlock();
                }
                return null;
            });
        } catch (Exception _) {
        }
    }

    boolean resting() {
        return !up() && breaker.resting(now());
    }

    void passed() {
        if (up()) return;
        var phase = breaker.passed();
        if (phase != null) notify(phase);
    }

    void failed(Throwable e, Tell tell) {
        var remind = alarm != null ? Hub.resolve(alarm).remind().toNanos() / 1e9 : Double.POSITIVE_INFINITY;
        var phase = breaker.failed(now(), SerlogException.message(e), remind, tell);
        if (phase != null) notify(phase);
    }

    private void notify(Phase phase) {
        if (alarm == null) return;
        var error = breaker.error();
        AlarmTask.post(
                alarm,
                Incident.of(
                        name,
                        kind,
                        phase,
                        error == null ? "" : error,
                        breaker.skippedNow(),
                        dropped.get(),
                        breaker.since()));
    }

    Status status() {
        return new Status(
                name,
                kind,
                breaker.health(),
                breaker.skipped(),
                dropped.get(),
                suppressed.get(),
                breaker.since(),
                breaker.error());
    }
}
