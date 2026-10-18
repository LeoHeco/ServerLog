package serlog

import (
	"bufio"
	"compress/gzip"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"
)

var msgLayout = MustPattern("{msg}")

func put(t *testing.T, f *File, msg string, at time.Time, l Level) {
	t.Helper()
	r := NewRecord(l, "t", msg)
	r.Time = at
	if err := f.Emit(NewEntry(r, msgLayout)); err != nil {
		t.Fatal(err)
	}
}

func listDir(t *testing.T, dir string) []string {
	t.Helper()
	es, err := os.ReadDir(dir)
	if err != nil {
		t.Fatal(err)
	}
	var out []string
	for _, e := range es {
		out = append(out, e.Name())
	}
	return out
}

func text(t *testing.T, path string) string {
	t.Helper()
	fh, err := os.Open(path)
	if err != nil {
		t.Fatal(err)
	}
	defer fh.Close()
	var r io.Reader = fh
	if strings.HasSuffix(path, ".gz") {
		zr, err := gzip.NewReader(fh)
		if err != nil {
			t.Fatal(err)
		}
		r = zr
	}
	b, _ := io.ReadAll(r)
	return string(b)
}

func ready(t *testing.T, f *File) *File {
	t.Helper()
	if err := f.Ready(); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { f.Close() })
	return f
}

func TestRollsBySizeKeepsTheNewestAndGzips(t *testing.T) {
	d := t.TempDir()
	f := ready(t, &File{Path: filepath.Join(d, "app.log"), MaxSize: 150, Keep: 2, Gzip: true})
	start := time.Now()
	for i := range 12 {
		put(t, f, fmt.Sprintf("line %02d %s", i, strings.Repeat("x", 60)), start.Add(time.Duration(i)*time.Second), LevelInfo)
		f.Flush()
	}
	var rolled []string
	for _, n := range listDir(t, d) {
		if strings.HasSuffix(n, ".log.gz") {
			rolled = append(rolled, n)
		}
	}
	if len(rolled) != 2 || len(listDir(t, d)) != 3 {
		t.Fatal(listDir(t, d))
	}
	if !strings.Contains(text(t, filepath.Join(d, "app.log")), "line 11") {
		t.Fatal("current file")
	}
	var joined string
	for _, n := range rolled {
		joined += text(t, filepath.Join(d, n))
	}
	if !strings.Contains(joined, "line 07") || !strings.Contains(joined, "line 09") || strings.Contains(joined, "line 05") {
		t.Fatal(joined)
	}
}

func TestRotatesWhenThePeriodTurns(t *testing.T) {
	d := t.TempDir()
	f := ready(t, &File{Path: filepath.Join(d, "day.log"), Rotate: "day"})
	now := time.Now()
	put(t, f, "today", now, LevelInfo)
	put(t, f, "still today", now, LevelInfo)
	put(t, f, "tomorrow", now.Add(25*time.Hour), LevelInfo)
	f.Flush()
	old := slices.DeleteFunc(listDir(t, d), func(n string) bool { return n == "day.log" })
	if len(old) != 1 || !strings.Contains(text(t, filepath.Join(d, old[0])), "still today") {
		t.Fatal(old)
	}
	if got := text(t, filepath.Join(d, "day.log")); got != "tomorrow\n" {
		t.Fatal(got)
	}
}

func TestNamesRolledFilesFromATemplate(t *testing.T) {
	d := t.TempDir()
	f := ready(t, &File{Path: filepath.Join(d, "svc.log"), MaxSize: 10, RollName: "{stem}-old.{ext}"})
	for _, m := range []string{"first line", "second line", "third line"} {
		put(t, f, m, time.Now(), LevelInfo)
	}
	f.Flush()
	if got := listDir(t, d); !slices.Equal(got, []string{"svc-old-1.log", "svc-old.log", "svc.log"}) {
		t.Fatal(got)
	}
}

func TestCleansUpByTotalSizeAndLeavesOthers(t *testing.T) {
	d := t.TempDir()
	os.WriteFile(filepath.Join(d, "big.yaml"), []byte("x"), 0o644)
	f := ready(t, &File{Path: filepath.Join(d, "big.log"), MaxSize: 100, MaxTotal: 250})
	for i := range 8 {
		put(t, f, fmt.Sprintf("%d %s", i, strings.Repeat("y", 97)), time.Now(), LevelInfo)
		f.Flush()
	}
	var total int64
	for _, n := range listDir(t, d) {
		if n != "big.log" && n != "big.yaml" {
			st, _ := os.Stat(filepath.Join(d, n))
			total += st.Size()
		}
	}
	if total > 250 || !slices.Contains(listDir(t, d), "big.yaml") {
		t.Fatal(total, listDir(t, d))
	}
}

