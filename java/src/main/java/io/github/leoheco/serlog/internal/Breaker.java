package io.github.leoheco.serlog.internal;

import io.github.leoheco.serlog.Status.Health;
import io.github.leoheco.serlog.alarm.Incident.Phase;
import java.time.Instant;

/// A slot's health, free of I/O so the shared vectors can drive it; times are seconds on a monotonic clock.
public final class Breaker {
    /// How a failure is told.
    public enum Tell {
        REPORT,
        SILENT,
        /// Riding in another incident: the failure is not told, its recovery is.
        FOLDED
    }

    private static final double LONGEST = 300;

    private Health health = Health.UP;
    private Instant since = Instant.now();
    private long skipped;
    private String error;
    private final double base;
    private double gap;
    private double next;
    private Double told;
    private long openedAt;

    public Breaker(double probe, double now) {
        this.base = this.gap = probe;
        this.next = now;
    }

    public synchronized Health health() {
        return health;
    }

    public synchronized Instant since() {
        return since;
    }

    public synchronized long skipped() {
        return skipped;
    }

    /// Records skipped since the current failure began.
    public synchronized long skippedNow() {
        return skipped - openedAt;
    }

    public synchronized String error() {
        return error;
    }

    /// Whether a write may go ahead; turns a due `down` into `probing`, and counts a skip otherwise.
    public synchronized boolean admit(double now) {
        if (health == Health.UP) return true;
        if (health == Health.DOWN && now >= next) {
            health = Health.PROBING;
            return true;
        }
        skipped++;
        return false;
    }

    /// Whether the sink is left out at `now`, without counting anything.
    public synchronized boolean resting(double now) {
        return health == Health.PROBING || health == Health.DOWN && now < next;
    }

    /// Seconds until the next probe.
    public synchronized double wait(double now) {
        return Math.max(next - now, 0);
    }

    /// A write succeeded; `resolved` when the failure had been told.
    public synchronized Phase passed() {
        if (health == Health.UP) return null;
        health = Health.UP;
        gap = base;
        since = Instant.now();
        error = null;
        var was = told;
        told = null;
        return was == null ? null : Phase.RESOLVED;
    }

    /// A write failed; `opened` or `reminder` when that is to be told.
    public synchronized Phase failed(double now, String error, double remind, Tell tell) {
        this.error = error;
        if (health == Health.UP) {
            health = Health.DOWN;
            gap = base;
            next = now + gap;
            since = Instant.now();
            openedAt = skipped;
            if (tell == Tell.SILENT) return null;
            told = now;
            return tell == Tell.REPORT ? Phase.OPENED : null;
        }
        if (health == Health.PROBING) {
            health = Health.DOWN;
            gap = Math.min(gap * 2, LONGEST);
            next = now + gap;
            if (tell == Tell.REPORT && told != null && now - told >= remind) {
                told = now;
                return Phase.REMINDER;
            }
        }
        return null;
    }
}
