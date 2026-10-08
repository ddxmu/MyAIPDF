# MyAIPDF AI client

OpenAI-compatible `GET /models` and `POST /chat/completions`. Requests are explicit,
bounded and do not follow redirects. HTTPS is required except for localhost.
Credentials are never serialized or logged; optional persistence uses macOS Keychain.

The model returns advice and an allowlisted operation proposal. No network response
executes a PDF operation or saves a document. The caller must validate the proposal,
confirm it, and check that the same document revision is still open.

`ai_models` and `ai_chat` in printcraft-automation expose this client headlessly.
Run `cargo test -p printcraft-ai` for loopback HTTP and hostile-response tests.

The desktop catalogue includes current-document text/image editing, page/bookmark/form/
comment/link tools, watermarks, OCR and the other shipped PDF panels. File/credential
workflows use pdf_tool_open: a confirmed panel route, not arbitrary file access.
watermark_remove_text is reviewed locally against fresh, separable text candidates.
Read-only results stay out of model history until the user explicitly sends them.
Confirmed edits are atomic/undoable; undo/redo must be separate plans. No autosave,
shell, script execution, other-document access, password or private-key arguments.