func TestReopensAfterTheFileIsMovedAway(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("Windows does not let others move a file Go holds open")
	}
	d := t.TempDir()
	path := filepath.Join(d, "moved.log")
	f := ready(t, &File{Path: path, Reopen: true})
	put(t, f, "before", time.Now(), LevelInfo)
	os.Rename(path, path+".1")
	time.Sleep(1100 * time.Millisecond)
	put(t, f, "after", time.Now(), LevelInfo)
	if got := text(t, path); got != "after\n" {
		t.Fatal(got)
	}
}

func TestTruncatesAndMakesDirectories(t *testing.T) {
	d := t.TempDir()
	path := filepath.Join(d, "t.log")
	os.WriteFile(path, []byte("old\n"), 0o644)
	f := ready(t, &File{Path: path, Truncate: true})
	put(t, f, "new", time.Now(), LevelInfo)
	if got := text(t, path); got != "new\n" {
		t.Fatal(got)
	}
	ready(t, &File{Path: filepath.Join(d, "a", "b", "c.log")})
	if _, err := os.Stat(filepath.Join(d, "a", "b", "c.log")); err != nil {
		t.Fatal(err)
	}
	var c *ConfigError
	if err := (&File{Path: filepath.Join(d, "x", "y.log"), NoMkdir: true}).Ready(); !errors.As(err, &c) {
		t.Fatal(err)
	}
}

func TestBuffersUntilAnError(t *testing.T) {
	path := filepath.Join(t.TempDir(), "buf.log")
	f := ready(t, &File{Path: path, Buffer: 64 << 10, FlushEvery: time.Hour})
	put(t, f, "first", time.Now(), LevelInfo)
	put(t, f, "second", time.Now(), LevelInfo)
	if got := text(t, path); got != "first\n" {
		t.Fatal(got)
	}
	put(t, f, "bad", time.Now(), LevelError)
	if got := text(t, path); got != "first\nsecond\nbad\n" {
		t.Fatal(got)
	}
}

func TestAFileClaimsItsCanonicalPath(t *testing.T) {
	d := t.TempDir()
	os.Mkdir(filepath.Join(d, "x"), 0o755)
	a := (&File{Path: filepath.Join(d, "x", "..", "same.log")}).Claim()
	b := (&File{Path: filepath.Join(d, "same.log")}).Claim()
	if a != b {
		t.Fatal(a, b)
	}
}

type seen struct {
	method, path string
	header       http.Header
	body         string
}

func stand(t *testing.T, answers ...int) (*httptest.Server, *[]seen) {
	var mu sync.Mutex
	var got []seen
	s := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var body string
		if r.Header.Get("Content-Encoding") == "gzip" {
			zr, _ := gzip.NewReader(r.Body)
			b, _ := io.ReadAll(zr)
			body = string(b)
		}
		mu.Lock()
		got = append(got, seen{r.Method, r.URL.Path, r.Header, body})
		code := 204
		if len(answers) > 0 {
			code, answers = answers[0], answers[1:]
		}
		mu.Unlock()
		w.WriteHeader(code)
	}))
	t.Cleanup(s.Close)
	return s, &got
}

func TestLokiPushesOneStreamPerLevel(t *testing.T) {
	s, got := stand(t)
	l := &Loki{URL: s.URL + "/loki/api/v1/push", Labels: map[string]string{"team": "mirrors"}}
	lay := l.DefaultLayout()
	a := NewRecord(LevelInfo, "sync", "started", Field{"mirror", "debian"}, Field{"tries", 3})
	a.File, a.Line = "a.go", 3
	if err := l.EmitBatch([]*Entry{NewEntry(a, lay), NewEntry(NewRecord(LevelError, "sync", `failed "rsync"`), lay), NewEntry(NewRecord(LevelInfo, "sync", "retrying"), lay)}); err != nil {
		t.Fatal(err)
	}
	r := (*got)[0]
	if r.path != "/loki/api/v1/push" || r.header.Get("Content-Encoding") != "gzip" || r.header.Get("Content-Type") != "application/json" {
		t.Fatal(r)
	}
	var body struct {
		Streams []struct {
			Stream map[string]string
			Values [][]any
		}
	}
	if err := json.Unmarshal([]byte(r.body), &body); err != nil {
		t.Fatal(err)
	}
	info, bad := body.Streams[0], body.Streams[1]
	if info.Stream["level"] != "info" || info.Stream["team"] != "mirrors" {
		t.Fatal(info.Stream)
	}
	if info.Values[0][1] != "started" || info.Values[1][1] != "retrying" || bad.Values[0][1] != `failed "rsync"` {
		t.Fatal(info.Values, bad.Values)
	}
	if got := fmt.Sprint(info.Values[0][2]); got != "map[file:a.go:3 mirror:debian target:sync tries:3]" {
		t.Fatal(got)
	}
}

