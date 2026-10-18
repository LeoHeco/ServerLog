package io.github.leoheco.serlog.internal;

import io.github.leoheco.serlog.LogRecord;
import io.github.leoheco.serlog.SerlogException;
import io.github.leoheco.serlog.alarm.Alarm;
import io.github.leoheco.serlog.alarm.Incident;
import java.time.Instant;
import java.util.ArrayList;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;

/// Walking an alarm's tiers: the same rules in every language, checked by `spec/cases/alarm.json`.
public final class Walk {
    /// What the walk needs from the sinks around it.
    public interface Desk {
        List<String> logs();

        boolean resting(String name);

        void deliver(String name, LogRecord rec) throws Exception;
    }

    private Walk() {}

    /// Delivers `incident` through `alarm`'s tiers, adding every failed delivery to it; `true` when one sink took it.
    public static boolean sound(Alarm alarm, Incident incident, Desk desk) {
        var out = new LinkedHashSet<String>();
        out.add(incident.sink());
        for (var tier : alarm.route(incident)) {
            var names = new LinkedHashSet<String>();
            for (var n : tier)
                for (var m : n.equals("*") ? desk.logs() : List.of(n))
                    if (!out.contains(m) && !desk.resting(m)) names.add(m);
            if (names.isEmpty()) continue;
            var rec = alarm.describe(incident);
            var heard = false;
            var errors = new ArrayList<String>();
            try (var pool = Executors.newVirtualThreadPerTaskExecutor()) {
                var jobs = new ArrayList<Future<?>>();
                for (var n : names)
                    jobs.add(pool.submit(() -> {
                        desk.deliver(n, rec);
                        return null;
                    }));
                for (var job : jobs) {
                    try {
                        job.get();
                        errors.add(null);
                        heard = true;
                    } catch (ExecutionException e) {
                        errors.add(SerlogException.message(e.getCause()));
                    } catch (InterruptedException e) {
                        Thread.currentThread().interrupt();
                        errors.add("interrupted");
                    }
                }
            }
            var i = 0;
            for (var n : names) {
                var err = errors.get(i++);
                if (err != null) incident.failures().add(new Incident.Failure(n, err, Instant.now()));
                out.add(n);
            }
            if (heard) return true;
        }
        return false;
    }
}
