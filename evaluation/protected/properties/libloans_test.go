package libloans_test

// Differential properties N1-N9, one per ticket (#587; evaluation/code-quality.json). Each
// property prints one line per seeded input; `quality` compares the lines with the frozen
// reference's. Only the API the oracle imports is used. The requirements fix which sentinels
// an error satisfies, so those are printed; other error text is not. A Date is printed through
// its String, and a rejected one only as whether it is the zero Date, since T3 leaves its fields
// and the zero Date's text to the implementer. Nothing is printed for behaviour the
// requirements leave open: dates outside years 1000-9999, the error of a loan line with more
// than one fault, and the order of rows that tie on every sort key. DaysBetween follows the
// corrected T3 (both ends counted), as the oracle does.

import (
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"strconv"
	"strings"
	"testing"
	"time"

	"example.com/libloans/calendar"
	"example.com/libloans/isbn"
	"example.com/libloans/lending"
	"example.com/libloans/money"
	"example.com/libloans/report"
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

// between is an integer from lo to hi, both included.
func (r *rng) between(lo, hi int) int { return lo + r.n(hi-lo+1) }

func (r *rng) pick(pool []string) string { return pool[r.n(len(pool))] }

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

var sentinels = []struct {
	name string
	err  error
}{
	{"ErrInvalidAmount", money.ErrInvalidAmount}, {"ErrInvalidISBN", isbn.ErrInvalidISBN},
	{"ErrInvalidDate", calendar.ErrInvalidDate}, {"ErrUnknownKind", lending.ErrUnknownKind},
	{"ErrMalformedLine", lending.ErrMalformedLine}, {"ErrAlreadyReturned", lending.ErrAlreadyReturned},
	{"ErrNotRenewable", lending.ErrNotRenewable}, {"ErrOnHold", lending.ErrOnHold},
	{"ErrRenewalLimit", lending.ErrRenewalLimit}, {"ErrOverdue", lending.ErrOverdue},
	{"ErrUnknownPatron", report.ErrUnknownPatron},
}

// kinds names every sentinel err satisfies, or is nil for a nil error.
func kinds(err error) any {
	if err == nil {
		return nil
	}
	out := []string{}
	for _, s := range sentinels {
		if errors.Is(err, s.err) {
			out = append(out, s.name)
		}
	}
	return out
}

// shown is a Date's text when err is nil, else whether it is the zero Date.
func shown(d calendar.Date, err error) any {
	if err != nil {
		return d == calendar.Date{}
	}
	return d.String()
}

func digits(r *rng, n int) string {
	b := make([]byte, n)
	for i := range b {
		b[i] = byte('0' + r.n(10))
	}
	return string(b)
}

func must(d calendar.Date, err error) calendar.Date {
	if err != nil {
		panic(err)
	}
	return d
}

// day is a valid date, mostly near 2026, else in years 1010 to 9990, so that moving it by a
// few thousand days stays within 1000-9999.
func day(r *rng) calendar.Date {
	if r.n(4) == 0 {
		return must(calendar.New(r.between(1010, 9990), time.January, 1)).AddDays(r.n(365))
	}
	return must(calendar.New(2026, time.January, 1)).AddDays(r.between(-400, 800))
}

// kind is a valid kind five times in six.
func kind(r *rng) lending.ItemKind {
	if r.n(6) > 0 {
		return lending.ItemKind(r.between(1, 3))
	}
	return lending.ItemKind([]int{0, 4, -1, 9}[r.n(4)])
}

// pad is ASCII whitespace or nothing.
func pad(r *rng) string { return r.pick([]string{"", "", "", " ", "\t", " \t ", "\n"}) }

// view is a Loan as text, with Returned as a date or nil.
func view(l lending.Loan) any {
	var returned any
	if l.Returned != nil {
		returned = l.Returned.String()
	}
	return []any{l.Patron, string(l.ISBN), l.Title, int(l.Kind), l.Borrowed.String(), l.Due.String(), returned}
}

// n1 checks Parse on amounts near and outside the syntax, and String over int64.
func n1(r *rng) any {
	s := r.pick([]string{"0", "7", "12.50", "007.05", "999999999.99", "1234567890", "", " 1", "1 ",
		"-1", "+1", "$1", "1,000", "1.5", "1.505", "1.", ".50", "-0.00", "1e3", "."})
	switch r.n(3) {
	case 0:
		s = digits(r, r.between(0, 11))
		if r.n(2) == 0 {
			s += "." + digits(r, r.n(4))
		}
	case 1:
		const chars = "0123456789.-+$, "
		b := make([]byte, r.n(9))
		for i := range b {
			b[i] = chars[r.n(len(chars))]
		}
		s = string(b)
	}
	c := money.Cents(r.next())
	switch r.n(4) {
	case 0:
		c = money.Cents([]int64{math.MinInt64, math.MaxInt64, 0, -1, 5, -305, 99, -100}[r.n(8)])
	case 1:
		c = money.Cents(r.between(-200000, 200000))
	}
	parsed, err := money.Parse(s)
	return []any{s, int64(parsed), kinds(err), c.String()}
}

// isbn10 is nine digits and their ISBN-10 check character.
func isbn10(body string) string {
	sum := 0
	for i := range 9 {
		sum += int(body[i]-'0') * (10 - i)
	}
	check := (11 - sum%11) % 11
	if check == 10 {
		return body + "X"
	}
	return body + strconv.Itoa(check)
}

// isbn13 is twelve digits and their ISBN-13 check digit.
func isbn13(body string) string {
	sum := 0
	for i := range 12 {
		sum += int(body[i]-'0') * (1 + 2*(i%2))
	}
	return body + strconv.Itoa((10-sum%10)%10)
}

// n2 checks Parse on valid ISBN-10s and ISBN-13s, one character changed one time in three,
// with hyphens and spaces inside and whitespace around, and on short strings of ISBN
// characters; and String on what it returns.
func n2(r *rng) any {
	var s string
	if r.n(2) == 0 {
		s = isbn10(digits(r, 9))
		if r.n(4) == 0 {
			s = strings.ToLower(s)
		}
	} else {
		prefix := r.pick([]string{"978", "979", ""})
		s = isbn13(prefix + digits(r, 12-len(prefix)))
	}
	if r.n(3) == 0 {
		b := []byte(s)
		b[r.n(len(b))] = "0123456789Xx"[r.n(12)]
		s = string(b)
	}
	if r.n(8) == 0 {
		s = s[:r.n(len(s))] + s[r.n(len(s)):]
	}
	var out strings.Builder
	for i := range len(s) {
		if i > 0 && r.n(5) == 0 {
			out.WriteString(r.pick([]string{"-", " ", "--", " - ", "\t", "_"}))
		}
		out.WriteByte(s[i])
	}
	s = pad(r) + out.String() + pad(r)
	if r.n(10) == 0 {
		const chars = "0123456789Xx- "
		b := make([]byte, r.n(15))
		for i := range b {
			b[i] = chars[r.n(len(chars))]
		}
		s = string(b)
	}
	got, err := isbn.Parse(s)
	return []any{s, string(got), kinds(err), got.String()}
}

// n3 checks New and Parse near and on real dates, and String, AddDays, Weekday, DaysBetween and
// equality on valid ones.
func n3(r *rng) any {
	y, m, d := r.between(995, 10005), r.between(-1, 13), r.between(-1, 32)
	if r.n(3) == 0 {
		y = []int{999, 1000, 9999, 10000, 1900, 2000, 2026, 2027, 2028}[r.n(9)]
	}
	made, madeErr := calendar.New(y, time.Month(m), d)
	s := fmt.Sprintf("%04d-%02d-%02d", r.between(990, 10010), r.between(0, 13), r.between(0, 32))
	switch r.n(5) {
	case 0:
		s = day(r).String()
	case 1:
		s = r.pick([]string{"2028-02-29", "2027-02-29", "2026-04-31", "1900-02-29", "2000-02-29",
			"2026-1-5", "2026/11/10", " 2026-11-10", "2026-11-10 ", "2026-11-10T00:00:00Z", "",
			"0999-12-31", "1000-01-01", "9999-12-31", "10000-01-01", "+026-11-10", "2026-11-10\n"})
	}
	parsed, err := calendar.Parse(s)
	a, b := day(r), day(r)
	n := r.between(-3000, 3000)
	if r.n(4) == 0 {
		b = a.AddDays(r.between(-3, 3))
	}
	same := must(calendar.Parse(a.String()))
	return []any{[]int{y, m, d}, shown(made, madeErr), kinds(madeErr), s, shown(parsed, err), kinds(err),
		a.String(), a.AddDays(n).String(), a.AddDays(n).AddDays(-n) == a, int(a.Weekday()),
		calendar.DaysBetween(a, b), calendar.DaysBetween(b, a), same == a, a == b}
}

// n4 checks ParseKind on spellings near the three names, String on any kind, and DueDate.
func n4(r *rng) any {
	name := []byte(r.pick([]string{"book", "dvd", "magazine", "books", "cd", "", "boo k", "unknown", "dvd."}))
	for i := range name {
		if r.n(2) == 0 && 'a' <= name[i] && name[i] <= 'z' {
			name[i] -= 'a' - 'A'
		}
	}
	s := pad(r) + string(name) + pad(r)
	k, err := lending.ParseKind(s)
	other := lending.ItemKind(r.between(-2, 5))
	from := day(r)
	due, dueErr := lending.DueDate(other, from)
	return []any{s, int(k), kinds(err), int(other), other.String(), from.String(), shown(due, dueErr), kinds(dueErr)}
}

// n5 checks Fine for returns from 10 days early to 90 days late, and a few far apart.
func n5(r *rng) any {
	k, due := kind(r), day(r)
	returned := due.AddDays(r.between(-10, 90))
	if r.n(10) == 0 {
		returned = due.AddDays(r.between(-2000, 2000))
	}
	fine, err := lending.Fine(k, due, returned)
	return []any{int(k), due.String(), returned.String(), int64(fine), kinds(err)}
}

// n6 checks Renew around the due date, with renewal counts from -1 to 3, holds and returns.
func n6(r *rng) any {
	k := kind(r)
	if r.n(3) == 0 {
		k = lending.Book
	}
	due := day(r)
	l := lending.Loan{Patron: "Ada", ISBN: "9780306406157", Title: "Notes", Kind: k,
		Borrowed: due.AddDays(-r.between(7, 21)), Due: due}
	if r.n(8) == 0 {
		returned := due.AddDays(r.between(-5, 5))
		l.Returned = &returned
	}
	today := due.AddDays(r.between(-25, 4))
	renewals, hold := r.between(-1, 3), r.n(6) == 0
	next, err := lending.Renew(l, today, renewals, hold)
	return []any{view(l), today.String(), renewals, hold, shown(next, err), kinds(err)}
}

// n7 checks ParseLoan on lines near the format, each with at most one fault.
func n7(r *rng) any {
	borrowed := day(r)
	returned := ""
	if r.n(2) == 0 {
		returned = borrowed.AddDays(r.between(0, 30)).String()
	}
	isbns := []string{"0-306-40615-2", "978 0 306 40615 7", "080442957X", "9780000000002", "0306406152"}
	fields := []string{r.pick([]string{"Ada Lovelace", "Bob", "cy", "D-2", "Ada  Byron"}), r.pick(isbns),
		r.pick([]string{"Notes on the Engine", "Space Film", "T", "Weekly  News"}),
		r.pick([]string{"book", "DVD", "Magazine", "mAgAzInE"}), borrowed.String(), returned}
	switch r.n(16) {
	case 0:
		fields = fields[:r.n(6)]
	case 1:
		fields = append(fields, r.pick([]string{"", "x"}))
	case 2:
		fields[0] = ""
	case 3:
		fields[2] = ""
	case 4:
		fields[1] = r.pick([]string{"0-306-40615-3", "", "X804429570", "978030640615", "ISBN 0306406152"})
	case 5:
		fields[3] = r.pick([]string{"cd", "", "books", "b ook"})
	case 6:
		fields[4] = r.pick([]string{"2026-02-29", "2026-1-05", "x", "", "0999-12-31", "2026/11/10"})
	case 7:
		fields[5] = r.pick([]string{"2026-02-29", "2026-1-05", "x", "2026-11-10T00:00"})
	case 8:
		fields[5] = borrowed.AddDays(-r.between(1, 3)).String()
	}
	for i := range fields {
		fields[i] = pad(r) + fields[i] + pad(r)
	}
	line := strings.Join(fields, "|")
	l, err := lending.ParseLoan(line)
	var got any = l == lending.Loan{}
	if err == nil {
		got = view(l)
	}
	return []any{line, got, kinds(err)}
}

// loans is 0-6 loans for a few patrons as of asOf, each with its own ISBN so that no two rows
// tie on every sort key: borrowed from 60 days before to 3 days after, due 0-25 days after
// borrowing, returned half the time up to 70 days after borrowing, and an invalid kind one
// time in fifteen.
func loans(r *rng, asOf calendar.Date) []lending.Loan {
	ids := []string{"9780306406157", "9780000000002", "9780804429573", "9791234567896", "9780000000019",
		"9780000000026", "9780000000033"}
	out := make([]lending.Loan, r.n(7))
	for i := range out {
		k := lending.ItemKind(r.between(1, 3))
		if r.n(15) == 0 {
			k = kind(r)
		}
		borrowed := asOf.AddDays(r.between(-60, 3))
		out[i] = lending.Loan{Patron: r.pick([]string{"Ada", "Ada", "Bob", "ada", "Ada "}), ISBN: isbn.ISBN(ids[i]),
			Title: r.pick([]string{"Notes", "Space Film", "A", "notes"}), Kind: k, Borrowed: borrowed,
			Due: borrowed.AddDays(r.between(0, 25))}
		if r.n(2) == 0 {
			returned := borrowed.AddDays(r.between(0, 70))
			out[i].Returned = &returned
		}
	}
	return out
}

func views(loans []lending.Loan) []any {
	out := []any{}
	for _, l := range loans {
		out = append(out, view(l))
	}
	return out
}

// n8 checks the overdue report.
func n8(r *rng) any {
	asOf := day(r)
	ls := loans(r, asOf)
	return []any{asOf.String(), views(ls), report.Overdue(ls, asOf)}
}

// n9 checks the patron statement.
func n9(r *rng) any {
	asOf := day(r)
	ls := loans(r, asOf)
	patron := r.pick([]string{"Ada", "Ada", "Bob", "ada", "Cy"})
	text, err := report.Statement(ls, patron, asOf)
	return []any{asOf.String(), views(ls), patron, text, kinds(err)}
}

func TestFalinksProperties(t *testing.T) {
	for _, p := range []struct {
		id    string
		check func(*rng) any
	}{{"N1", n1}, {"N2", n2}, {"N3", n3}, {"N4", n4}, {"N5", n5}, {"N6", n6}, {"N7", n7}, {"N8", n8}, {"N9", n9}} {
		r := &rng{seed}
		for i := 0; i < inputs; i++ {
			emit(p.id, i, func() any { return p.check(r) })
		}
	}
}
