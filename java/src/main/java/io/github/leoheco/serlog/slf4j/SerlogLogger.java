package io.github.leoheco.serlog.slf4j;

import io.github.leoheco.serlog.Field;
import io.github.leoheco.serlog.Level;
import io.github.leoheco.serlog.LogRecord;
import io.github.leoheco.serlog.internal.Hub;
import io.github.leoheco.serlog.internal.Make;
import java.time.Instant;
import java.util.ArrayList;
import java.util.List;
import org.slf4j.MDC;
import org.slf4j.Marker;
import org.slf4j.event.LoggingEvent;
import org.slf4j.helpers.LegacyAbstractLogger;
import org.slf4j.helpers.MessageFormatter;
import org.slf4j.spi.LoggingEventAware;

/// An SLF4J logger that makes serlog records: the MDC and key-value pairs become fields, markers become tags.
final class SerlogLogger extends LegacyAbstractLogger implements LoggingEventAware {
    SerlogLogger(String name) {
        this.name = name;
    }

    private static Level level(org.slf4j.event.Level l) {
        return switch (l) {
            case TRACE -> Level.TRACE;
            case DEBUG -> Level.DEBUG;
            case INFO -> Level.INFO;
            case WARN -> Level.WARN;
            case ERROR -> Level.ERROR;
        };
    }

    @Override
    public boolean isTraceEnabled() {
        return Hub.enabled(Level.TRACE);
    }

    @Override
    public boolean isDebugEnabled() {
        return Hub.enabled(Level.DEBUG);
    }

    @Override
    public boolean isInfoEnabled() {
        return Hub.enabled(Level.INFO);
    }

    @Override
    public boolean isWarnEnabled() {
        return Hub.enabled(Level.WARN);
    }

    @Override
    public boolean isErrorEnabled() {
        return Hub.enabled(Level.ERROR);
    }

    @Override
    protected String getFullyQualifiedCallerName() {
        return SerlogLogger.class.getName();
    }

    private static List<Field> mdc() {
        var ctx = MDC.getCopyOfContextMap();
        var out = new ArrayList<Field>();
        if (ctx != null) ctx.forEach((k, v) -> out.add(new Field(k, v)));
        return out;
    }

    private void make(
            Level lvl,
            List<Marker> markers,
            String msg,
            Object[] args,
            Throwable error,
            List<Field> extra,
            Instant time) {
        if (!Hub.enabled(lvl)) return;
        var fields = mdc();
        fields.addAll(extra);
        if (error != null) fields.add(new Field("error", error));
        var text = args == null || args.length == 0 ? msg : MessageFormatter.basicArrayFormat(msg, args);
        var rec = LogRecord.of(lvl, name, text == null ? "" : text, fields);
        if (markers != null && !markers.isEmpty())
            rec = rec.withTags(markers.stream().map(Marker::getName).toList());
        if (time != null) rec = rec.withTime(time);
        Hub.emit(Make.caller(rec));
    }

    @Override
    protected void handleNormalizedLoggingCall(
            org.slf4j.event.Level level, Marker marker, String msg, Object[] args, Throwable error) {
        make(level(level), marker == null ? null : List.of(marker), msg, args, error, List.of(), null);
    }

    @Override
    public void log(LoggingEvent e) {
        var kv = new ArrayList<Field>();
        if (e.getKeyValuePairs() != null) for (var p : e.getKeyValuePairs()) kv.add(new Field(p.key, p.value));
        var time = e.getTimeStamp() > 0 ? Instant.ofEpochMilli(e.getTimeStamp()) : null;
        make(level(e.getLevel()), e.getMarkers(), e.getMessage(), e.getArgumentArray(), e.getThrowable(), kv, time);
    }
}
