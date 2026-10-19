// A sink of your own, from definition to use: texting warnings and errors to a phone.
package phone_test

import (
	"fmt"
	"strings"

	serlog "github.com/LeoHeco/ServerLog/go"
)

// region stubs
func fooPhoneIsOk[N int64 | string](phone N) bool {
	s := fmt.Sprint(phone)
	return s != "" && strings.Trim(s, "0123456789") == ""
}

func fooUsePhoneToLog[N int64 | string](phone N, text string) {
	fmt.Printf("sms %v: %s\n", phone, text)
}

// endregion

// region sink
type Phone[N int64 | string] struct {
	serlog.Slot
	Phone N
}

func NewPhone[N int64 | string](phone N) (*Phone[N], error) {
	if !fooPhoneIsOk(phone) {
		return nil, fmt.Errorf("cannot text %v", phone)
	}
	return &Phone[N]{Phone: phone}, nil
}

func (*Phone[N]) Kind() string { return "phone" }

func (p *Phone[N]) Emit(e *serlog.Entry) error {
	fooUsePhoneToLog(p.Phone, e.Line())
	return nil
}

// endregion

// region register
func init() {
	serlog.RegisterSink("phone", func(o *serlog.Options) (serlog.Sink, error) {
		switch n := o.Raw("phone").(type) {
		case int64:
			return sink(NewPhone(n))
		case string:
			return sink(NewPhone(n))
		default:
			return nil, fmt.Errorf("cannot text %v", n)
		}
	})
}

func sink[S serlog.Sink](s S, err error) (serlog.Sink, error) {
	if err != nil {
		return nil, err
	}
	return s, nil
}

// endregion

func Example() {
	// region refuse
	if _, err := NewPhone("call me maybe"); err != nil {
		fmt.Println("refused:", err)
	}
	// endregion

	// region config
	err := serlog.Loads(`
app: mirror
sinks:
  - kind: console
    pattern: "{level:<5} {msg}{fields}"
  - kind: phone
    name: oncall
    phone: 13800138000
    level: warn
    pattern: "{level} {msg}"
`, serlog.FormatYAML)
	if err != nil {
		panic(err)
	}
	// endregion

	// region use
	log := serlog.Get("sync", "mirror", "debian")
	log.Info("synced", "files", 42)
	log.Warn("disk nearly full", "free", "9%")
	log.Error("sync failed", "tries", 3)
	serlog.Flush()
	// endregion

	// region status
	for _, s := range serlog.Statuses() {
		fmt.Printf("%s: %s, %s\n", s.Name, s.Kind, s.Health)
	}
	// endregion

	// Output:
	// refused: cannot text call me maybe
	// INFO  synced mirror=debian files=42
	// WARN  disk nearly full mirror=debian free=9%
	// sms 13800138000: WARN disk nearly full
	// ERROR sync failed mirror=debian tries=3
	// sms 13800138000: ERROR sync failed
	// console-1: console, up
	// oncall: phone, up
}
