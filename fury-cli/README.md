# fury-cli

The `fury` binary generates TypeScript classes and a schema registry from Python models. Build and verify the bindings using the [workspace README](../README.md).

```sh
cargo build --locked -p fury-cli
FURY_PYTHON="$PWD/target/bindings/current/venv/bin/python" \
  ./target/debug/fury generate --lang-from python --lang-to ts \
  --src examples/web/models.py --dst target/generated-models
```

Run from the workspace root with the verified wheel installed in the bindings environment. `--src` accepts a Python file with `fury.BaseModel` subclasses. The CLI imports the file and collects model names, so run generation on trusted source in a build environment you control. `--src all` instead reads the valid `.fury_registry.bin` in the current directory.

Output includes one class per selected model and a matching `.fury_registry.bin` in a separate destination. Generated classes import `@fury/wasm-core`. Bind that module name to the matching [WASM package](../fury-ts/) and regenerate the classes and registry together after schema changes. The code generator currently supports Python input and TypeScript output.
