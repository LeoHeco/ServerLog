package io.github.leoheco.serlog.sink;

import io.github.leoheco.serlog.Entry;
import io.github.leoheco.serlog.Level;
import io.github.leoheco.serlog.SerlogException;
import io.github.leoheco.serlog.SinkBase;
import io.github.leoheco.serlog.internal.Units;
import io.github.leoheco.serlog.layout.Clock;
import java.io.IOException;
import java.io.OutputStream;
import java.nio.ByteBuffer;
import java.nio.channels.FileChannel;
import java.nio.charset.StandardCharsets;
import java.nio.file.FileAlreadyExistsException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardOpenOption;
import java.nio.file.attribute.FileTime;
import java.nio.file.attribute.PosixFilePermission;
import java.nio.file.attribute.PosixFilePermissions;
import java.time.Duration;
import java.time.Instant;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Comparator;
import java.util.EnumSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.Executor;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import java.util.stream.Collectors;
import java.util.zip.GZIPOutputStream;

/// A log file, rolled over by size or time, cleaned up by count, age or total size, and reopened when moved.
public final class FileSink extends SinkBase<FileSink> {
    /// When files roll over by time, on local time.
    public enum Rotate {
        MINUTE("%Y%m%d%H%M"),
        HOUR("%Y%m%d%H"),
        DAY("%Y%m%d"),
        WEEK("%G%V");

        private final String period;

        Rotate(String period) {
            this.period = period;
        }

        /// A period by its config name.
        public static Rotate parse(String name) {
            return valueOf(name.strip().toUpperCase(Locale.ROOT));
        }

        /// The config name.
        public String id() {
            return name().toLowerCase(Locale.ROOT);
        }
    }

    /// The default name of a rolled file.
    public static final String NAME = "{stem}.{time:%Y%m%d-%H%M%S}.{ext}";

    private static final Pattern HOLE = Pattern.compile("\\{([^}]*)}");
    private static final Executor BACKGROUND =
            r -> Thread.ofVirtual().name("serlog-file").start(r);

    private final Path path;
    private boolean append = true;
    private String perms;
    private boolean mkdir = true;
    private Long maxSize;
    private Rotate rotate;
    private Integer keep;
    private Duration maxAge;
    private Long maxTotal;
    private boolean gzip;
    private String rollName = NAME;
    private boolean reopen;
    private long buffer;
    private Duration flushEvery = Duration.ofSeconds(1);

    private FileChannel ch;
    private long size;
    private String period;
    private Instant began = Instant.now();
    private long checked;
    private long flushed = Long.MIN_VALUE;
    private final StringBuilder pending = new StringBuilder();
    private CompletableFuture<Void> jobs = CompletableFuture.completedFuture(null);

    /// Appends one line per entry to `path`.
    public FileSink(Path path) {
        this.path = path;
    }

    /// Appends one line per entry to `path`.
    public FileSink(String path) {
        this(Path.of(path));
    }

    /// Whether to keep what the file holds, `true` by default.
    public FileSink append(boolean append) {
        this.append = append;
        return this;
    }

    /// The permissions of a new file, such as `0640`; ignored where the file system has none.
    public FileSink perms(String octal) {
        this.perms = octal;
        return this;
    }

    /// Whether to make the directory, `true` by default.
    public FileSink mkdir(boolean mkdir) {
        this.mkdir = mkdir;
        return this;
    }

    /// Rolls over once the file would pass this many bytes.
    public FileSink maxSize(long bytes) {
        this.maxSize = bytes;
        return this;
    }

    /// Rolls over when the period turns.
    public FileSink rotate(Rotate rotate) {
        this.rotate = rotate;
        return this;
    }

    /// Keeps this many rolled files.
    public FileSink keep(int keep) {
        this.keep = keep;
        return this;
    }

    /// Deletes rolled files older than this.
    public FileSink maxAge(Duration age) {
        this.maxAge = age;
        return this;
    }

    /// Deletes the oldest rolled files while they hold more than this many bytes.
    public FileSink maxTotal(long bytes) {
        this.maxTotal = bytes;
        return this;
    }

    /// Compresses rolled files in the background.
    public FileSink gzip(boolean gzip) {
        this.gzip = gzip;
        return this;
    }

    /// The name of a rolled file, such as `{stem}.{time:%Y%m%d}.{ext}`.
    public FileSink rollName(String template) {
        this.rollName = template;
        return this;
    }

    /// Checks once a second whether the file was moved, truncated or deleted, and reopens it.
    public FileSink reopen(boolean reopen) {
        this.reopen = reopen;
        return this;
    }

    /// Holds up to this many bytes, writing them every `every` and at once for `error` and above.
    public FileSink buffer(long bytes, Duration every) {
        this.buffer = bytes;
        this.flushEvery = every;
        return this;
    }

    /// The path written to.
    public Path path() {
        return path;
    }

