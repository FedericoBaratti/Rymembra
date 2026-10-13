/* Capitolo 8 — la rete: il client HTTP, la cadenza e la cache, i flussi posizionabili, OAuth e il portachiavi. */
(window.AETHER_DOCS = window.AETHER_DOCS || []).push({
  id: "network",
  title: "Network",
  lead: "One blocking HTTP client for every service, per-service pacing with a circuit breaker, seekable HTTP streams, and the OAuth pieces that do not depend on the provider.",
  entries: [
    {
      id: "http",
      title: "The HTTP client",
      src: ["core/aether-net/src/http.rs", "core/aether-net/src/immagine.rs", "core/aether-net/src/lib.rs"],
      body: [
        "`aether_net::Rete` wraps a `ureq` 3 agent (cheap to clone; clones share the connection pool). It sees only the error catalog — never the database — which is what makes «a lock held during a request» impossible to write.",
        {
          ul: [
            "**Status is not an error**: `http_status_as_error(false)` keeps 4xx/5xx bodies, because the body is the information (`invalid_grant` vs `invalid_client` from Google). `stato_a_errore` translates when the caller wants the default mapping (`net.http { status }`, `net.rateLimited`, …).",
            "**TLS** uses the operating system's trust store (`rustls-platform-verifier`), so a corporate proxy with a root installed in Windows works, and revoked roots go away with Windows updates. `https_only` everywhere.",
            "**User-Agent**: `Aether/<version> (+https://github.com/federicobaratti/aether)` — MusicBrainz policy asks for a contact. The update check is the exception and sends no Aether user agent ([Updates](#update)).",
            "**Body cap**: `TETTO_CORPO_DI_SERIE` = 10 MiB unless a client raises it (`con_tetto_corpo`). Images are recognised by magic bytes (`immagine::tipo`), never by `Content-Type`.",
          ],
        },
        {
          table: {
            head: ["Constructor", "Timeouts", "Redirects", "Used by"],
            rows: [
              ["`Rete::nuova(service, t)`", "global `t`", "`Liberi(10)`", "metadata, catalogs, most clients"],
              ["`Rete::per_prelievo`", "DNS 10 s, connect 15 s, headers 30 s, body 30 min", "default", "downloads"],
              ["`Rete::per_flusso`", "as above, body 90 s", "none until a gate is given", "`FlussoHttp`"],
              ["`Rete::verso(service, t, url)`", "global `t`", "none", "language models: plain `http` allowed only to `127.0.0.1`/`localhost`"],
            ],
          },
        },
        { h: "Redirect policy" },
        "`ureq`'s own redirects are disabled; each client declares `Rimandi`: `Nessuno` (a 3xx is a response — for every request that carries a secret or a body: OAuth, scrobbling, language models), `Liberi(n)` (followed by `ureq`, `Authorization` stripped), or `Sorvegliati { quanti, cancello }` — followed by hand, GET only, each hop approved by a `Cancello` (`Arc<dyn Fn(&str) -> bool>`) supplied by the caller: Drive allows at most 2 hops to Google hosts, streams 5 hops within the same catalog. Redirect statuses are 301, 302, 303, 307, 308.",
        { h: "Retries" },
        "`ritenta` / `ritenta_finche` retry only errors whose catalog rule says so (`AppError::is_retryable`: network down, timeouts, 408, 429, 5xx), backing off from 1 s and doubling up to `ATTESA_MASSIMA` = 30 s, honouring `Retry-After` and `X-RateLimit-Reset-In`. A 400 or 401 is never retried. `preleva` streams a body to a file in 64 KiB blocks with a cancel check; `flusso` streams a POST response (SSE); `intervallo_finche` fetches a byte range with 3 attempts inside a 45 s budget, checking cancellation every 50 ms.",
      ],
      see: ["rate-limits", "http-streams", "oauth"],
    },
    {
      id: "rate-limits",
      title: "Pacing, circuit breaker and response cache",
      src: ["core/aether-meta/src/cadenza.rs", "core/aether-meta/src/deposito.rs", "core/aether-meta/src/lib.rs", "core/aether-app/src/enrich.rs"],
      body: [
        "Retrying after a 503 is a second request that arrives too early. `aether_meta::Cadenza` is a gate **before** each request: one per service, it reserves the next slot under a short lock and sleeps outside it.",
        {
          table: {
            head: ["Service", "Minimum spacing"],
            rows: [
              ["MusicBrainz", "`RITMO_MUSICBRAINZ` = 1,100 ms (their limit is 1 req/s per IP)"],
              ["Cover Art Archive", "250 ms"],
              ["iTunes Search", "350 ms"],
              ["Deezer", "220 ms"],
              ["LRCLIB", "250 ms"],
              ["ListenBrainz Labs", "1,000 ms"],
            ],
          },
        },
        "**Circuit breaker**: after `GUASTI_PER_APRIRE` = 3 consecutive transport failures the service is considered down for `RAFFREDDAMENTO` = 5 min and calls fail at once with `net.circuitOpen`; a 404 counts as success. There is no half-open state: services are queried in bursts, and the next burst after the cooldown is the probe. Without it, an enrichment pass on an offline laptop would wait through 120 twenty-second timeouts.",
        "**Cache**: `Fornitori` remembers answers through the `Deposito` trait, implemented in `aether-app` by `DepositoSqlite` over the `enrich_cache` table on a secondary connection (expired rows are deleted on open). Lifetimes: search 7 days, release 30 days, «not found» 3 days, affinity 90 days, affinity «not found» 30 days. `json()` distinguishes `Ok(None)` («they don't have it», cacheable) from `Err` («unknown», not cached).",
      ],
      see: ["http", "enrich", "lyrics"],
    },
    {
      id: "http-streams",
      title: "Seekable HTTP streams",
      src: ["core/aether-net/src/flusso.rs", "apps/desktop/src-tauri/src/riproduzione/flusso.rs"],
      body: [
        "`FlussoHttp` is a `Read + Seek + Send + Sync` over HTTP Range requests, so the decoder can read tags at the end, return to the first frame and seek — without any file on disk (Jamendo's terms forbid caching; `TERMS.md` § 2). At most two windows are in memory and they are overwritten as playback advances.",
        {
          ul: [
            "Window `FINESTRA` = 256 KiB, doubling on sequential refills up to `FINESTRA_MASSIMA` = 2 MiB.",
            "After `ANTICIPO_DOPO` = 64 KiB of sequential reading, the next window is fetched on thread `flusso-anticipo`; a failed prefetch backs off 1 s → 16 s. Without prefetch, each window was a silence mid-song on a slow line.",
            "The constructor makes one request immediately to learn the total length (`Content-Range`) and remembers the node a redirect led to (`DaRete`), saving ≈ 900 ms per request on the Internet Archive.",
            "A seek elsewhere abandons the in-flight request (`pezzo_finche`) instead of competing for the line.",
            "`ascolta_annullo` takes the engine's cancel flag, so a new command interrupts a read waiting on the network.",
          ],
        },
        "**Errors must look like the network.** Symphonia only sees `io::Error`s; `da_app` maps `net.offline` to OS error 1231, `net.timeout` to 121, everything else to 64 (`ERROR_NETNAME_DELETED`) — numbers `errors::rete::e_di_rete` recognises — so a dropped stream becomes a retryable `playback.streamInterrupted` instead of «file damaged».",
      ],
      see: ["streaming", "http"],
    },
    {
      id: "oauth",
      title: "OAuth and the keychain",
      src: ["core/aether-oauth/src/lib.rs", "core/aether-oauth/src/pkce.rs", "core/aether-oauth/src/loopback.rs", "core/aether-oauth/src/portachiavi.rs", "core/aether-oauth/src/scadenza.rs", "core/aether-cloud/src/oauth.rs"],
      body: [
        "`aether-oauth` holds the parts of OAuth 2.0 that are the same for every provider, and no HTTP; provider details (endpoints, scopes, form fields) stay with the provider's crate (`aether-cloud` for Google).",
        {
          ul: [
            "**PKCE** (RFC 7636): verifier = base64url of 32 random bytes (43 characters), challenge S256; plus a random `state`.",
            "**Loopback** (`Attesa`): binds `127.0.0.1:0` — a port chosen by the OS, never fixed (guessable) and never `0.0.0.0` (the LAN). Non-blocking accept polled every 100 ms; request line ≤ 8 KiB, ≤ 64 headers; a wrong `state` gets a 400 and the wait continues; cancellable; replies with a small localized page. Consent always happens in the **system browser**: a webview able to draw `accounts.google.com` would be a phishing surface.",
            "**Keychain** (`Portachiavi` trait): `DiSistema` uses `keyring` (Windows Credential Manager) under the service `io.github.federicobaratti.aether`; `InMemoria` for tests; `Guasto` when the keychain is unreachable. It **never falls back** to `settings`.",
            "**Expiry**: tokens are refreshed `ANTICIPO` = 60 s before they expire.",
          ],
        },
        "Never written into an error (and therefore never into the log): the code verifier, authorization codes, access and refresh tokens. Keys stored in the keychain include `google.refresh_token`, `google.client_secret`, `lastfm.api_key`, `lastfm.segreto`, `lastfm.sessione`, `listenbrainz.token` and the per-profile language-model keys.",
      ],
      see: ["cloud", "scrobbling", "models"],
    },
  ],
});
