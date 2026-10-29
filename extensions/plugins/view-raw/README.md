# View Raw

Adds a **View Raw** action to the message right-click menu to inspect a message's
unformatted Markdown content and structured JSON metadata (IDs, author, message
type, reply/forward reference, mentions, attachments, embeds, stickers,
reactions, and poll availability).

## Capabilities

- `selected_message` — receives the selected message text from the message context menu.
- `timeline` — reads loaded message IDs, authors, raw content, edit state, and attachment counts.
- `message_details` — reads message type, reply target, mentions, attachment metadata, and reactions.
- `message_content` — reads embed summaries, stickers, message references, and poll availability.
- `clipboard_write` — proposes copying either the raw JSON payload or raw Markdown content to the clipboard.

## Build and package

From `extensions/`:

```powershell
cargo build --locked --release --target wasm32-unknown-unknown -p view-raw
python pack.py plugins/view-raw/manifest.json target/wasm32-unknown-unknown/release/view_raw.wasm plugins/packages/view-raw.serein-extension
```