    @Override
    public String kind() {
        return "file";
    }

    private void open(boolean fresh) throws IOException {
        var isNew = !Files.exists(path);
        var opts = EnumSet.of(StandardOpenOption.CREATE, StandardOpenOption.WRITE);
        opts.add(fresh && !append ? StandardOpenOption.TRUNCATE_EXISTING : StandardOpenOption.APPEND);
        if (isNew
                && perms != null
                && path.getFileSystem().supportedFileAttributeViews().contains("posix")) {
            try {
                Files.createFile(path, PosixFilePermissions.asFileAttribute(posix(perms)));
            } catch (FileAlreadyExistsException _) {
            }
        }
        ch = FileChannel.open(path, opts);
        size = isNew ? 0 : ch.size();
        began = Files.getLastModifiedTime(path).toInstant();
        period = rotate == null ? null : Clock.strftime(began, rotate.period);
    }

    private static Set<PosixFilePermission> posix(String octal) {
        var bits = Integer.parseInt(octal, 8);
        var out = EnumSet.noneOf(PosixFilePermission.class);
        var all = PosixFilePermission.values();
        for (var i = 0; i < 9; i++) if ((bits & (1 << (8 - i))) != 0) out.add(all[i]);
        return out;
    }

    private String[] parts() {
        var name = path.getFileName().toString();
        var dot = name.lastIndexOf('.');
        return dot > 0 ? new String[] {name.substring(0, dot), name.substring(dot + 1)} : new String[] {name, ""};
    }

    private Path name(Instant began) {
        var p = parts();
        var out = HOLE.matcher(rollName)
                .replaceAll(m -> Matcher.quoteReplacement(
                        switch (m.group(1)) {
                            case "stem" -> p[0];
                            case "ext" -> p[1];
                            case String h when h.startsWith("time:") -> Clock.strftime(began, h.substring(5));
                            default -> m.group();
                        }))
                .replaceAll("\\.+$", "");
        var dir = path.toAbsolutePath().getParent();
        if (free(dir, out)) return dir.resolve(out);
        var dot = out.lastIndexOf('.');
        var base = dot > 0 ? out.substring(0, dot) : out;
        var tail = dot > 0 ? out.substring(dot) : "";
        for (var n = 1; ; n++) if (free(dir, base + "-" + n + tail)) return dir.resolve(base + "-" + n + tail);
    }

    private static boolean free(Path dir, String n) {
        return !Files.exists(dir.resolve(n)) && !Files.exists(dir.resolve(n + ".gz"));
    }

    private Pattern[] shape() {
        var p = parts();
        var plain = HOLE.matcher(rollName)
                .replaceAll(m -> switch (m.group(1)) {
                    case "stem" -> Matcher.quoteReplacement(p[0]);
                    case "ext" -> Matcher.quoteReplacement(p[1]);
                    default -> "*";
                })
                .replaceAll("\\.+$", "");
        var dot = plain.lastIndexOf('.');
        var numbered = dot > 0 ? plain.substring(0, dot) + "*" + plain.substring(dot) : plain + "*";
        return new Pattern[] {glob(plain), glob(numbered)};
    }

    private static Pattern glob(String g) {
        return Pattern.compile(
                Arrays.stream(g.split("\\*", -1)).map(Pattern::quote).collect(Collectors.joining(".*")));
    }

    private record Old(Path path, long time, long size) {}

    private void prune() throws IOException {
        if (keep == null && maxAge == null && maxTotal == null) return;
        var shapes = shape();
        var dir = path.toAbsolutePath().getParent();
        var self = path.getFileName().toString();
        var old = new ArrayList<Old>();
        try (var list = Files.list(dir)) {
            for (var p : list.toList()) {
                var n = p.getFileName().toString();
                var bare = n.endsWith(".gz") ? n.substring(0, n.length() - 3) : n;
                if (n.equals(self)
                        || Arrays.stream(shapes).noneMatch(s -> s.matcher(bare).matches())) continue;
                old.add(new Old(p, Files.getLastModifiedTime(p).toMillis(), Files.size(p)));
            }
        }
        old.sort(Comparator.comparingLong(Old::time).thenComparing(o -> o.path().toString()));
        var total = old.stream().mapToLong(Old::size).sum();
        var left = old.size();
        var now = System.currentTimeMillis();
        for (var f : old) {
            var aged = maxAge != null && now - f.time() > maxAge.toMillis();
            var many = keep != null && left > keep;
            var big = maxTotal != null && total > maxTotal;
            if (!(aged || many || big)) continue;
            Files.deleteIfExists(f.path());
            left--;
            total -= f.size();
        }
    }

