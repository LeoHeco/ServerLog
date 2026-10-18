package io.github.leoheco.serlog.sink;

import io.github.leoheco.serlog.Compat.Output;
import io.github.leoheco.serlog.Entry;
import io.github.leoheco.serlog.Level;
import io.github.leoheco.serlog.SinkBase;
import io.github.leoheco.serlog.internal.Hub;
import java.io.PrintStream;
import java.util.List;
import java.util.Map;
import java.util.Optional;

/// Standard output or standard error, one line per entry.
public final class ConsoleSink extends SinkBase<ConsoleSink> {
    private Output stream;

    /// Prints where the compat profile says, standard output unless it says otherwise.
    public ConsoleSink() {}

    /// `stdout`, `stderr` or `auto`, which sends `error` and above to standard error.
    public ConsoleSink stream(Output stream) {
        this.stream = stream;
        return this;
    }

    private PrintStream out(Level level) {
        var s = stream != null ? stream : Hub.compat().output();
        return s == Output.STDERR || s == Output.AUTO && level.atLeast(Level.ERROR) ? System.err : System.out;
    }

    @Override
    public String kind() {
        return "console";
    }

    @Override
    public void emit(Entry entry) {
        var out = out(entry.record().level());
        out.print(entry.line() + "\n");
        out.flush();
    }

    @Override
    public void emitBatch(List<Entry> entries) {
        for (var out : List.of(System.out, System.err)) {
            var text = new StringBuilder();
            for (var e : entries)
                if (out(e.record().level()) == out) text.append(e.line()).append('\n');
            if (!text.isEmpty()) {
                out.print(text);
                out.flush();
            }
        }
    }

    @Override
    public boolean tty() {
        var c = System.console();
        return c != null && c.isTerminal();
    }

    @Override
    public Optional<Map<String, Object>> options() {
        return Optional.of(stream == null ? Map.of() : Map.of("stream", stream.id()));
    }
}
