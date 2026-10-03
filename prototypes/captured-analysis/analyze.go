// Throwaway evidence for issue 18: package-local function uses, not a dependency graph.
package main

import (
	"context"
	"encoding/json"
	"fmt"
	"go/ast"
	"go/types"
	"os"
	"path/filepath"
	"sort"

	"golang.org/x/tools/go/packages"
	"golang.org/x/tools/go/types/objectpath"
)

func main() {
	dir, capture, tags := os.Args[1], os.Args[2], os.Args[3]
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	if len(os.Args) > 4 {
		fmt.Println("loading")
		go func() { var s string; fmt.Fscan(os.Stdin, &s); cancel() }()
	}
	pkgs, err := packages.Load(&packages.Config{
		Dir: dir, Context: ctx, Mode: packages.LoadAllSyntax | packages.NeedModule,
		BuildFlags: []string{"-tags=" + tags}, Tests: false,
	}, "./...")
	out := map[string]any{"capture": capture, "tags": tags, "context_error": fmt.Sprint(ctx.Err())}
	if err != nil {
		out["load_error"] = err.Error()
	}
	owners, uses, errors := []map[string]any{}, []map[string]any{}, []string{}
	files := []string{}
	for _, p := range pkgs {
		for _, e := range p.Errors {
			errors = append(errors, e.Error())
		}
		for _, name := range p.CompiledGoFiles {
			rel, _ := filepath.Rel(dir, name)
			files = append(files, rel)
		}
		for _, f := range p.Syntax {
			for _, d := range f.Decls {
				fn, ok := d.(*ast.FuncDecl)
				if !ok {
					continue
				}
				obj := p.TypesInfo.Defs[fn.Name]
				key, pathError := objectpath.For(obj)
				start, end := p.Fset.Position(fn.Pos()), p.Fset.Position(fn.End())
				file, _ := filepath.Rel(dir, start.Filename)
				owner := p.PkgPath + "." + string(key)
				owners = append(owners, map[string]any{"key": owner, "name": fn.Name.Name,
					"file": file, "start": start.Offset, "end": end.Offset,
					"line": start.Line, "objectpath_error": fmt.Sprint(pathError), "ill_typed": p.IllTyped})
				// ponytail: only top-level fixture functions; methods/nested owners need a fuller syntax walk.
				ast.Inspect(fn, func(n ast.Node) bool {
					id, ok := n.(*ast.Ident)
					if !ok {
						return true
					}
					callee, ok := p.TypesInfo.Uses[id].(*types.Func)
					if !ok {
						return true
					}
					path, e := objectpath.For(callee)
					pos := p.Fset.Position(id.Pos())
					uses = append(uses, map[string]any{"owner": owner, "target": callee.Pkg().Path() + "." + string(path),
						"file": file, "offset": pos.Offset, "line": pos.Line, "objectpath_error": fmt.Sprint(e)})
					return true
				})
			}
		}
	}
	sort.Slice(owners, func(i, j int) bool { return fmt.Sprint(owners[i]) < fmt.Sprint(owners[j]) })
	sort.Slice(uses, func(i, j int) bool { return fmt.Sprint(uses[i]) < fmt.Sprint(uses[j]) })
	sort.Strings(files)
	out["owners"], out["uses"], out["errors"], out["compiled_files"] = owners, uses, errors, files
	json.NewEncoder(os.Stdout).Encode(out)
}