func TestLokiTellsRetryableAnswersFromRefusals(t *testing.T) {
	s, _ := stand(t, 503, 400)
	l := &Loki{URL: s.URL + "/loki/api/v1/push"}
	e := NewEntry(NewRecord(LevelInfo, "t", "x"), l.DefaultLayout())
	if err := l.Emit(e); err == nil || !strings.Contains(err.Error(), "503") || !retryable(err) {
		t.Fatal(err)
	}
	if err := l.Emit(e); !errors.Is(err, ErrRejected) {
		t.Fatal(err)
	}
}

func TestLokiChecksItsURLAndAsksReady(t *testing.T) {
	s, got := stand(t)
	var c *ConfigError
	if err := (&Loki{URL: "ftp://x"}).Ready(); !errors.As(err, &c) {
		t.Fatal(err)
	}
	if err := (&Loki{URL: s.URL + "/loki/api/v1/push", Connect: true}).Ready(); err != nil {
		t.Fatal(err)
	}
	if last := (*got)[len(*got)-1]; last.method != "GET" || last.path != "/ready" {
		t.Fatal(last)
	}
}

func smtpStand(t *testing.T) (int, func() string) {
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { ln.Close() })
	var mu sync.Mutex
	var data strings.Builder
	go func() {
		for {
			conn, err := ln.Accept()
			if err != nil {
				return
			}
			go func() {
				defer conn.Close()
				rd := bufio.NewReader(conn)
				fmt.Fprint(conn, "220 test\r\n")
				for {
					line, err := rd.ReadString('\n')
					if err != nil {
						return
					}
					switch cmd := strings.ToUpper(line); {
					case strings.HasPrefix(cmd, "EHLO"):
						fmt.Fprint(conn, "250-test\r\n250 OK\r\n")
					case strings.HasPrefix(cmd, "DATA"):
						fmt.Fprint(conn, "354 go on\r\n")
						for {
							part, err := rd.ReadString('\n')
							if err != nil || part == ".\r\n" {
								break
							}
							mu.Lock()
							data.WriteString(part)
							mu.Unlock()
						}
						fmt.Fprint(conn, "250 queued\r\n")
					case strings.HasPrefix(cmd, "QUIT"):
						fmt.Fprint(conn, "221 bye\r\n")
						return
					default:
						fmt.Fprint(conn, "250 OK\r\n")
					}
				}
			}()
		}
	}()
	return ln.Addr().(*net.TCPAddr).Port, func() string {
		mu.Lock()
		defer mu.Unlock()
		return data.String()
	}
}

func TestMailSendsOneMessagePerBatch(t *testing.T) {
	port, data := smtpStand(t)
	m := &Mail{Host: "127.0.0.1", Port: port, Security: "none", From: "bot@example.org", To: []string{"ops@example.org"}}
	if err := m.Ready(); err != nil {
		t.Fatal(err)
	}
	if m.Enabled(LevelWarn, "x") || !m.Enabled(LevelError, "x") {
		t.Fatal("enabled")
	}
	lay := MustPattern("{level} {msg}")
	if err := m.EmitBatch([]*Entry{NewEntry(NewRecord(LevelError, "sync", "disk <full>"), lay), NewEntry(NewRecord(LevelFatal, "sync", "mirror stalled"), lay)}); err != nil {
		t.Fatal(err)
	}
	all := strings.ReplaceAll(data(), "=\r\n", "")
	for _, want := range []string{"2 FATAL records", "ERROR disk <full>", "disk &lt;full&gt;", "mirror stalled"} {
		if !strings.Contains(all, want) {
			t.Fatalf("no %q in\n%s", want, all)
		}
	}
}

func TestMailRefusesToStartWithoutAddresses(t *testing.T) {
	for _, m := range []*Mail{
		{Host: "h", To: []string{"ops@example.org"}},
		{Host: "h", From: "bot@example.org"},
		{Host: "h", User: "bot@example.org", To: []string{"ops@example.org"}},
	} {
		var c *ConfigError
		if err := m.Ready(); !errors.As(err, &c) {
			t.Fatal(err)
		}
	}
}

func TestTheMailTemplateMatchesTheSpec(t *testing.T) {
	spec, err := os.ReadFile(filepath.Join("..", "spec", "mail.html"))
	if err != nil || string(spec) != MailPage() {
		t.Fatal("mail.html differs from spec/mail.html", err)
	}
}
