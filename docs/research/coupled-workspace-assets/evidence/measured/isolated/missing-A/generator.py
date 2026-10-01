import pathlib
p = pathlib.Path("src/generated")
p.mkdir(exist_ok=True)
(p / "tag.ts").write_text('export const tag = "v2";\n')
(p / "schema.ts").write_text('export const schema = "key";\n')
