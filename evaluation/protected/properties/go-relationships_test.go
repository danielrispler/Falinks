package fixture_test

// Differential properties L1-L4 (#228 decisions 10-11; evaluation/code-quality.json). Each
// property prints one line per seeded input; `quality` compares the lines with the frozen
// reference's. Only the API the oracle imports is used.

import (
	"encoding/json"
	"fmt"
	"slices"
	"strconv"
	"strings"
	"testing"

	f "fixture"
)

const seed, inputs = 228, 500

// rng is splitmix64.
type rng struct{ s uint64 }

func (r *rng) next() uint64 {
	r.s += 0x9e3779b97f4a7c15
	z := r.s
	z = (z ^ (z >> 30)) * 0xbf58476d1ce4e5b9
	z = (z ^ (z >> 27)) * 0x94d049bb133111eb
	return z ^ (z >> 31)
}

func (r *rng) n(k int) int { return int(r.next() % uint64(k)) }

func emit(id string, i int, value func() any) {
	line := func() (line string) {
		defer func() {
			if recover() != nil {
				line = "panic"
			}
		}()
		b, err := json.Marshal(value())
		if err != nil {
			return strconv.Quote(err.Error())
		}
		return string(b)
	}()
	fmt.Printf("falinks-property %s %d %s\n", id, i, line)
}

// space is 0-3 ASCII spaces and tabs.
func space(r *rng) string {
	var b strings.Builder
	for range r.n(4) {
		b.WriteByte(" \t"[r.n(2)])
	}
	return b.String()
}

// line is blank, or ASCII words with spaces and tabs around and between them.
func line(r *rng) string {
	if r.n(3) == 0 {
		return space(r)
	}
	const letters = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"
	words := make([]string, 1+r.n(3))
	for i := range words {
		w := make([]byte, 1+r.n(6))
		for j := range w {
			w[j] = letters[r.n(len(letters))]
		}
		words[i] = string(w)
	}
	return space(r) + strings.Join(words, " "+space(r)) + space(r)
}

func lines(r *rng) []string {
	out := make([]string, r.n(13))
	for i := range out {
		out[i] = line(r)
	}
	return out
}

func records(r *rng) []f.Record {
	out := make([]f.Record, r.n(13))
	for i := range out {
		out[i] = f.Record{ID: 1 + r.n(100), Text: line(r)}
	}
	return out
}

// empty treats nil and empty alike: the requirements do not decide between them.
func empty[T any](s []T) []T {
	if s == nil {
		return []T{}
	}
	return s
}

func l1(r *rng) any {
	in := lines(r)
	original := append([]string(nil), in...)
	out := f.Produce(in)
	return []any{empty(out), slices.Equal(in, original)}
}

func l2(r *rng) any {
	in := records(r)
	original := append([]f.Record(nil), in...)
	out := f.Consume(in)
	return []any{empty(out), slices.Equal(in, original)}
}

func l3(r *rng) any { return f.Render(records(r)) }

func l4(r *rng) any {
	kept, dropped := f.Stats(lines(r))
	return []int{kept, dropped}
}

func TestFalinksProperties(t *testing.T) {
	for _, p := range []struct {
		id    string
		check func(*rng) any
	}{{"L1", l1}, {"L2", l2}, {"L3", l3}, {"L4", l4}} {
		r := &rng{seed}
		for i := 0; i < inputs; i++ {
			emit(p.id, i, func() any { return p.check(r) })
		}
	}
}
