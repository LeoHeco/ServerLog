package serlog

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"
)

const fullConfig = `
app: demo
level: trace
watch: 1s
levels:
  notice: { value: 35, colour: blue }
process:
  - fields: { region: cn }
redact: [pass]
alarms:
  ops:
    tiers: [[oncall], [stderr]]
    remind: 30m
sinks:
  - { kind: captured, label: all, name: all, format: json }
  - { kind: captured, label: notices, level: notice, pattern: "{level}:{msg}" }
  - { kind: captured, label: oncall, name: oncall, alarm_only: true }
  - { kind: file, name: app, path: "${SERLOG_TEST_DIR}/app.log", max_size: 1MiB, keep: 3, on_fail: ops, queue: 100, wait: 50ms }
`

type captured struct {
	Memory
	label string
}

func (c *captured) Kind() string            { return "captured" }
func (c *captured) Options() map[string]any { return map[string]any{"label": c.label} }

var caught = map[string]*captured{}

func reloadScenario() error {
	RegisterSink("captured", func(o *Options) (Sink, error) {
		c := &captured{label: o.Need("label")}
		c.Size = 100
		caught[c.label] = c
		return c, nil
	})
	if err := os.WriteFile("serlog.yaml", []byte(os.Getenv("CONFIG")), 0o644); err != nil {
		return err
	}
	if err := Load("serlog.yaml"); err != nil {
		return err
	}
	Get("main").Info("hello", "pass", "secret")
	Log(nil, 35, "notable")
	Flush()
	out := map[string]any{"all": caught["all"].Lines(), "notices": caught["notices"].Lines()}
	var names []string
	for _, s := range Statuses() {
		names = append(names, s.Name)
	}
	out["names"] = names
	b, _ := os.ReadFile(filepath.Join(os.Getenv("SERLOG_TEST_DIR"), "app.log"))
	out["file"] = string(b)
	y, err := Dump(FormatYAML)
	if err != nil {
		return err
	}
	out["yaml"] = y
	var trees []map[string]any
	for _, f := range []Format{FormatYAML, FormatJSON, FormatTOML} {
		d, err := Dump(f)
		if err != nil {
			return err
		}
		t, err := Read(d, f)
		if err != nil {
			return fmt.Errorf("%s: %w\n%s", f, err, d)
		}
		trees = append(trees, t)
	}
	out["same"] = reflect.DeepEqual(trees[0], trees[1]) && reflect.DeepEqual(trees[1], trees[2])
	if out["same"] == false {
		out["trees"] = fmt.Sprint(trees)
	}
	time.Sleep(1100 * time.Millisecond)
	os.WriteFile("serlog.yaml", []byte("level: info\nsinks:\n  - { kind: captured, label: after, name: after }\n"), 0o644)
	time.Sleep(2500 * time.Millisecond)
	Info("reloaded")
	Flush()
	out["after"] = caught["after"].Lines()
	names = nil
	for _, s := range Statuses() {
		names = append(names, s.Name)
	}
	out["names_after"] = names
	return json.NewEncoder(os.Stdout).Encode(out)
}

func scenario(name string) error {
	switch name {
	case "reload":
		return reloadScenario()
	case "zero":
		if err := Init(Config{}); err != nil {
			return err
		}
		Debug("quiet")
		Warn("loud", "n", 1)
	case "debug":
		if err := Init(Config{}); err != nil {
			return err
		}
		Get("app:db").Debug("on")
		Get("app:noisy").Debug("off")
		Get("web").Debug("no")
	case "python":
		if err := Init(Config{App: "demo", Compat: CompatPython}); err != nil {
			return err
		}
		Info("hidden")
		Warn("shown")
		Fatal("worse")
	case "twice":
		if err := Init(Config{}); err != nil {
			return err
		}
		return Init(Config{})
	case "empty":
		os.WriteFile("serlog.yaml", nil, 0o644)
		if err := Init(Config{}); err != nil {
			return err
		}
		Get("demo").Info("up")
	case "panic":
		bad := SinkFunc(func(*Entry) error { return errors.New("gone") })
		if err := Init(Config{Sinks: []Sink{With(bad, Slot{Queue: 10, Wait: 10 * time.Millisecond, OnFail: "panic", Retries: NoRetries})}}); err != nil {
			return err
		}
		Info("x")
		time.Sleep(5 * time.Second)
	}
	Flush()
	return nil
}

