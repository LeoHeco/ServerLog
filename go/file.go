package serlog

import (
	"cmp"
	"compress/gzip"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"regexp"
	"runtime"
	"slices"
	"strconv"
	"strings"
	"sync"
	"time"
)

// RollName is the default name of a rolled file.
const RollName = "{stem}.{time:%Y%m%d-%H%M%S}.{ext}"

var periods = map[string]string{"minute": "%Y%m%d%H%M", "hour": "%Y%m%d%H", "day": "%Y%m%d", "week": "%G%V"}

var holes = regexp.MustCompile(`\{([^}]*)\}`)

// File appends one line per entry to Path, rolls over by MaxSize or Rotate ("minute", "hour", "day" or "week" on
// local time), cleans up rolled files by Keep, MaxAge or MaxTotal, and with Reopen checks once a second whether
// the file was moved, truncated or deleted. Buffer holds up to that many bytes, written every FlushEvery and at
// once for error and above.
type File struct {
	Slot
	Path       string
	Truncate   bool
	Perms      string
	NoMkdir    bool
	MaxSize    int64
	Rotate     string
	Keep       int
	MaxAge     time.Duration
	MaxTotal   int64
	Gzip       bool
	RollName   string
	Reopen     bool
	Buffer     int64
	FlushEvery time.Duration

	f       *os.File
	size    int64
	period  string
	began   time.Time
	checked time.Time
	flushed time.Time
	pending []byte
	jobs    sync.WaitGroup
	jobErr  error
	jobMu   sync.Mutex
}

// Kind is "file".
func (f *File) Kind() string { return "file" }

func (f *File) template() string { return or(f.RollName, RollName) }

func (f *File) mode() (os.FileMode, error) {
	if f.Perms == "" {
		return 0o644, nil
	}
	m, err := strconv.ParseUint(f.Perms, 8, 32)
	if err != nil {
		return 0, configErr("file: bad `perms` %q", f.Perms)
	}
	return os.FileMode(m), nil
}

func (f *File) open(fresh bool) error {
	_, statErr := os.Stat(f.Path)
	isNew := errors.Is(statErr, fs.ErrNotExist)
	flags := os.O_CREATE | os.O_WRONLY | os.O_APPEND
	if fresh && f.Truncate {
		flags = os.O_CREATE | os.O_WRONLY | os.O_TRUNC
	}
	mode, err := f.mode()
	if err != nil {
		return err
	}
	fh, err := os.OpenFile(f.Path, flags, mode)
	if err != nil {
		return err
	}
	st, err := fh.Stat()
	if err != nil {
		fh.Close()
		return err
	}
	f.f, f.size, f.began = fh, st.Size(), st.ModTime()
	if isNew {
		f.size = 0
	}
	if f.Rotate != "" {
		f.period = strftime(f.began.Local(), periods[f.Rotate])
	}
	return nil
}

func (f *File) parts() (string, string) {
	name := filepath.Base(f.Path)
	ext := filepath.Ext(name)
	return strings.TrimSuffix(name, ext), strings.TrimPrefix(ext, ".")
}

func free(dir, n string) bool {
	_, a := os.Stat(filepath.Join(dir, n))
	_, b := os.Stat(filepath.Join(dir, n+".gz"))
	return errors.Is(a, fs.ErrNotExist) && errors.Is(b, fs.ErrNotExist)
}

func (f *File) rolledName(began time.Time) string {
	stem, ext := f.parts()
	out := holes.ReplaceAllStringFunc(f.template(), func(m string) string {
		switch h := m[1 : len(m)-1]; {
		case h == "stem":
			return stem
		case h == "ext":
			return ext
		case strings.HasPrefix(h, "time:"):
			return strftime(began.Local(), h[5:])
		}
		return m
	})
	out = strings.TrimRight(out, ".")
	dir := filepath.Dir(f.Path)
	if free(dir, out) {
		return filepath.Join(dir, out)
	}
	base, tail := out, ""
	if dot := strings.LastIndexByte(out, '.'); dot > 0 {
		base, tail = out[:dot], out[dot:]
	}
	for n := 1; ; n++ {
		if c := fmt.Sprintf("%s-%d%s", base, n, tail); free(dir, c) {
			return filepath.Join(dir, c)
		}
	}
}

func glob(g string) *regexp.Regexp {
	parts := strings.Split(g, "*")
	for i, p := range parts {
		parts[i] = regexp.QuoteMeta(p)
	}
	return regexp.MustCompile("^" + strings.Join(parts, ".*") + "$")
}

