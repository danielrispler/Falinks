package fixture_test

// Differential properties G1-G5 (#228 decisions 10-11; evaluation/code-quality.json). Each
// property prints one line per seeded input; `quality` compares the lines with the frozen
// reference's. Only the API the oracle imports is used.

import (
	"encoding/json"
	"fmt"
	"net/http/httptest"
	"net/url"
	"slices"
	"sort"
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

var kinds = []string{"red", "Red", "red2", "blue"}

func items(r *rng) []f.Item {
	out := make([]f.Item, r.n(21))
	for i := range out {
		out[i] = f.Item{ID: r.n(100), Kind: kinds[r.n(len(kinds))]}
	}
	return out
}

func param(name, value string) string { return name + "=" + url.QueryEscape(value) }

// kind is absent, empty, in the pool or not.
func kind(r *rng) []string {
	switch r.n(4) {
	case 0:
		return nil
	case 1:
		return []string{param("kind", "")}
	case 2:
		return []string{param("kind", kinds[r.n(len(kinds))])}
	}
	return []string{param("kind", []string{"green", "RED", "re", "blue "}[r.n(4)])}
}

// page sets offset and limit, each absent or a canonical decimal 0-25.
func page(r *rng) []string {
	var out []string
	for _, name := range []string{"offset", "limit"} {
		if r.n(2) == 0 {
			out = append(out, param(name, strconv.Itoa(r.n(26))))
		}
	}
	return out
}

// invalid is empty, negative, not digits, or above the platform int maximum.
func invalid(r *rng) string {
	switch r.n(4) {
	case 0:
		return ""
	case 1:
		return "-" + strconv.Itoa(1+r.n(1000))
	case 2:
		return []string{"x", "ten", "1a", "a1", "1.5", "1e3", "one two"}[r.n(7)]
	}
	return "9223372036854775808" + strings.Repeat("7", r.n(10))
}

type response struct {
	Status int
	Type   string
	Body   any
}

func get(items []f.Item, params []string) response {
	w := httptest.NewRecorder()
	f.Handler(items).ServeHTTP(w, httptest.NewRequest("GET", "/?"+strings.Join(params, "&"), nil))
	if w.Code != 200 {
		return response{Status: w.Code}
	}
	var body any
	if err := json.Unmarshal(w.Body.Bytes(), &body); err != nil {
		body = "invalid JSON"
	}
	return response{w.Code, w.Header().Get("Content-Type"), body}
}

func encoded(value any) string {
	b, _ := json.Marshal(value)
	return string(b)
}

// G1 compares the selected items as a multiset, so ordering is G2's alone.
func g1(r *rng) any {
	got := get(items(r), kind(r))
	list, _ := got.Body.([]any)
	selected := make([]string, 0, len(list))
	for _, item := range list {
		selected = append(selected, encoded(item))
	}
	sort.Strings(selected)
	return []any{got.Status, list == nil, selected}
}

// G2: the response keeps input order, and the caller's slice is unchanged.
func g2(r *rng) any {
	in := items(r)
	original := append([]f.Item(nil), in...)
	got := get(in, kind(r))
	list, _ := got.Body.([]any)
	j := 0
	for _, item := range original {
		if j < len(list) && encoded(list[j]) == encoded(item) {
			j++
		}
	}
	return []bool{j == len(list), slices.Equal(in, original)}
}

func g3(r *rng) any {
	got := get(items(r), append(kind(r), page(r)...))
	return []any{got.Status, got.Body}
}

// G4 compares only the status code.
func g4(r *rng) any {
	params := kind(r)
	bad := r.n(3)
	for i, name := range []string{"offset", "limit"} {
		switch {
		case bad == i || bad == 2:
			params = append(params, param(name, invalid(r)))
		case r.n(2) == 0:
			params = append(params, param(name, strconv.Itoa(r.n(26))))
		}
	}
	return get(items(r), params).Status
}

func g5(r *rng) any {
	in := items(r)
	if r.n(4) == 0 {
		in = nil
	}
	params := kind(r)
	names := []string{"page", "sort", "Kind", "LIMIT", "q", "offset_"}
	for _, i := range r.perm(len(names))[:r.n(4)] {
		params = append(params, param(names[i], []string{"", "1", "red", "-1", "x y"}[r.n(5)]))
	}
	if r.n(2) == 0 {
		for i, j := 0, len(params)-1; i < j; i, j = i+1, j-1 {
			params[i], params[j] = params[j], params[i]
		}
	}
	return get(in, params)
}

func (r *rng) perm(n int) []int {
	out := make([]int, n)
	for i := range out {
		j := r.n(i + 1)
		out[i], out[j] = out[j], i
	}
	return out
}

func TestFalinksProperties(t *testing.T) {
	for _, p := range []struct {
		id    string
		check func(*rng) any
	}{{"G1", g1}, {"G2", g2}, {"G3", g3}, {"G4", g4}, {"G5", g5}} {
		r := &rng{seed}
		for i := 0; i < inputs; i++ {
			emit(p.id, i, func() any { return p.check(r) })
		}
	}
}
