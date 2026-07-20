# Page Stamper Plugin

A first-party example plugin demonstrating the PDF Platform plugin SDK.

## What it does

- Reads the document page count
- Adds a page number annotation to the bottom-right of each page
- All annotations are undoable (via the Command system)

## Capabilities used

- `ReadText` — read page count
- `Annotate` — submit annotation commands

## How to build

```bash
# From the plugin-sdk directory
cargo build --target wasm32-wasi --example page_stamper
```

## How to use

1. Place the compiled `.wasm` file in the plugins directory
2. Enable the plugin in Settings > Plugins
3. Grant `ReadText` and `Annotate` capabilities
4. Use Plugins > Page Stamper > Add Page Numbers from the menu
