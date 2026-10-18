package io.github.leoheco.serlog;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.Comparator;
import java.util.List;
import java.util.Locale;
import java.util.regex.Pattern;
import java.util.stream.Collectors;
import java.util.stream.Stream;

/// Which records pass, by target and level, written the `RUST_LOG` way:
/// `info,app=debug,app.db=trace,hyper=off,web:*=warn/regex`. The most specific directive wins; anything after the
/// first `/` is a regex the message must match. As with `env_logger`, targets no directive matches are off unless a
/// default level is given, and an empty spec lets `error` and above through.
public final class Filter {
    private record Rule(String target, Pattern glob, Level level) {
        int rank() {
            return (glob == null ? 0 : 1_000_000) - target.replace("*", "").length();
        }

        boolean covers(String t) {
            if (glob != null) return glob.matcher(t).matches();
            if (!t.startsWith(target)) return false;
            var rest = t.substring(target.length());
            return rest.isEmpty() || Stream.of("::", ".", ":", "/").anyMatch(rest::startsWith);
        }
    }

    private final String spec;
    private final Level base;
    private final List<Rule> rules;
    private final Pattern pattern;

    /// Parses comma-separated directives, optionally followed by `/regex`.
    public Filter(String spec) {
        this.spec = spec;
        var cut = spec.indexOf('/');
        var directives = cut < 0 ? spec : spec.substring(0, cut);
        Level base = directives.isBlank() ? Level.ERROR : null;
        var rules = new ArrayList<Rule>();
        for (var d : directives.split(",")) {
            d = d.strip();
            if (d.isEmpty()) continue;
            var eq = d.indexOf('=');
            if (eq >= 0) {
                rules.add(
                        rule(d.substring(0, eq).strip(), off(d.substring(eq + 1).strip())));
                continue;
            }
            try {
                base = off(d);
            } catch (IllegalArgumentException e) {
                rules.add(rule(d, Level.of(0)));
            }
        }
        this.base = base;
        rules.sort(Comparator.comparingInt(Rule::rank));
        this.rules = List.copyOf(rules);
        this.pattern = cut < 0 ? null : Pattern.compile(spec.substring(cut + 1));
    }

    /// Lets `level` and above through for every target.
    public Filter(Level level) {
        this(level.id().equals("custom") ? String.valueOf(level.value()) : level.id());
    }

    /// A filter from a [Filter], a [Level], a level number or a spec.
    public static Filter from(Object f) {
        return switch (f) {
            case Filter x -> x;
            case Level l -> new Filter(l);
            case Number n -> new Filter(Level.of(n.intValue()));
            case String s -> new Filter(s);
            default -> throw new IllegalArgumentException("not a filter: " + f);
        };
    }

    private static Level off(String level) {
        return level.toLowerCase(Locale.ROOT).equals("off") ? null : Level.parse(level);
    }

    private static Rule rule(String target, Level level) {
        var glob = target.contains("*")
                ? Pattern.compile(Arrays.stream(target.split("\\*", -1))
                        .map(Pattern::quote)
                        .collect(Collectors.joining(".*")))
                : null;
        return new Rule(target, glob, level);
    }

    /// Whether a record at `level` from `target` passes, before its message is looked at.
    public boolean allows(Level level, String target) {
        var min = base;
        for (var r : rules)
            if (r.covers(target)) {
                min = r.level();
                break;
            }
        return min != null && level.atLeast(min);
    }

    /// Whether the message passes the `/regex` part, if there is one.
    public boolean admits(String msg) {
        return pattern == null || pattern.matcher(msg).find();
    }

    /// Whether there is a `/regex` part.
    public boolean hasPattern() {
        return pattern != null;
    }

    /// The lowest level any target lets through, or `null` when everything is off.
    public Level floor() {
        return Stream.concat(Stream.of(base), rules.stream().map(Rule::level))
                .filter(l -> l != null)
                .min(Comparator.naturalOrder())
                .orElse(null);
    }

    @Override
    public String toString() {
        return spec;
    }

    @Override
    public boolean equals(Object o) {
        return o instanceof Filter f && f.spec.equals(spec);
    }

    @Override
    public int hashCode() {
        return spec.hashCode();
    }
}
