# email-form-backend

> [!WARNING]
> **This project was generated with AI assistance.**
> The code, tests, configuration and this documentation were written by an AI coding assistant (Claude, via OpenCode). They have not had a full independent human security review, and the real SMTP connection (TLS and authentication) has not been tested against an actual mail server. Review the code and test it in your own environment before using it in production.

A small backend for website contact forms. The frontend posts the form as JSON, and the service validates it, optionally verifies a reCAPTCHA token, and sends the message to you by email over SMTP.

Built with [Actix Web](https://actix.rs), [lettre](https://lettre.rs) for SMTP and [reqwest](https://docs.rs/reqwest) for reCAPTCHA verification. TLS is handled by rustls, so OpenSSL is not required.

## Features

- `POST /submit` endpoint taking a JSON body.
- Input validation with length limits (the message limit is configurable).
- Email delivery over SMTP with STARTTLS, implicit TLS or no encryption.
- Optional reCAPTCHA v2 / v3 verification, including score, action and hostname checks.
- Optional CORS for frontends served from another origin.
- Optional per-IP rate limiting.
- Layered configuration: defaults, then `config.toml`, then environment variables.
- Internal error details are logged, never sent to clients.

## Quick start

You need a recent stable Rust toolchain (the project uses edition 2024) and a C compiler, which the `aws-lc-rs` crypto backend needs at build time.

1. Edit the `[smtp]` section of `config.toml`: set `host`, `from` and `to`.
2. Provide the credentials through the environment:

   ```sh
   export EMAIL_FORM_SMTP__USERNAME="user"
   export EMAIL_FORM_SMTP__PASSWORD="password"
   ```

3. Run it:

   ```sh
   cargo run --release
   ```

4. Send a test submission:

   ```sh
   curl -i -X POST http://127.0.0.1:8080/submit \
     -H 'Content-Type: application/json' \
     -d '{"name": "Ada", "email": "ada@example.com", "message": "Hello!"}'
   ```

reCAPTCHA is off in the default configuration, so no token is needed in this example. See [reCAPTCHA](#recaptcha) to turn it on.

For local development without a real mail server, run a catch-all SMTP server such as [MailHog](https://github.com/mailhog/MailHog) or [Mailpit](https://mailpit.axllent.org) and set `security = "none"` and `port = 1025` under `[smtp]`.

## API

### `POST /submit`

The request body must be JSON with the `Content-Type: application/json` header.

| Field             | Type   | Required                         | Rules                          |
| ----------------- | ------ | -------------------------------- | ------------------------------ |
| `name`            | string | yes                              | 1 to 100 characters            |
| `email`           | string | yes                              | valid email address, at most 256 characters |
| `message`         | string | yes                              | 1 to `form.max_message_length` characters (default 4096) |
| `recaptcha_token` | string | only when reCAPTCHA is enabled   | 1 to 4096 characters           |

Lengths are counted in characters, not bytes. Unknown fields are ignored. When reCAPTCHA is disabled, `recaptcha_token` is accepted and ignored.

### Responses

Error bodies are plain text.

| Status | Meaning | Body |
| ------ | ------- | ---- |
| `200 OK` | The email was sent. | empty |
| `400 Bad Request` | The content type is not `application/json`, the body is not valid JSON, a field is missing, or a field is invalid. | `Content type error`, the parse error, or `invalid fields: email, name` (field names, sorted) |
| `400 Bad Request` | The message is longer than the configured limit. | `message must be at most N characters` |
| `400 Bad Request` | `recaptcha_token` is missing while reCAPTCHA is enabled. | `recaptcha_token is required` |
| `400 Bad Request` | reCAPTCHA verification failed. | `recaptcha verification failed: <reason>` |
| `413 Payload Too Large` | The request body is larger than 2 MiB (the Actix default). | the size error |
| `429 Too Many Requests` | The client is over the rate limit. | `Too many requests, retry in Ns`, plus a `Retry-After` header |
| `502 Bad Gateway` | The reCAPTCHA service or the SMTP server could not be reached or rejected the request. | `Bad Gateway` |
| `500 Internal Server Error` | Any other server-side failure. | `Internal Server Error` |

Details for `500` and `502` responses are written to the log only.

Checks run in this order: JSON parsing and field validation, the message length limit, reCAPTCHA verification, then sending the email. A submission that fails an earlier step never reaches a later one, so invalid requests don't cost a call to Google or to your mail server.

### Email format

The email is plain text and goes from `smtp.from` to `smtp.to`:

- **Subject:** `New message from <name>`
- **Reply-To:** the submitter, as `Name <email>`, so replying goes straight to them.
- **Body:** `Name:` and `Email:` lines, a blank line, then the message.

Control characters (including newlines) are stripped from the name before it is used in the subject or `Reply-To`, which prevents header injection.

### Frontend example

```js
const response = await fetch("https://forms.example.com/submit", {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({ name, email, message, recaptcha_token: token }),
});

if (!response.ok) {
  console.error(await response.text());
}
```

If the frontend is on a different origin than the backend, enable [CORS](#cors).

## Configuration

Settings are read in this order, with later sources overriding earlier ones:

1. Built-in defaults.
2. `config.toml` in the working directory (optional).
3. Environment variables.

The repository ships a commented [`config.toml`](config.toml) listing every option.

### Environment variables

Variables are named `EMAIL_FORM_<SECTION>__<KEY>`, with **two** underscores between the section and the key and one after the `EMAIL_FORM` prefix. The section and key correspond to the TOML table and key, in upper case.

| TOML                       | Environment variable                      |
| -------------------------- | ----------------------------------------- |
| `server.port`              | `EMAIL_FORM_SERVER__PORT`                 |
| `smtp.password`            | `EMAIL_FORM_SMTP__PASSWORD`               |
| `recaptcha.secret`         | `EMAIL_FORM_RECAPTCHA__SECRET`            |
| `rate_limit.burst_size`    | `EMAIL_FORM_RATE_LIMIT__BURST_SIZE`       |
| `cors.allowed_origins`     | `EMAIL_FORM_CORS__ALLOWED_ORIGINS` (comma-separated) |

Keep secrets such as the SMTP password and the reCAPTCHA secret in environment variables instead of in `config.toml`.

### Reference

#### `[server]`

| Key    | Default       | Description |
| ------ | ------------- | ----------- |
| `host` | `"127.0.0.1"` | Address to bind to. Use `"0.0.0.0"` to accept outside connections, for example in a container. |
| `port` | `8080`        | Port to listen on. |

#### `[form]`

| Key                  | Default | Description |
| -------------------- | ------- | ----------- |
| `max_message_length` | `4096`  | Maximum message length in characters. |

#### `[smtp]`

| Key        | Default      | Description |
| ---------- | ------------ | ----------- |
| `host`     | required     | SMTP server host name. |
| `security` | `"starttls"` | `"starttls"` (port 587), `"tls"` for implicit TLS (port 465) or `"none"` (port 25). Use `"none"` only for local development servers. |
| `port`     | by security  | Overrides the default port of the chosen mode. |
| `username` | none         | Login name. Must be set together with `password`. |
| `password` | none         | Login password. Must be set together with `username`. |
| `from`     | required     | Sender address, such as `Contact Form <form@example.com>`. |
| `to`       | required     | Recipient address. |

`from` and `to` are checked at startup, so a malformed address stops the server immediately.

#### `[recaptcha]`

See [reCAPTCHA](#recaptcha). The section is optional, and `secret` is required when it exists.

| Key                 | Default                                            | Description |
| ------------------- | -------------------------------------------------- | ----------- |
| `secret`            | required                                           | Your reCAPTCHA secret key. |
| `verify_url`        | `https://www.google.com/recaptcha/api/siteverify`  | Verification endpoint. |
| `min_score`         | none                                               | v3: reject tokens with a lower score (0.0 to 1.0). |
| `expected_action`   | none                                               | v3: the token must have been issued for this action. |
| `expected_hostname` | none                                               | The token must have been issued for this host name. |

#### `[cors]`

See [CORS](#cors). The section is optional, and `allowed_origins` is required when it exists.

| Key               | Default  | Description |
| ----------------- | -------- | ----------- |
| `allowed_origins` | required | List of allowed origins, or `["*"]`. |

#### `[rate_limit]`

See [Rate limiting](#rate-limiting). The section is optional, and every key has a default.

| Key                   | Default | Description |
| --------------------- | ------- | ----------- |
| `burst_size`          | `5`     | Requests a client can send at once. Must be greater than zero. |
| `replenish_seconds`   | `60`    | Seconds until one more request is allowed. Must be greater than zero. |
| `trust_proxy_headers` | `false` | Identify clients by `X-Forwarded-For` / `Forwarded`. |

### Optional features

reCAPTCHA, CORS and rate limiting are all **off unless their section exists** in `config.toml` or at least one of their environment variables is set. A section that exists but is incomplete (for example `[recaptcha]` without a `secret`) stops the server from starting. A half-configured feature can't silently turn itself off.

## reCAPTCHA

The service verifies the token with Google's `siteverify` endpoint, which is shared by v2 and v3.

- **v2 (checkbox or invisible):** set only `secret`. A token passes if Google reports success.
- **v3:** also set `min_score` (for example `0.5`) and `expected_action`. The action must match the string your frontend passes to `grecaptcha.execute`.
- **Hostname check:** set `expected_hostname` to make sure tokens were issued for your site and not another site using the same key pair.

If you set `min_score`, `expected_action` or `expected_hostname` and Google's response lacks that value, the token is rejected.

reCAPTCHA Enterprise uses a different API and is not supported.

When reCAPTCHA is not configured, the server logs a warning at startup and does not verify submissions. Without it, rate limiting is your main protection against abuse.

## CORS

Leave CORS off when the frontend is served from the same origin as this backend. Turn it on by adding `[cors]` with the origins that may call the API:

```toml
[cors]
allowed_origins = ["https://example.com", "http://localhost:3000"]
```

- Each origin is a scheme and host with an optional port and **no trailing slash**. `*` allows any origin.
- Only the `POST` method and the `Content-Type` header are allowed, and browsers may cache preflight responses for an hour.
- Malformed origins stop the server at startup.

CORS is a browser mechanism, not access control. Requests from non-browser clients, such as curl, are not blocked by it. Use rate limiting and reCAPTCHA to deal with abuse.

## Rate limiting

Turn it on by adding `[rate_limit]`:

```toml
[rate_limit]
burst_size = 5
replenish_seconds = 60
```

- Only `POST` requests are counted, separately for each client IP address. CORS preflight requests are not counted.
- With the values above, a client can send 5 submissions immediately, then one more every 60 seconds.
- Requests rejected with `400` also count, since they cost the server work.
- Counters live in memory. They reset when the server restarts and are not shared between instances.

### Running behind a reverse proxy

By default, clients are identified by the address of the TCP connection. Behind a reverse proxy that is the proxy's address, so every visitor would share one budget. If you run behind a proxy you control, set `trust_proxy_headers = true` so the client address is read from `X-Forwarded-For` / `Forwarded`.

Enable it **only** behind a proxy that sets or overwrites those headers. Otherwise clients can send their own header and avoid the limit.

## Installing as a systemd service

The `deploy/` directory contains a hardened systemd unit and an install script. This needs a systemd-based Linux system and root access.

```sh
cargo build --release
sudo deploy/install.sh
```

The script installs:

| File | Purpose |
| ---- | ------- |
| `/usr/local/bin/email-form-backend` | the binary |
| `/etc/email-form-backend/config.toml` | the configuration (a copy of the default `config.toml`, mode 0644) |
| `/etc/email-form-backend/environment` | secrets and overrides as `KEY=value` lines (mode 0600, readable by root only) |
| `/etc/systemd/system/email-form-backend.service` | the unit |

The service is enabled but **not started**, because the default `[smtp]` settings are placeholders. Configure it, then start it:

```sh
sudoedit /etc/email-form-backend/config.toml      # set the [smtp] section
sudoedit /etc/email-form-backend/environment      # EMAIL_FORM_SMTP__PASSWORD=...
sudo systemctl start email-form-backend
journalctl -u email-form-backend -f
```

Pass `--now` to start the service right away. Run the script again after rebuilding to upgrade: it replaces the binary and unit, restarts a running service, and **never overwrites** an existing `config.toml` or `environment` file.

To remove the service, run `sudo deploy/install.sh --uninstall`. This keeps `/etc/email-form-backend`, and `--purge` removes it as well.

Things to know:

- The service reads `config.toml` from `/etc/email-form-backend`, its working directory. Edit the installed copy and not the one in the repository. Changes take effect after `systemctl restart email-form-backend`.
- It runs as a transient unprivileged user (`DynamicUser=yes`) with a restrictive sandbox: read-only file system, no access to home directories, only IPv4 and IPv6 sockets, and a filtered set of system calls. Check the result with `systemd-analyze security email-form-backend`.
- To listen on a port below 1024, uncomment the two `CAP_NET_BIND_SERVICE` lines in the unit. Alternatively, keep the default port and put a reverse proxy in front.
- It restarts automatically on failure after 5 seconds. A configuration error makes it fail on every start, so look at `journalctl` if it keeps restarting.
- Logs go to the journal. Change the level with `RUST_LOG` in the `environment` file.

## Logging

Logging uses `env_logger`. The default level is `info`, and `RUST_LOG` changes it:

```sh
RUST_LOG=debug cargo run
RUST_LOG=email_form_backend=debug,warn cargo run
```

What gets logged:

- **`info`:** startup (listen address, SMTP host and security mode, never credentials), one access-log line per request, and `submission delivered` for each sent email.
- **`warn`:** rejected reCAPTCHA verifications, and a missing reCAPTCHA configuration at startup.
- **`error`:** server-side failures (`500` and `502`) with the full cause.
- **`debug`:** validation rejections and details of the reCAPTCHA response.

Names, email addresses and message contents are never logged. At `debug` level, error details can appear in the middleware's own log lines, so use `info` in production.

## Development

```sh
cargo test      # run the tests
cargo clippy --all-targets
cargo fmt
```

The tests don't need network access or an SMTP server. reCAPTCHA is tested against a mock `siteverify` server started inside the test process, and email delivery against lettre's stub transport. The real SMTP connection (TLS and authentication) is not covered by the tests, so try it against your mail provider or a local MailHog before deploying.

## License

[MIT](LICENSE.md)