func (f *File) shapes() []*regexp.Regexp {
	stem, ext := f.parts()
	plain := holes.ReplaceAllStringFunc(f.template(), func(m string) string {
		switch m[1 : len(m)-1] {
		case "stem":
			return stem
		case "ext":
			return ext
		}
		return "*"
	})
	plain = strings.TrimRight(plain, ".")
	numbered := plain + "*"
	if dot := strings.LastIndexByte(plain, '.'); dot > 0 {
		numbered = plain[:dot] + "*" + plain[dot:]
	}
	return []*regexp.Regexp{glob(plain), glob(numbered)}
}

func (f *File) prune() error {
	if f.Keep == 0 && f.MaxAge == 0 && f.MaxTotal == 0 {
		return nil
	}
	dir, self := filepath.Dir(f.Path), filepath.Base(f.Path)
	entries, err := os.ReadDir(dir)
	if err != nil {
		return err
	}
	type old struct {
		path string
		time time.Time
		size int64
	}
	var olds []old
	shapes := f.shapes()
	for _, e := range entries {
		n := e.Name()
		bare := strings.TrimSuffix(n, ".gz")
		if n == self || !slices.ContainsFunc(shapes, func(r *regexp.Regexp) bool { return r.MatchString(bare) }) {
			continue
		}
		if info, err := e.Info(); err == nil {
			olds = append(olds, old{filepath.Join(dir, n), info.ModTime(), info.Size()})
		}
	}
	slices.SortFunc(olds, func(a, b old) int { return cmp.Or(a.time.Compare(b.time), strings.Compare(a.path, b.path)) })
	var total int64
	for _, o := range olds {
		total += o.size
	}
	left := len(olds)
	for _, o := range olds {
		aged := f.MaxAge > 0 && time.Since(o.time) > f.MaxAge
		many := f.Keep > 0 && left > f.Keep
		big := f.MaxTotal > 0 && total > f.MaxTotal
		if !aged && !many && !big {
			continue
		}
		if err := os.Remove(o.path); err != nil && !errors.Is(err, fs.ErrNotExist) {
			return err
		}
		left--
		total -= o.size
	}
	return nil
}

func compress(path string) error {
	info, err := os.Stat(path)
	if err != nil {
		return err
	}
	in, err := os.Open(path)
	if err != nil {
		return err
	}
	defer in.Close()
	out, err := os.Create(path + ".gz")
	if err != nil {
		return err
	}
	zw := gzip.NewWriter(out)
	if _, err := io.Copy(zw, in); err != nil {
		out.Close()
		return err
	}
	if err := zw.Close(); err != nil {
		out.Close()
		return err
	}
	if err := out.Close(); err != nil {
		return err
	}
	in.Close()
	if err := os.Chtimes(path+".gz", info.ModTime(), info.ModTime()); err != nil {
		return err
	}
	return os.Remove(path)
}

func (f *File) after(rolled string) {
	defer f.jobs.Done()
	err := error(nil)
	if f.Gzip {
		err = compress(rolled)
	}
	if err == nil {
		err = f.prune()
	}
	if err != nil {
		f.jobMu.Lock()
		f.jobErr = err
		f.jobMu.Unlock()
	}
}

func (f *File) roll(t time.Time) error {
	if err := f.drain(); err != nil {
		return err
	}
	if f.f != nil {
		f.f.Close()
		f.f = nil
	}
	to := f.rolledName(f.began)
	if err := os.Rename(f.Path, to); err != nil {
		return err
	}
	if err := f.open(false); err != nil {
		return err
	}
	f.began = t
	if f.Rotate != "" {
		f.period = strftime(t.Local(), periods[f.Rotate])
	}
	f.jobs.Wait()
	f.jobs.Add(1)
	go f.after(to)
	return nil
}

func (f *File) drain() error {
	f.flushed = time.Now()
	if len(f.pending) == 0 || f.f == nil {
		return nil
	}
	_, err := f.f.Write(f.pending)
	f.pending = f.pending[:0]
	return err
}

