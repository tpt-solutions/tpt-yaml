# Templates

Small, realistic YAML snippets for trying `tpt-yaml-cli` (and the
underlying `tpt-yaml-core`/`-serde`/`-edit`/`-schema` crates) against
common real-world shapes, rather than only toy examples.

| File | Shape |
| ---- | ----- |
| `k8s-deployment.yaml` | A Kubernetes `Deployment` manifest snippet |
| `docker-compose.yaml` | A Docker Compose service definition |
| `ci-config.yaml` | A GitHub Actions CI workflow |
| `openapi.yaml` | An OpenAPI 3.0 document snippet, including a `$ref` |

Each is kept short (20-40 lines) and is real, valid YAML for its domain —
not a from-scratch invented shape — so it exercises anchors-free but
realistically nested block mappings/sequences, quoted and plain scalars,
and (in `openapi.yaml`) a JSON-Schema-shaped `components.schemas` section
that pairs naturally with `tpt-yaml-schema`.

## Running them through the CLI

Parse/validate (see `crates/tpt-yaml-cli/README.md` for the full
`check` reference):

```sh
cargo run -p tpt-yaml-cli -- check templates/k8s-deployment.yaml
cargo run -p tpt-yaml-cli -- check templates/docker-compose.yaml
cargo run -p tpt-yaml-cli -- check templates/ci-config.yaml
cargo run -p tpt-yaml-cli -- check templates/openapi.yaml
```

Reformat through the canonical pretty-printer:

```sh
cargo run -p tpt-yaml-cli -- fmt templates/docker-compose.yaml
```

Convert to JSON (goes through `tpt_yaml_serde::Value`):

```sh
cargo run -p tpt-yaml-cli -- convert templates/openapi.yaml --to json
```

Diff two versions of the same template as you edit one:

```sh
cargo run -p tpt-yaml-cli -- diff templates/k8s-deployment.yaml my-edited-copy.yaml
```

`openapi.yaml`'s `components.schemas.Widget` is itself a small JSON Schema
2020-12 document, so it also works directly as a `--schema` argument to
`check` against a YAML value shaped like a widget — see
`crates/tpt-yaml-schema/README.md` for the supported keyword subset.