    private void after(Path rolled) {
        try {
            if (gzip) {
                var mtime = Files.getLastModifiedTime(rolled);
                var gz = rolled.resolveSibling(rolled.getFileName() + ".gz");
                try (var in = Files.newInputStream(rolled);
                        OutputStream out = new GZIPOutputStream(Files.newOutputStream(gz))) {
                    in.transferTo(out);
                }
                Files.setLastModifiedTime(gz, FileTime.from(mtime.toInstant()));
                Files.delete(rolled);
            }
            prune();
        } catch (IOException e) {
            throw new java.io.UncheckedIOException(e);
        }
    }

    private void roll(Instant t) throws IOException {
        drain();
        if (ch != null) ch.close();
        var to = name(began);
        Files.move(path, to);
        open(false);
        began = t;
        period = rotate == null ? null : Clock.strftime(t, rotate.period);
        jobs = jobs.handle((_, _) -> null).thenRunAsync(() -> after(to), BACKGROUND);
    }

    private void drain() throws IOException {
        if (!pending.isEmpty() && ch != null) {
            write(pending.toString());
            pending.setLength(0);
        }
        flushed = System.nanoTime();
    }

    private void write(String text) throws IOException {
        var buf = ByteBuffer.wrap(text.getBytes(StandardCharsets.UTF_8));
        while (buf.hasRemaining()) ch.write(buf);
    }

    private void put(String text, Instant t, boolean urgent) throws IOException {
        var now = System.nanoTime();
        if (reopen && ch != null && now - checked >= 1_000_000_000L) {
            checked = now;
            if (!Files.exists(path) || Files.size(path) < size) {
                drain();
                ch.close();
                ch = null;
            }
        }
        if (ch == null) open(false);
        var n = text.getBytes(StandardCharsets.UTF_8).length;
        var full = maxSize != null && size > 0 && size + n > maxSize;
        var turned = rotate != null && !Clock.strftime(t, rotate.period).equals(period);
        if (full || turned) roll(t);
        size += n;
        if (buffer == 0) {
            write(text);
            return;
        }
        pending.append(text);
        if (urgent || pending.length() >= buffer || flushed == Long.MIN_VALUE || now - flushed >= flushEvery.toNanos())
            drain();
    }

    @Override
    public void emit(Entry entry) throws IOException {
        put(entry.line() + "\n", entry.record().time(), entry.record().level().atLeast(Level.ERROR));
    }

    @Override
    public void emitBatch(List<Entry> entries) throws IOException {
        if (entries.isEmpty()) return;
        if (maxSize != null || rotate != null) {
            for (var e : entries) emit(e);
            return;
        }
        var text = new StringBuilder();
        var urgent = false;
        for (var e : entries) {
            text.append(e.line()).append('\n');
            urgent |= e.record().level().atLeast(Level.ERROR);
        }
        put(text.toString(), entries.getLast().record().time(), urgent);
    }

    @Override
    public void flush() throws IOException {
        drain();
        try {
            jobs.join();
        } catch (RuntimeException e) {
            jobs = CompletableFuture.completedFuture(null);
            throw new IOException(
                    "cleaning up rolled files failed: "
                            + SerlogException.message(e.getCause() != null ? e.getCause() : e),
                    e);
        }
    }

    @Override
    public void close() throws IOException {
        flush();
        if (ch != null) {
            ch.close();
            ch = null;
        }
    }

    @Override
    public void ready() throws IOException {
        if (Files.isDirectory(path)) throw new SerlogException.Config(path + " is a directory");
        var dir = path.toAbsolutePath().getParent();
        if (mkdir) Files.createDirectories(dir);
        else if (!Files.isDirectory(dir)) throw new SerlogException.Config(dir + " does not exist");
        open(true);
    }

    @Override
    public Optional<String> claim() {
        Path p;
        try {
            p = path.toRealPath();
        } catch (IOException e) {
            p = path.toAbsolutePath().normalize();
        }
        var s = p.toString();
        return Optional.of(
                "file " + (System.getProperty("os.name", "").startsWith("Windows") ? s.toLowerCase(Locale.ROOT) : s));
    }

    @Override
    public Optional<Map<String, Object>> options() {
        var o = new LinkedHashMap<String, Object>();
        o.put("path", path.toString());
        if (!append) o.put("append", false);
        if (perms != null) o.put("perms", perms);
        if (!mkdir) o.put("mkdir", false);
        if (maxSize != null) o.put("max_size", Units.showSize(maxSize));
        if (rotate != null) o.put("rotate", rotate.id());
        if (keep != null) o.put("keep", keep);
        if (maxAge != null) o.put("max_age", Units.showTime(maxAge));
        if (maxTotal != null) o.put("max_total", Units.showSize(maxTotal));
        if (gzip) o.put("gzip", true);
        if (!rollName.equals(NAME)) o.put("roll_name", rollName);
        if (reopen) o.put("reopen", true);
        if (buffer > 0) {
            o.put("buffer", Units.showSize(buffer));
            o.put("flush_every", Units.showTime(flushEvery));
        }
        return Optional.of(o);
    }
}
