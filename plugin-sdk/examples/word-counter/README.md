# Word Counter Plugin

A first-party example plugin demonstrating the PDF Platform plugin SDK.

## What it does

- Reads text from all document pages
- Counts total words
- Displays the count in a side panel

## Capabilities used

- `ReadText` — read text content from document pages

## How to build

```bash
# From the plugin-sdk directory
cargo build --target wasm32-wasi --example word_counter
```

## How to use

1. Place the compiled `.wasm` file in the plugins directory
2. Enable the plugin in Settings > Plugins
3. Grant the `ReadText` capability
4. Open a document — the word count panel appears automatically