func (f *File) put(text string, t time.Time, urgent bool) error {
	if f.Reopen && f.f != nil && time.Since(f.checked) >= time.Second {
		f.checked = time.Now()
		if st, err := os.Stat(f.Path); err != nil || st.Size() < f.size {
			if err := f.drain(); err != nil {
				return err
			}
			f.f.Close()
			f.f = nil
		}
	}
	if f.f == nil {
		if err := f.open(false); err != nil {
			return err
		}
	}
	n := int64(len(text))
	full := f.MaxSize > 0 && f.size > 0 && f.size+n > f.MaxSize
	turned := f.Rotate != "" && strftime(t.Local(), periods[f.Rotate]) != f.period
	if full || turned {
		if err := f.roll(t); err != nil {
			return err
		}
	}
	f.size += n
	if f.Buffer == 0 {
		_, err := io.WriteString(f.f, text)
		return err
	}
	f.pending = append(f.pending, text...)
	if urgent || int64(len(f.pending)) >= f.Buffer || f.flushed.IsZero() || time.Since(f.flushed) >= cmp.Or(f.FlushEvery, time.Second) {
		return f.drain()
	}
	return nil
}

// Emit appends the line.
func (f *File) Emit(e *Entry) error {
	return f.put(e.Line()+"\n", e.Record.Time, e.Record.Level >= LevelError)
}

// EmitBatch appends a batch with one write unless it may roll over in between.
func (f *File) EmitBatch(es []*Entry) error {
	if f.MaxSize > 0 || f.Rotate != "" {
		for _, e := range es {
			if err := f.Emit(e); err != nil {
				return err
			}
		}
		return nil
	}
	var b strings.Builder
	urgent := false
	for _, e := range es {
		b.WriteString(e.Line() + "\n")
		urgent = urgent || e.Record.Level >= LevelError
	}
	return f.put(b.String(), es[len(es)-1].Record.Time, urgent)
}

// Flush writes out the buffer and waits for rolled files to be compressed and cleaned up.
func (f *File) Flush() error {
	err := f.drain()
	f.jobs.Wait()
	f.jobMu.Lock()
	defer f.jobMu.Unlock()
	if err == nil && f.jobErr != nil {
		err, f.jobErr = fmt.Errorf("cleaning up rolled files failed: %w", f.jobErr), nil
	}
	return err
}

// Close writes out the buffer, waits for rolled files and closes the file.
func (f *File) Close() error {
	err := f.Flush()
	if f.f != nil {
		err = errors.Join(err, f.f.Close())
		f.f = nil
	}
	return err
}

// Ready makes the directory and opens the file.
func (f *File) Ready() error {
	if f.Path == "" {
		return configErr("file: `path` is required")
	}
	if f.Rotate != "" && periods[f.Rotate] == "" {
		return configErr("file: `rotate` is minute, hour, day or week, not `%s`", f.Rotate)
	}
	if st, err := os.Stat(f.Path); err == nil && st.IsDir() {
		return configErr("%s is a directory", f.Path)
	}
	dir := filepath.Dir(f.Path)
	if !f.NoMkdir {
		if err := os.MkdirAll(dir, 0o755); err != nil {
			return err
		}
	} else if _, err := os.Stat(dir); err != nil {
		return configErr("%s does not exist", dir)
	}
	return f.open(true)
}

// Claim is the file's canonical absolute path.
func (f *File) Claim() string {
	p, err := filepath.Abs(f.Path)
	if err != nil {
		p = f.Path
	}
	if r, err := filepath.EvalSymlinks(p); err == nil {
		p = r
	} else if d, err := filepath.EvalSymlinks(filepath.Dir(p)); err == nil {
		p = filepath.Join(d, filepath.Base(p))
	}
	if runtime.GOOS == "windows" {
		p = strings.ToLower(p)
	}
	return "file " + p
}

// Options describes the sink for export.
func (f *File) Options() map[string]any {
	o := map[string]any{"path": f.Path}
	if f.Truncate {
		o["append"] = false
	}
	if f.Perms != "" {
		o["perms"] = f.Perms
	}
	if f.NoMkdir {
		o["mkdir"] = false
	}
	if f.MaxSize > 0 {
		o["max_size"] = showSize(f.MaxSize)
	}
	if f.Rotate != "" {
		o["rotate"] = f.Rotate
	}
	if f.Keep > 0 {
		o["keep"] = f.Keep
	}
	if f.MaxAge > 0 {
		o["max_age"] = showTime(f.MaxAge)
	}
	if f.MaxTotal > 0 {
		o["max_total"] = showSize(f.MaxTotal)
	}
	if f.Gzip {
		o["gzip"] = true
	}
	if f.RollName != "" && f.RollName != RollName {
		o["roll_name"] = f.RollName
	}
	if f.Reopen {
		o["reopen"] = true
	}
	if f.Buffer > 0 {
		o["buffer"] = showSize(f.Buffer)
		o["flush_every"] = showTime(cmp.Or(f.FlushEvery, time.Second))
	}
	return o
}