func TestMain(m *testing.M) {
	if name := os.Getenv("SERLOG_SCENARIO"); name != "" {
		if err := scenario(name); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		os.Exit(0)
	}
	os.Exit(m.Run())
}

type ran struct {
	out, err string
	code     int
}

func run(t *testing.T, name string, env ...string) ran {
	t.Helper()
	dir := t.TempDir()
	cmd := exec.Command(os.Args[0], "-test.run=^$")
	cmd.Dir = dir
	for _, kv := range os.Environ() {
		if !strings.HasPrefix(kv, "FORCE_COLOR=") {
			cmd.Env = append(cmd.Env, kv)
		}
	}
	cmd.Env = append(cmd.Env, append([]string{"SERLOG_SCENARIO=" + name, "NO_COLOR=1", "SERLOG_TEST_DIR=" + filepath.ToSlash(dir)}, env...)...)
	var out, errb strings.Builder
	cmd.Stdout, cmd.Stderr = &out, &errb
	err := cmd.Run()
	code := 0
	var x *exec.ExitError
	if errors.As(err, &x) {
		code = x.ExitCode()
	}
	return ran{out.String(), errb.String(), code}
}

func TestLoadsDumpsAndReloads(t *testing.T) {
	r := run(t, "reload", "CONFIG="+fullConfig)
	if r.code != 0 {
		t.Fatal(r.err)
	}
	var out struct {
		All, Notices, Names []string
		File, Yaml          string
		Same                bool
		Trees               string
		After               []string
		NamesAfter          []string `json:"names_after"`
	}
	lines := strings.Split(strings.TrimSpace(r.out), "\n")
	if err := json.Unmarshal([]byte(lines[len(lines)-1]), &out); err != nil {
		t.Fatal(err, r.out)
	}
	hit := false
	for _, l := range out.All {
		hit = hit || strings.Contains(l, `"msg":"hello"`) && strings.Contains(l, `"region":"cn"`) && strings.Contains(l, `"pass":"***"`)
	}
	if !hit {
		t.Fatal(out.All)
	}
	if fmt.Sprint(out.Notices) != "[NOTICE:notable]" || !strings.Contains(out.File, "hello") {
		t.Fatal(out.Notices, out.File)
	}
	if fmt.Sprint(out.Names) != "[all captured-1 oncall app]" {
		t.Fatal(out.Names)
	}
	for _, want := range []string{"notice", "region", "redact", "ops", "remind: 30m", "max_size: 1MiB", "on_fail: ops", "queue: 100"} {
		if !strings.Contains(out.Yaml, want) {
			t.Fatalf("no %q in\n%s", want, out.Yaml)
		}
	}
	if !out.Same {
		t.Fatal(out.Trees)
	}
	if len(out.After) == 0 || !strings.HasSuffix(out.After[len(out.After)-1], "reloaded") || fmt.Sprint(out.NamesAfter) != "[after]" {
		t.Fatal(out.After, out.NamesAfter)
	}
}

func TestReadsTheSameTreeFromEveryFormat(t *testing.T) {
	y, _ := Read("filter: info\nsinks:\n  - kind: console\n    stream: stderr\n", FormatYAML)
	j, _ := Read(`{"filter":"info","sinks":[{"kind":"console","stream":"stderr"}]}`, FormatJSON)
	o, _ := Read("filter = \"info\"\n[[sinks]]\nkind = \"console\"\nstream = \"stderr\"\n", FormatTOML)
	if !reflect.DeepEqual(y, j) || !reflect.DeepEqual(j, o) {
		t.Fatal(y, j, o)
	}
}

