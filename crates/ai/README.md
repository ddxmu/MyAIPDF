# MyAIPDF AI client

OpenAI-compatible `GET /models` and `POST /chat/completions`. Requests are explicit,
bounded and do not follow redirects. HTTPS is required except for localhost.
Credentials are never serialized or logged; optional persistence uses macOS Keychain.

The model returns advice and an allowlisted operation proposal. No network response
executes a PDF operation or saves a document. The caller must validate the proposal,
confirm it, and check that the same document revision is still open.

`ai_models` and `ai_chat` in printcraft-automation expose this client headlessly.
Run `cargo test -p printcraft-ai` for loopback HTTP and hostile-response tests.
