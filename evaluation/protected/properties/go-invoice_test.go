package invoice_test

// Differential properties I1-I9, one per ticket (#228 decisions 10-11, #336;
// evaluation/code-quality.json). Each property prints one line per seeded input; `quality`
// compares the lines with the frozen reference's. Only the API the oracle imports is used, and
// no property calls discount.Line, whose result the requirement leaves to the implementer: the
// discount is read through Totals with a zero tax Table, as the oracle reads it. Nothing the
// requirements leave open is printed (code-quality.json's `excluded`): error messages, the value
// returned with an error, nil against empty slices, the type of a conversion error, and Summary's
// field order, since Summary is read field by field. Each pool lists its valid values first.

import (
	"encoding/json"
	"errors"
	"fmt"
	"strconv"
	"strings"
	"testing"

	"invoice"
	"invoice/core"
	"invoice/discount"
	"invoice/fx"
	"invoice/parse"
	"invoice/render"
	"invoice/tax"
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
func (r *rng) between(lo, hi int64) int64 { return lo + int64(r.next()%uint64(hi-lo+1)) }

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

// failure is an error as the requirements fix it: its package's *Error and Line, or "other".
func failure(err error) any {
	var (
		c *core.Error
		d *discount.Error
		t *tax.Error
		x *fx.Error
		p *parse.Error
	)
	switch {
	case err == nil:
		return nil
	case errors.As(err, &c):
		return []any{"core", c.Line}
	case errors.As(err, &d):
		return []any{"discount", d.Line}
	case errors.As(err, &t):
		return []any{"tax", t.Line}
	case errors.As(err, &x):
		return []any{"fx", x.Line}
	case errors.As(err, &p):
		return []any{"parse", p.Line}
	}
	return "other"
}

// text joins 0-5 lines at \n, each padded with spaces, tabs or a \r, or blank, or a comment.
func text(r *rng, line func(*rng) string) string {
	lines := make([]string, r.n(6))
	for i := range lines {
		switch r.n(8) {
		case 0:
			lines[i] = r.pick([]string{"", " \t", "\r"})
		case 1:
			lines[i] = r.pick([]string{"# note", "  #x y"})
		default:
			pad := []string{"", "", " ", "\t", "  "}
			lines[i] = r.pick(pad) + line(r) + r.pick(pad) + r.pick([]string{"", "", "\r"})
		}
	}
	return strings.Join(lines, "\n")
}

// digits is n ASCII digits, so leading zeros occur.
func digits(r *rng, n int) string {
	b := make([]byte, n)
	for i := range b {
		b[i] = byte('0' + r.n(10))
	}
	return string(b)
}

// number is an optional -, 1-6 digits, and an optional . with 0 to places+1 digits.
func number(r *rng, places int) string {
	s := r.pick([]string{"", "", "", "-"}) + digits(r, 1+r.n(6))
	if r.n(2) == 0 {
		s += "." + digits(r, r.n(places+2))
	}
	return s
}

// garbled is 0-8 characters near the money and rate syntax.
func garbled(r *rng) string {
	const chars = "0123456789-.+% e"
	b := make([]byte, r.n(9))
	for i := range b {
		b[i] = chars[r.n(len(chars))]
	}
	return string(b)
}

var (
	amountEdges = []string{"92233720368547758.07", "92233720368547758.08", "-92233720368547758.08",
		"-92233720368547758.09", "-0", "-0.00", "007.05", "0.5", "1.", ".5", "99999999999999999999", "+1", " 1"}
	rateEdges = []string{"9223372036854.775807", "9223372036854.775808", "922337203685477.5807%",
		"922337203685477.5808%", "0%", "100%", "007.5%", "1.%", "%", "5%%", "-1", "0.000001", "12.3456%"}
	halves = []core.Rate{500000, 1500000, 250000, 125000, 82500, 1, 999999}
)

// i1 checks money and rates: parsing, printing, MulRate's and DivRound's rounding.
func i1(r *rng) any {
	amount := r.pick(amountEdges)
	if r.n(3) > 0 {
		amount = []string{number(r, 2), garbled(r)}[r.n(2)]
	}
	rate := r.pick(rateEdges)
	if r.n(3) > 0 {
		rate = number(r, 6)
		if r.n(2) == 0 {
			rate = strings.TrimPrefix(number(r, 4), "-") + "%"
		}
	}
	parsed := func(s fmt.Stringer, value int64, err error) any {
		if err != nil {
			return "error"
		}
		return []any{value, s.String()}
	}
	money, moneyErr := core.ParseMoney(amount)
	share, shareErr := core.ParseRate(rate)
	cents := core.Money(r.next())
	back, _ := core.ParseMoney(cents.String())
	factor := core.Rate(r.next() >> 1)
	factorBack, _ := core.ParseRate(factor.String())
	price := core.Money(r.between(-1_000_000_000, 1_000_000_000))
	by := halves[r.n(len(halves))]
	if r.n(2) == 0 {
		by = core.Rate(r.between(0, 3_000_000))
	}
	a, b := r.between(-1_000_000_000_000, 1_000_000_000_000), r.between(-20, 20)
	if r.n(2) == 0 {
		a = r.between(-50, 50)
	}
	divided := func() (out any) {
		defer func() {
			if recover() != nil {
				out = "panic"
			}
		}()
		return core.DivRound(a, b)
	}()
	return []any{parsed(money, int64(money), moneyErr), parsed(share, int64(share), shareErr),
		cents.String(), back, factor.String(), factorBack, price.MulRate(by), divided}
}

var (
	ids        = []string{"INV-7", "I", "A#1", "", "IN V", "\tX"}
	customers  = []string{"Acme Ltd", "C", " Bo ", "", " \t"}
	currencies = []string{"USD", "EUR", "JPY", "usd", "US", "USDX", "U5D"}
	skus       = []string{"MUG", "PEN", "BK-1", "GIFT", "TEA", "", "B K"}
	categories = []string{"books", "home", "toys", "food", "food-2", "clothing", "office", "", "Books", "fo od", "a_b", "é"}
)

// often is the first of pool most of the time, otherwise any of it.
func often(r *rng, pool []string) string {
	if r.n(6) > 0 {
		return pool[0]
	}
	return r.pick(pool)
}

// item is a line, valid unless bad: an invalid SKU, quantity, price or category now and then.
func item(r *rng, bad bool) core.LineItem {
	l := core.LineItem{SKU: r.pick(skus[:5]), Qty: r.between(1, 12), Unit: core.Money(r.between(0, 20000)),
		Category: r.pick(categories[:7])}
	if !bad || r.n(4) > 0 {
		return l
	}
	switch r.n(4) {
	case 0:
		l.SKU = r.pick(skus)
	case 1:
		l.Qty = r.between(-1, 0)
	case 2:
		l.Unit = core.Money(r.between(-100, -1))
	default:
		l.Category = r.pick(categories)
	}
	return l
}

// bill is an invoice of 0-6 lines; with bad, any header field or line may be invalid and SKUs
// may repeat.
func bill(r *rng, bad bool) core.Invoice {
	inv := core.Invoice{ID: ids[0], Customer: customers[0], Currency: currencies[r.n(3)]}
	if bad {
		inv.ID, inv.Customer = often(r, ids), often(r, customers)
		if r.n(6) == 0 {
			inv.Currency = r.pick(currencies)
		}
	}
	seen := map[string]bool{}
	for range r.n(7) {
		l := item(r, bad)
		if seen[l.SKU] && !bad {
			continue
		}
		seen[l.SKU] = true
		inv.Lines = append(inv.Lines, l)
	}
	if !bad && len(inv.Lines) == 0 {
		inv.Lines = append(inv.Lines, item(r, false))
	}
	return inv
}

// i2 checks Gross, Subtotal, Validate's first broken rule and Summarize.
func i2(r *rng) any {
	inv := bill(r, true)
	var gross []any
	for _, l := range inv.Lines {
		gross = append(gross, l.Gross())
	}
	s := invoice.Summarize(inv)
	return []any{gross, inv.Subtotal(), failure(inv.Validate()), s.ID, s.Customer, s.Subtotal}
}

var (
	ruleRates = []string{"10%", "5%", "12.5%", "0.1", "100%", "33.3333%", "0%", "7.77%", "101%", "-1%", "x"}
	amounts   = []string{"0.50", "5", "1.00", "0.05", "2.5", "0", "-1.00", "1.234"}
	counts    = []string{"1", "2", "3", "5", "10", "02", "0", "+1", "x"}
)

// rule is a discount rule line, valid unless bad.
func rule(r *rng, bad bool) string {
	shares, money, qty := ruleRates[:8], amounts[:5], counts[:6]
	if bad && r.n(3) == 0 {
		shares, money, qty = ruleRates, amounts, counts
	}
	limit := ""
	if r.n(3) == 0 {
		limit = " cap " + r.pick(money)
	}
	switch r.n(5) {
	case 0:
		least := ""
		if r.n(2) == 0 {
			least = " min " + r.pick(qty)
		}
		return "percent " + r.pick(categories[:4]) + " " + r.pick(shares) + least + limit
	case 1:
		tiers, at := []string{}, int64(0)
		for range 1 + r.n(3) {
			at += r.between(1, 4)
			tiers = append(tiers, fmt.Sprintf("%d:%s", at, r.pick(shares)))
		}
		if bad && r.n(4) == 0 {
			tiers = append(tiers, "2:5%")
		}
		return "tier " + r.pick(categories[:4]) + " " + strings.Join(tiers, " ") + limit
	case 2:
		return "bundle " + r.pick(skus[:4]) + " " + r.pick(qty) + " " + r.pick(qty)
	case 3:
		return "off " + r.pick(skus[:4]) + " " + r.pick(money)
	}
	if !bad {
		return "percent home 20%"
	}
	return r.pick([]string{"discount books 10%", "percent books", "percent books 10% min", "tier books",
		"bundle MUG 2", "bundle MUG 2 1 cap 5", "off PEN", "percent books 10% cap 5.00 min 2", "percent"})
}

// rules parses a valid rules text.
func rules(r *rng) []discount.Rule {
	parsed, _ := discount.ParseRules(text(r, func(r *rng) string { return rule(r, false) }))
	return parsed
}

// i3 checks ParseRules, Rule.String and each rule's discount, read through Totals with a zero
// tax Table.
func i3(r *rng) any {
	bad := r.n(2) == 0
	parsed, err := discount.ParseRules(text(r, func(r *rng) string { return rule(r, bad) }))
	var printed []string
	for _, one := range parsed {
		printed = append(printed, one.String())
	}
	var taken []any
	for range 4 {
		inv := core.Invoice{ID: "I", Customer: "C", Currency: "USD", Lines: []core.LineItem{item(r, false)}}
		taken = append(taken, invoice.Totals(inv, parsed, tax.Table{}).Discount)
	}
	return []any{failure(err), err != nil && parsed == nil, printed, taken}
}

// shuffled joins lines at \n in a random order, padded now and then, with blank and comment
// lines between them.
func shuffled(r *rng, lines []string) string {
	for i := len(lines) - 1; i > 0; i-- {
		j := r.n(i + 1)
		lines[i], lines[j] = lines[j], lines[i]
	}
	pad := []string{"", "", " ", "\t"}
	var out []string
	for _, line := range lines {
		if r.n(6) == 0 {
			out = append(out, r.pick([]string{"", " \t", "# note"}))
		}
		out = append(out, r.pick(pad)+line+r.pick(pad)+r.pick([]string{"", "", "\r"}))
	}
	return strings.Join(out, "\n")
}

var (
	taxRates   = []string{"5%", "20%", "0%", "8.875%", "0.05", "150%", "-1%", "x"}
	thresholds = []string{"10.00", "110", "0", "50.5", "-1.00", "1.234"}
)

// taxText is a valid tax table: a default half the time, rates for about half the categories,
// some over a threshold, and some exempt SKUs.
func taxText(r *rng) string {
	shares, over := taxRates[:6], thresholds[:4]
	var lines []string
	if r.n(2) == 0 {
		lines = append(lines, "default "+r.pick(shares))
	}
	for _, category := range categories[:7] {
		if r.n(2) == 0 {
			continue
		}
		line := "rate " + category + " " + r.pick(shares)
		if r.n(3) == 0 {
			line += " over " + r.pick(over) + " " + r.pick(shares)
		}
		lines = append(lines, line)
	}
	for _, sku := range skus[:5] {
		if r.n(5) == 0 {
			lines = append(lines, "exempt "+sku)
		}
	}
	return shuffled(r, lines)
}

// entry is a tax table line that may be invalid; categories, the default and SKUs repeat.
func entry(r *rng) string {
	shares, over := taxRates[:6], thresholds[:4]
	if r.n(3) == 0 {
		shares, over = taxRates, thresholds
	}
	switch r.n(5) {
	case 0, 1:
		line := "rate " + r.pick(categories[:5]) + " " + r.pick(shares)
		if r.n(2) == 0 {
			line += " over " + r.pick(over) + " " + r.pick(shares)
		}
		return line
	case 2:
		return "default " + r.pick(shares)
	case 3:
		return "exempt " + r.pick(skus[:4])
	}
	return r.pick([]string{"rate toys", "vat toys 5%", "rate toys 5% above 1 6%", "rate toys 5% over 1",
		"default", "exempt", "exempt A B", "rate toys 5% 6%"})
}

func taxes(r *rng) tax.Table {
	t, _ := tax.ParseTable(taxText(r))
	return t
}

// i4 checks ParseTable, Table.String, Rate and Line.
func i4(r *rng) any {
	source := taxText(r)
	if r.n(2) == 0 {
		source = text(r, entry)
	}
	t, err := tax.ParseTable(source)
	var taxed []any
	for range 4 {
		l := item(r, false)
		l.Unit = core.Money([]int64{0, 1000, 1001, 5050, 11000, 11001, r.between(0, 20000)}[r.n(7)])
		taxed = append(taxed, []any{t.Rate(l), t.Line(l, core.Money(r.between(0, 50000)))})
	}
	return []any{failure(err), t.String(), taxed}
}

var (
	codes   = []string{"USD", "EUR", "JPY", "GBP", "CHF", "usd", "EU", "EURO"}
	fxRates = []string{"0.5", "2", "1.1", "0.9137", "4", "0.25", "1", "3.000", "0", "5%", "-1", "x"}
)

// fxText is a valid exchange-rate table: each pair of USD, EUR, JPY, GBP and CHF half the
// time, in either direction, and each of them whole now and then.
func fxText(r *rng) string {
	var lines []string
	for i, a := range codes[:5] {
		if r.n(5) == 0 {
			lines = append(lines, "whole "+a)
		}
		for _, b := range codes[i+1 : 5] {
			if r.n(2) == 0 {
				continue
			}
			from, to := a, b
			if r.n(2) == 0 {
				from, to = b, a
			}
			lines = append(lines, from+" "+to+" "+r.pick(fxRates[:8]))
		}
	}
	return shuffled(r, lines)
}

// pair is an exchange-rate line that may be invalid; pairs and whole currencies repeat.
func pair(r *rng) string {
	code, factors := codes[:5], fxRates[:8]
	if r.n(3) == 0 {
		code, factors = codes, fxRates
	}
	switch r.n(4) {
	case 0:
		return "whole " + r.pick(code)
	case 1:
		return r.pick([]string{"USD EUR", "USD USD 1", "USD EUR 1 2", "whole", "whole JPY JPY", "rate USD GBP 1"})
	}
	return r.pick(code) + " " + r.pick(code) + " " + r.pick(factors)
}

func rates(r *rng) fx.Table {
	t, _ := fx.ParseTable(fxText(r))
	return t
}

// i5 checks ParseTable, Table.String and Convert's route and rounding.
func i5(r *rng) any {
	source := fxText(r)
	if r.n(2) == 0 {
		source = text(r, pair)
	}
	t, err := fx.ParseTable(source)
	var converted []any
	for range 4 {
		m := core.Money(r.between(-1_000_000, 1_000_000))
		from, to := r.pick(codes[:6]), r.pick(codes[:6])
		got, failed := t.Convert(m, from, to)
		if failed != nil {
			converted = append(converted, "error")
			continue
		}
		converted = append(converted, got)
	}
	return []any{failure(err), t.String(), converted}
}

var cells = []string{"apple", "kiwi", "é", "日本語", "", "a b", "12.50", "a\nb", "x\n", "\n", "long cell text"}

// i6 checks the table layout: widths, alignment, Max, multi-line cells and rules.
func i6(r *rng) any {
	cols := make([]render.Column, r.n(5))
	for i := range cols {
		// Titles are never empty, so the rule never ends in an empty column, whose trailing
		// spaces the requirement may or may not drop.
		cols[i] = render.Column{Title: r.pick([]string{"SKU", "Qty", "Long title", "é", "Amount"}),
			Right: r.n(2) == 0, Max: []int{0, 0, 0, 1, 2, 3, 5}[r.n(7)]}
	}
	grid := func(n int) [][]string {
		rows := make([][]string, r.n(n))
		for i := range rows {
			rows[i] = make([]string, r.n(6))
			for j := range rows[i] {
				rows[i][j] = r.pick(cells)
			}
		}
		return rows
	}
	rows := grid(5)
	return render.Table(cols, rows, grid(3))
}

// comment is no comment most of the time, or one after a space or tab.
func comment(r *rng) string { return r.pick([]string{"", "", "", " # note", "\t#x"}) }

// invoiceText is a valid invoice text, headers anywhere, or, half the time, with one more
// line that may break it.
func invoiceText(r *rng) string {
	lines := []string{
		r.pick([]string{"invoice INV-7", "invoice A#1"}) + comment(r),
		r.pick([]string{"customer Acme Ltd", "customer   Acme \t Ltd", "customer Bo#1"}) + comment(r),
		r.pick([]string{"currency USD", "currency EUR"}) + comment(r),
	}
	names := []string{"MUG", "PEN", "BK-1", "GIFT", "TEA", "TEA#2"}
	for i := range 1 + r.n(4) {
		j := i + r.n(len(names)-i)
		names[i], names[j] = names[j], names[i]
		fields := []string{names[i], r.pick([]string{"1", "3", "12", "007"}),
			r.pick([]string{"12.50", "4.99", "3", "0.5", "007.05", "0"}), r.pick(categories[:7])}
		lines = append(lines, strings.Join(fields, " ")+comment(r))
	}
	if r.n(2) == 0 {
		lines = append(lines, line(r))
	}
	return shuffled(r, lines)
}

// line is an invoice text line that may be invalid or repeat a header.
func line(r *rng) string {
	switch r.n(6) {
	case 0:
		return r.pick([]string{"invoice INV-7", "invoice", "invoice A B"}) + comment(r)
	case 1:
		return r.pick([]string{"customer Acme Ltd", "customer", "customer # nobody"}) + comment(r)
	case 2:
		return r.pick([]string{"currency USD", "currency usd", "currency", "currency USD EUR"}) + comment(r)
	case 3:
		return r.pick([]string{"BK-1 3 12.50", "BK-1 3 12.50 books extra", "BK-1 +3 12.50 books",
			"BK-1 -3 12.50 books", "BK-1 3.0 12.50 books", "BK-1 99999999999999999999 1 books",
			"BK-1 3 1.234 books", "BK-1 3 $1 books", "#tight", "x#y 1 1 books"})
	}
	fields := []string{r.pick([]string{"MUG", "CAR", "TEA#2"}), r.pick([]string{"1", "0", "9223372036854775807"}),
		r.pick([]string{"1", "-1.00", "0.5"}), r.pick(categories)}
	return strings.Join(fields, " ") + comment(r)
}

// i7 checks Parse's invoice and error, and Format on what parses cleanly.
func i7(r *rng) any {
	inv, err := parse.Parse(invoiceText(r))
	items := []any{}
	for _, l := range inv.Lines {
		items = append(items, []any{l.SKU, l.Qty, l.Unit, l.Category})
	}
	formatted := ""
	if err == nil {
		formatted = parse.Format(inv)
	}
	return []any{failure(err), inv.ID, inv.Customer, inv.Currency, items, formatted}
}

// i8 checks Totals on invoices it does not validate.
func i8(r *rng) any {
	inv := bill(r, false)
	if r.n(3) == 0 {
		inv.ID, inv.Customer, inv.Currency = r.pick(ids), r.pick(customers), r.pick(currencies)
		inv.Lines = append(inv.Lines, inv.Lines[0])
	}
	s := invoice.Totals(inv, rules(r), taxes(r))
	return []any{s.ID, s.Customer, s.Subtotal, s.Discount, s.Tax, s.Total}
}

// i9 checks the statement, its Summary and its errors.
func i9(r *rng) any {
	inv := bill(r, r.n(4) == 0)
	currency := inv.Currency
	if r.n(3) > 0 {
		currency = r.pick(append(codes[:5:5], "NOK"))
	}
	s, printed, err := invoice.Statement(inv, rules(r), taxes(r), rates(r), currency)
	// Validate's error is a *core.Error; a conversion error may be of any type.
	var invalid *core.Error
	failed := failure(err)
	if err != nil && !errors.As(err, &invalid) {
		failed = "error"
	}
	return []any{failed, s.ID, s.Customer, s.Subtotal, s.Discount, s.Tax, s.Total, s.Currency,
		s.Converted, printed}
}

func TestFalinksProperties(t *testing.T) {
	for _, p := range []struct {
		id    string
		check func(*rng) any
	}{{"I1", i1}, {"I2", i2}, {"I3", i3}, {"I4", i4}, {"I5", i5}, {"I6", i6}, {"I7", i7}, {"I8", i8}, {"I9", i9}} {
		r := &rng{seed}
		for i := 0; i < inputs; i++ {
			emit(p.id, i, func() any { return p.check(r) })
		}
	}
}
