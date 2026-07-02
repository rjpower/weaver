"""Test fixture: echo the GH_TOKEN this watch subprocess received.

Watches run env-stripped; loom injects the operator's GH_TOKEN (Settings →
Environment) into the subprocess so github watches (pr-label, review-wait,
archive-merged) can shell out to `gh`. This program echoes the value it was
handed into its round summary, so an integration test can assert that injection
end-to-end.
"""

import os

from weaver_loom import Round

rnd = Round()
rnd.finish("token[" + (os.environ.get("GH_TOKEN") or "") + "]")