func TestReportsMistakesByName(t *testing.T) {
	for _, c := range []struct {
		yaml, says string
	}{
		{"sinks: [{ kind: console, colr: never }]", "unknown key `colr`"},
		{"bogus: 1", "unknown key `bogus`"},
		{"sinks: [{ kind: console, level: info, filter: info }]", "not both"},
		{"sinks: [{ kind: console, on_fail: pager }]", "not in `alarms`"},
		{"sinks: [{ kind: console, format: xml }]", "format"},
		{"alarms: { panic: { tiers: [[stderr]] } }", "cannot name"},
		{"process: [{ shout: {} }]", "shout"},
		{"alarms: { ops: { tiers: [[nobody]] } }\nsinks: [{ kind: memory, on_fail: ops }]", "nobody"},
	} {
		t.Run(c.says, func(t *testing.T) {
			tree, err := Read(c.yaml, FormatYAML)
			if err != nil {
				t.Fatal(err)
			}
			cfg, err := Parse(tree)
			if err == nil {
				_, err = makePlan(cfg, nil)
			}
			if err == nil || !strings.Contains(err.Error(), c.says) {
				t.Fatalf("got %v, want %q", err, c.says)
			}
		})
	}
}

func TestUnknownKindsAndVariables(t *testing.T) {
	var k *KindError
	if _, err := SinkFrom(map[string]any{"kind": "kafka"}); !errors.As(err, &k) {
		t.Fatal(err)
	}
	if _, err := Expand(map[string]any{"x": "${SERLOG_SURELY_UNSET}"}); err == nil || !strings.Contains(err.Error(), "SERLOG_SURELY_UNSET") {
		t.Fatal(err)
	}
}

func TestZeroConfigFollowsTheEnvironment(t *testing.T) {
	r := run(t, "zero", "SERLOG_FILTER=warn", "SERLOG_FORMAT=json")
	var rec map[string]any
	if r.code != 0 || json.Unmarshal([]byte(strings.TrimSpace(r.out)), &rec) != nil {
		t.Fatal(r.out, r.err)
	}
	if rec["level"] != "warn" || rec["msg"] != "loud" || rec["n"] != 1.0 {
		t.Fatal(rec)
	}
}

func TestDebugAddsNamespacesAtDebug(t *testing.T) {
	r := run(t, "debug", "DEBUG=app:*,-app:noisy")
	lines := strings.Split(strings.TrimSpace(r.out), "\n")
	if r.code != 0 || len(lines) != 1 || !strings.HasSuffix(lines[0], "DEBUG app:db: on") {
		t.Fatal(r.out, r.err)
	}
}

func TestProfilesChangePatternStreamAndLevel(t *testing.T) {
	r := run(t, "python")
	if r.code != 0 || r.out != "" || strings.TrimSpace(r.err) != "WARNING:demo:shown\nCRITICAL:demo:worse" {
		t.Fatalf("%d %q %q", r.code, r.out, r.err)
	}
}

func TestInitRunsOnce(t *testing.T) {
	r := run(t, "twice")
	if r.code == 0 || !strings.Contains(r.err, "already initialised") {
		t.Fatal(r.code, r.err)
	}
}

func TestAnEmptyConfigFileStartsTheConsole(t *testing.T) {
	r := run(t, "empty")
	if r.code != 0 || !strings.HasSuffix(r.out, "demo: up\n") {
		t.Fatal(r.out, r.err)
	}
}

func TestABackgroundPanicExitsWith70(t *testing.T) {
	r := run(t, "panic")
	if r.code != 70 || !strings.Contains(r.err, "gone") {
		t.Fatal(r.code, r.err)
	}
}
