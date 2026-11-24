# Generate a TypeScript client

Define a model in a trusted Python source file.

```python
from fury import BaseModel


class Order(BaseModel):
    amount_cents: int
```

Build the bindings from the [workspace README](../README.md), then generate the files with the Python environment that has the wheel installed.

```sh
FURY_PYTHON="$PWD/target/bindings/current/venv/bin/python" \
  ./target/debug/fury generate --lang-from python --lang-to ts \
  --src models.py --dst target/generated-models
```

The generator writes `order.ts` and `.fury_registry.bin`. Import the class from the generated file and publish the registry with the client. [WASM setup](../fury-ts/README.md)

Generation executes the selected Python source to register its models. Use `--src all` to generate from the registry in the current directory. Generate into a fresh output directory when you publish a release.
