"""Basic usage of the `tpt_yaml` Python extension module.

Build/install it first (from `crates/tpt-yaml-python`, into an active virtualenv):

    pip install maturin
    maturin develop            # or `maturin develop --release`

Then run this example from that same directory:

    python examples/basic.py
"""

import tpt_yaml

# --- loads: YAML text -> native Python object ---------------------------------

source = "name: Ada\ncount: 3\ntags: [a, b]\n"
value = tpt_yaml.loads(source)
print("loads() ->", value)
assert value == {"name": "Ada", "count": 3, "tags": ["a", "b"]}

# --- dumps: native Python object -> YAML text ----------------------------------

yaml_text = tpt_yaml.dumps({"name": "Ada", "count": 3, "tags": ["a", "b"]})
print("dumps() ->")
print(yaml_text)

# --- error handling -------------------------------------------------------------

try:
    tpt_yaml.loads("key: [1, 2")
except tpt_yaml.TptYamlError as e:
    print(f"parse failed as expected: {e}")
else:
    raise AssertionError("expected TptYamlError for malformed YAML")

print("basic.py OK")
