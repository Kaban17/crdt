#!/usr/bin/env sh
# Сборка интерактивной демонстрации: crdt-lab → wasm → единый web/index.html.
# Требует: rustup target wasm32-unknown-unknown, wasm-pack, python3.
set -e
cd "$(dirname "$0")/.."

wasm-pack build wasm --target no-modules --no-typescript --release --out-dir pkg

python3 - <<'EOF'
import base64, pathlib

root = pathlib.Path('.')
glue = (root / 'wasm/pkg/crdt_wasm.js').read_text()
wasm_b64 = base64.b64encode((root / 'wasm/pkg/crdt_wasm_bg.wasm').read_bytes()).decode()
tpl = (root / 'web/playground.template.html').read_text()

assert '/*__GLUE__*/' in tpl and '__WASM_B64__' in tpl, 'placeholders missing in template'
out = tpl.replace('/*__GLUE__*/', glue).replace('__WASM_B64__', wasm_b64)
(root / 'web/index.html').write_text(out)
print(f'web/index.html: {len(out)} bytes (wasm {len(wasm_b64)} b64 chars)')
EOF
