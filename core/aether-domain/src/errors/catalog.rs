//! Il catalogo: una riga per errore, e tutto ciò che lo riguarda su quella riga.
//!
//! La macro genera cinque cose da un'unica dichiarazione — il tipo con i
//! parametri, il tipo senza, l'elenco completo, i metadati e la mappa inversa
//! dai codici del vecchio albero. È l'unico modo per cui «aggiungere un errore»
//! non possa voler dire «e dimenticarne una delle cinque»: era esattamente così
//! che nascevano le chiavi i18n orfane.

/// Raggruppamento per area di responsabilità: guida il routing dei log e la UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Domain {
    /// Rete.
    Net,
    /// Database.
    Db,
    /// Filesystem.
    Fs,
    /// Riproduzione.
    Playback,
    /// Libreria e scansione.
    Library,
    /// Metadati e arricchimento.
    Metadata,
    /// Scaricamento.
    Download,
    /// Importazione da Spotify.
    Spotify,
    /// Skin.
    Skin,
    /// Trasferimento diretto PC↔telefono.
    Transfer,
    /// Sincronizzazione con Drive.
    Sync,
    /// Impostazioni e segreti.
    Settings,
    /// Confine fra interfaccia e nucleo.
    Ipc,
    /// Guasti interni.
    Internal,
}

/// Quanto è grave.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Esito atteso, non un guasto — «nessun risultato», «annullato».
    Info,
    /// L'operazione è fallita ma l'app è integra. È il caso normale.
    Warning,
    /// Una funzionalità è compromessa finché non si interviene.
    Error,
    /// Lo stato del processo non è più affidabile.
    Fatal,
}

/// Come si decide se ritentare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryRule {
    /// Sempre.
    Always,
    /// Mai.
    Never,
    /// Dipende dallo stato HTTP.
    HttpStatus,
}

/// Un 5xx, un 408 o un 429 valgono un altro tentativo; un 4xx «colpa nostra» no.
#[must_use]
pub const fn http_retryable(status: u16) -> bool {
    status == 408 || status == 429 || status >= 500
}

macro_rules! catalogo {
    ($(
        $(#[$meta:meta])*
        $variant:ident = $code:literal, $domain:ident, $severity:ident, $retry:ident, $legacy:expr
        $(, { $($field:ident : $ty:ty),* $(,)? })? ;
    )+) => {
        /// Cosa è andato storto, con i suoi parametri.
        ///
        /// I parametri sono nel tipo e non in una mappa: sono i dati che l'i18n
        /// interpola, e se un messaggio dice «manca yt-dlp» il nome del binario
        /// deve essere un valore, non una stringa concatenata che nessuna
        /// traduzione può riordinare.
        // I singoli campi non hanno una riga di documentazione propria, e non è
        // una dimenticanza: sono i parametri del codice, il loro significato sta
        // nella descrizione della variante, e centocinquanta commenti «il
        // percorso» sopra un campo che si chiama `path` sono rumore che rende
        // più difficile leggere il catalogo — cioè l'unica cosa che questo file
        // deve permettere di fare bene.
        #[allow(missing_docs)]
        #[derive(Debug, Clone, PartialEq, Eq)]
        #[non_exhaustive]
        pub enum ErrorCode {
            $( $(#[$meta])* $variant $({ $($field: $ty),* })? , )+
        }

        /// Lo stesso catalogo senza parametri: serve a contare, aggregare i log
        /// e attraversare la mappa dei codici del vecchio albero.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[non_exhaustive]
        pub enum ErrorCodeKind {
            $( $(#[$meta])* $variant , )+
        }

        impl ErrorCode {
            /// Il codice senza i suoi parametri.
            #[must_use]
            pub const fn kind(&self) -> ErrorCodeKind {
                match self { $( Self::$variant { .. } => ErrorCodeKind::$variant, )+ }
            }
        }

        impl ErrorCodeKind {
            /// Ogni codice del catalogo, in ordine di dichiarazione.
            pub const ALL: &'static [Self] = &[ $( Self::$variant, )+ ];

            /// Il nome stabile, es. `net.offline`. Attraversa FFI e database.
            #[must_use]
            pub const fn code(self) -> &'static str {
                match self { $( Self::$variant => $code, )+ }
            }

            /// La chiave di traduzione, **derivata** dal codice.
            ///
            /// Derivata e non scritta a mano: è il meccanismo per cui non
            /// possono più esistere chiavi orfane.
            #[must_use]
            pub const fn i18n_key(self) -> &'static str {
                match self { $( Self::$variant => concat!("errors.", $code), )+ }
            }

            /// L'area di responsabilità.
            #[must_use]
            pub const fn domain(self) -> Domain {
                match self { $( Self::$variant => Domain::$domain, )+ }
            }

            /// La gravità.
            #[must_use]
            pub const fn severity(self) -> Severity {
                match self { $( Self::$variant => Severity::$severity, )+ }
            }

            /// Come si decide se ritentare.
            #[must_use]
            pub const fn retry_rule(self) -> RetryRule {
                match self { $( Self::$variant => RetryRule::$retry, )+ }
            }

            /// Il codice stringa che il vecchio albero mandava sul filo, dove
            /// esisteva. Le righe `downloads` già salvate lo contengono.
            #[must_use]
            pub const fn legacy_code(self) -> Option<&'static str> {
                match self { $( Self::$variant => $legacy, )+ }
            }
        }
    };
}

catalogo! {
    // ── net ─────────────────────────────────────────────────────────────────
    /// Nessuna connessione.
    NetOffline = "net.offline", Net, Warning, Always, None, { url: Option<String> };
    /// Scaduto il tempo massimo.
    NetTimeout = "net.timeout", Net, Warning, Always, None, { url: Option<String>, timeout_ms: Option<u64> };
    /// Risposta con stato di errore.
    NetHttp = "net.http", Net, Warning, HttpStatus, None, { status: u16, url: Option<String> };
    /// Il servizio ha chiesto di rallentare.
    NetRateLimited = "net.rateLimited", Net, Warning, Always, None, { service: Option<String>, retry_after_ms: Option<u64> };
    /// La risposta non ha la forma attesa.
    NetBadSchema = "net.badSchema", Net, Error, Never, None, { service: Option<String>, detail: Option<String> };
    /// Interruttore aperto: il servizio ha fallito troppe volte di seguito.
    NetCircuitOpen = "net.circuitOpen", Net, Warning, Always, None, { service: String, retry_after_ms: Option<u64> };

    // ── db ──────────────────────────────────────────────────────────────────
    // `Fatal`: senza database l'app non ha una libreria. Non deve però impedire
    // la registrazione dei canali verso l'interfaccia — era il difetto per cui
    // il renderer restava appeso sugli scheletri di caricamento per sempre.
    /// Il database non si apre.
    DbOpenFailed = "db.openFailed", Db, Fatal, Never, None, { path: Option<String> };
    /// Il file del database è corrotto.
    DbCorrupt = "db.corrupt", Db, Fatal, Never, None, { path: Option<String>, quarantined_as: Option<String> };
    /// Una migrazione è fallita.
    DbMigrationFailed = "db.migrationFailed", Db, Fatal, Never, None, { from: u32, to: u32, step: Option<String> };
    /// Il database è stato scritto da una versione più nuova: indietro non si torna.
    DbVersionAhead = "db.versionAhead", Db, Fatal, Never, None, { db_version: u32, app_version: u32 };
    /// Database occupato da un altro scrittore.
    DbLocked = "db.locked", Db, Warning, Always, None;
    /// Una query è fallita.
    DbQueryFailed = "db.queryFailed", Db, Error, Never, None, { detail: Option<String> };

    // ── fs ──────────────────────────────────────────────────────────────────
    /// Il file non c'è.
    FsNotFound = "fs.notFound", Fs, Warning, Never, None, { path: String };
    /// Permesso negato.
    FsPermissionDenied = "fs.permissionDenied", Fs, Error, Never, None, { path: String };
    /// Disco pieno.
    FsDiskFull = "fs.diskFull", Fs, Error, Never, None, { path: Option<String> };
    /// Il file è in uso da un altro processo.
    FsInUse = "fs.inUse", Fs, Warning, Always, None, { path: String };
    /// Percorso non valido per questo sistema.
    FsPathInvalid = "fs.pathInvalid", Fs, Error, Never, None, { path: String };
    /// Lettura fallita.
    FsReadFailed = "fs.readFailed", Fs, Warning, Always, None, { path: String, detail: Option<String> };
    /// Scrittura fallita.
    FsWriteFailed = "fs.writeFailed", Fs, Error, Always, None, { path: String, detail: Option<String> };

    // ── playback ────────────────────────────────────────────────────────────
    // Nel vecchio albero TUTTO questo arrivava come una stringa opaca: Howler
    // passava un numero (in interfaccia si leggeva «2») e il codice d'errore di
    // ExoPlayer veniva scartato. Ritentare la stessa traccia con lo stesso
    // decodificatore non cambia esito: la macchina a stati la salta, non la riprova.
    /// La decodifica è fallita.
    PlaybackDecodeFailed = "playback.decodeFailed", Playback, Warning, Never, None, { track_id: Option<i64>, format: Option<String> };
    /// La sorgente non è raggiungibile.
    PlaybackSourceUnavailable = "playback.sourceUnavailable", Playback, Warning, Never, None, { track_id: Option<i64>, path: Option<String> };
    /// Formato non supportato dal motore audio.
    PlaybackFormatUnsupported = "playback.formatUnsupported", Playback, Warning, Never, None, { format: Option<String> };
    /// Il dispositivo audio è sparito (cuffie staccate, scheda cambiata).
    PlaybackDeviceLost = "playback.deviceLost", Playback, Error, Always, None;
    /// Il sistema ha impedito l'avvio automatico.
    PlaybackAutoplayBlocked = "playback.autoplayBlocked", Playback, Info, Always, None;
    /// La riproduzione si è impantanata.
    PlaybackStalled = "playback.stalled", Playback, Warning, Always, None, { track_id: Option<i64>, position_ms: Option<u64> };
    /// Nessun motore audio disponibile.
    PlaybackEngineUnavailable = "playback.engineUnavailable", Playback, Fatal, Never, None;

    // ── library ─────────────────────────────────────────────────────────────
    /// La scansione è fallita.
    LibraryScanFailed = "library.scanFailed", Library, Warning, Always, None, { path: Option<String>, detail: Option<String> };
    /// Il brano non è in libreria.
    LibraryTrackNotFound = "library.trackNotFound", Library, Warning, Never, Some("TRACK_NOT_FOUND"), { track_id: Option<i64> };
    /// La playlist non esiste.
    LibraryPlaylistNotFound = "library.playlistNotFound", Library, Warning, Never, None, { playlist_id: Option<i64> };
    /// Una playlist con questo nome c'è già.
    ///
    /// Non è un dettaglio di database: l'identità di una playlist **è** il suo
    /// nome normalizzato (`PlaylistKey`), quindi due playlist con lo stesso nome
    /// sono la stessa playlist. Dirlo come conflitto, e non come violazione di
    /// vincolo, è la differenza fra «scegline un altro» e «errore SQL».
    LibraryPlaylistExists = "library.playlistExists", Library, Warning, Never, None, { name: String };
    /// Il nome della playlist non identifica niente.
    ///
    /// Vuoto, o fatto di sola punteggiatura: la normalizzazione lo riduce a una
    /// chiave vuota, che scontrerebbe con qualunque altro nome altrettanto
    /// vuoto.
    LibraryPlaylistNameInvalid = "library.playlistNameInvalid", Library, Warning, Never, None, { name: String };
    /// La playlist è automatica: la sua appartenenza la decidono le regole.
    ///
    /// Aggiungere un brano a mano a una playlist automatica non è vietato per
    /// principio: è che non avrebbe effetto. Ogni dispositivo la ricalcola dalle
    /// regole, e la riga aggiunta sparirebbe al primo ricalcolo senza che
    /// nessuno abbia sbagliato niente.
    LibraryPlaylistIsSmart = "library.playlistIsSmart", Library, Warning, Never, None, { playlist_id: Option<i64> };
    /// Le regole della playlist automatica non sono valide.
    LibrarySmartRulesInvalid = "library.smartRulesInvalid", Library, Warning, Never, Some("SMART_RULES_INVALID");
    /// Campo non valido in una regola.
    LibrarySmartFieldInvalid = "library.smartFieldInvalid", Library, Warning, Never, Some("SMART_FIELD_INVALID"), { value: String };
    /// Operatore non valido in una regola.
    LibrarySmartOpInvalid = "library.smartOpInvalid", Library, Warning, Never, Some("SMART_OP_INVALID"), { value: String };

    // ── metadata ────────────────────────────────────────────────────────────
    /// I tag non si leggono.
    MetadataTagReadFailed = "metadata.tagReadFailed", Metadata, Warning, Always, None, { path: Option<String> };
    /// I tag non si scrivono.
    MetadataTagWriteFailed = "metadata.tagWriteFailed", Metadata, Error, Always, None, { path: Option<String>, detail: Option<String> };
    /// Riletti dopo la scrittura, i tag non corrispondono.
    MetadataTagVerifyFailed = "metadata.tagVerifyFailed", Metadata, Error, Never, Some("TAG_VERIFY_FAILED"), { fields: String };
    /// Nessuna corrispondenza trovata.
    MetadataEnrichNoMatch = "metadata.enrichNoMatch", Metadata, Info, Never, Some("ENRICH_NO_MATCH");
    /// Trovata, ma incerta: decide una persona.
    MetadataEnrichNeedsReview = "metadata.enrichNeedsReview", Metadata, Info, Never, Some("ENRICH_NEEDS_REVIEW");
    /// Trovata e applicata.
    MetadataEnrichFound = "metadata.enrichFound", Metadata, Info, Never, Some("ENRICH_FOUND"), { what: String };
    /// MusicBrainz non risponde.
    MetadataMusicbrainzUnavailable = "metadata.musicbrainzUnavailable", Metadata, Warning, Always, Some("ENRICH_MB_UNAVAILABLE");
    /// Una passata di arricchimento è già in corso.
    ///
    /// `Info` e ritentabile, come `sync.busy`: non è un guasto, è che una
    /// passata automatica e un annullamento non devono intrecciarsi — il secondo
    /// riporterebbe indietro dei tag che la prima sta riscrivendo nello stesso
    /// istante, e quale dei due vincerebbe dipenderebbe dall'ordine in cui i due
    /// fili arrivano al file.
    MetadataEnrichBusy = "metadata.enrichBusy", Metadata, Info, Always, None;
    /// L'impronta acustica non è disponibile.
    MetadataFingerprintUnavailable = "metadata.fingerprintUnavailable", Metadata, Info, Never, None;

    // ── download ────────────────────────────────────────────────────────────
    // La ritentabilità qui SOSTITUISCE `classifyDownloadFailure`, che nel vecchio
    // albero aveva default divergenti fra desktop (`permanent`) e mobile
    // (`transient`). Ora è esplicita per ogni codice, una volta sola.
    /// URL non riconosciuto.
    DownloadUnrecognizedUrl = "download.unrecognizedUrl", Download, Warning, Never, Some("DL_UNRECOGNIZED_URL");
    /// URL malformato.
    DownloadInvalidUrl = "download.invalidUrl", Download, Warning, Never, Some("DL_INVALID_URL");
    /// Contenuto con limite d'età.
    DownloadAgeRestricted = "download.ageRestricted", Download, Warning, Never, Some("DL_AGE_RESTRICTED");
    /// Contenuto privato.
    DownloadPrivate = "download.private", Download, Warning, Never, Some("DL_PRIVATE");
    /// Contenuto non più disponibile.
    DownloadUnavailable = "download.unavailable", Download, Warning, Never, Some("DL_UNAVAILABLE");
    /// La sorgente ha chiesto di rallentare.
    DownloadRateLimited = "download.rateLimited", Download, Warning, Always, Some("DL_RATE_LIMITED");
    /// Rallentamento, con ritentativo già programmato.
    DownloadRateLimitedRetry = "download.rateLimitedRetry", Download, Info, Always, Some("DL_RATE_LIMITED_RETRY");
    /// Accesso negato dalla sorgente.
    ///
    /// Ritentabile, contro l'istinto. Un `403` da YouTube non è una proprietà
    /// del video — quelli che lo sono hanno un codice loro (privato, rimosso,
    /// con limite d'età) — ma del momento: arriva a ondate, legato all'indirizzo
    /// IP e al ritmo delle richieste. Chi lo produce
    /// (`aether_yt::scarica`) ha già camminato i tre profili di client di
    /// `argomenti::TENTATIVI` prima di arrendersi, quindi qui non si sta
    /// riprovando la stessa cosa: si sta riprovando *più tardi*, che è l'unica
    /// mossa rimasta e quella che di solito funziona.
    ///
    /// Con `Never` il brano resterebbe perduto per sempre, e il vecchio albero
    /// faceva proprio questo — non per scelta, ma perché `classifyDownloadFailure`
    /// non aveva il 403 in nessuna delle sue due liste e cadeva nel default
    /// `permanent`, mentre il commento tre righe sopra dichiarava di volerlo
    /// trattare come passeggero.
    DownloadForbidden = "download.forbidden", Download, Warning, Always, Some("DL_FORBIDDEN");
    /// Guasto di rete durante lo scaricamento.
    DownloadNetwork = "download.network", Download, Warning, Always, Some("DL_NETWORK");
    /// Fallito senza una causa più precisa.
    DownloadFailed = "download.failed", Download, Warning, Always, Some("DL_FAILED");
    /// La ricerca non ha prodotto risultati.
    DownloadNoResults = "download.noResults", Download, Info, Never, Some("DL_NO_RESULTS");
    /// I file prodotti non sono validi.
    DownloadInvalidFiles = "download.invalidFiles", Download, Warning, Never, Some("DL_INVALID_FILES");
    /// yt-dlp non ha risposto in tempo.
    DownloadYtdlpTimeout = "download.ytdlpTimeout", Download, Warning, Always, Some("DL_YTDLP_TIMEOUT");
    /// yt-dlp ha risposto in modo incomprensibile.
    DownloadYtdlpBadResponse = "download.ytdlpBadResponse", Download, Warning, Always, Some("DL_YTDLP_BAD_RESPONSE");
    /// Il pacchetto yt-dlp è corrotto.
    ///
    /// Ritentabile, e non è un dettaglio: uno zip scompattato a metà o un
    /// traceback di `zipimport` sono un guasto d'ambiente, non un URL cattivo.
    /// Il vecchio albero lo forzava a «transitorio» dentro la funzione di
    /// decisione; qui la scelta sta nel catalogo, dove la vedono tutti.
    DownloadYtdlpCorrupted = "download.ytdlpCorrupted", Download, Error, Always, Some("YTDLP_CORRUPTED");
    /// yt-dlp è occupato.
    DownloadYtdlpBusy = "download.ytdlpBusy", Download, Info, Always, Some("YTDLP_BUSY");
    /// Errore riportato da YouTube.
    DownloadYtError = "download.ytError", Download, Warning, Always, Some("DL_YT_ERROR"), { detail: String };
    /// spotdl è uscito con un codice di errore.
    DownloadSpotdlExit = "download.spotdlExit", Download, Warning, Always, Some("DL_SPOTDL_EXIT"), { code: String };
    /// La ricerca esterna è fallita.
    DownloadExternalSearchFailed = "download.externalSearchFailed", Download, Warning, Always, Some("EXT_SEARCH_FAILED");
    /// Manca un binario esterno.
    ///
    /// `dir` e `url` servono a dire dove metterlo e dove prenderlo: senza, il
    /// messaggio è una constatazione invece che un'istruzione.
    DownloadBinaryMissing = "download.binaryMissing", Download, Error, Never, Some("BINARY_MISSING"), { name: String, dir: Option<String>, url: Option<String> };

    // ── spotify ─────────────────────────────────────────────────────────────
    // Un dominio a sé e non `Download`: da qui non si scarica niente. Si legge
    // un elenco di brani da un servizio che non ha mai promesso di farcelo
    // leggere, e i modi in cui quella lettura fallisce non somigliano ai modi in
    // cui fallisce yt-dlp — somigliano a «hanno cambiato il sito».
    //
    // Il riconoscimento del link riusa `download.unrecognizedUrl` e
    // `download.invalidUrl`, che dicono già esattamente questo e hanno già la
    // loro traduzione: un secondo codice per la stessa frase sarebbe la cosa che
    // poi diverge.
    /// Non si è riusciti a ottenere il gettone anonimo del lettore web.
    ///
    /// Ritentabile perché la causa di gran lunga più comune è un intoppo di
    /// rete. Quando invece è la rotazione dei cifrari, ritentare non serve — ma
    /// non fa danno: chi chiama scende comunque al livello successivo, e
    /// `spotify_diagnostica` dice qual è dei due.
    SpotifyTokenUnavailable = "spotify.tokenUnavailable", Spotify, Warning, Always, None;
    /// Nessun livello è riuscito a leggere il contenuto.
    ///
    /// Non ritentabile: prima di arrivare qui si è già provato Pathfinder, la
    /// pagina incorporabile e oEmbed. Se hanno detto di no tutti e tre, dirlo
    /// una quarta volta non cambia la risposta.
    SpotifyResolveFailed = "spotify.resolveFailed", Spotify, Warning, Never, None;
    /// Il contenuto non è pubblico, o non esiste più.
    ///
    /// Distinto da quello sopra perché è l'unico caso in cui l'utente può fare
    /// qualcosa: rendere pubblica la playlist, o controllare il link.
    SpotifyNotPublic = "spotify.notPublic", Spotify, Warning, Never, None;
    /// L'elenco dei brani è arrivato più corto di quanto Spotify dichiari.
    ///
    /// Porta i due numeri perché il messaggio possa dire «142 su 300» invece di
    /// «alcuni brani»: una playlist importata a metà in silenzio è il guasto
    /// peggiore possibile qui, e la differenza fra saperlo e non saperlo è tutta
    /// in questi due valori.
    SpotifyTracklistTruncated = "spotify.tracklistTruncated", Spotify, Warning, Never, None, { letti: u32, attesi: u32 };
    /// L'archivio non si apre: non è uno zip, o è troncato.
    ///
    /// Non ritentabile. Un file scaricato a metà non si completa riprovando ad
    /// aprirlo, e la risposta utile è «riscaricalo», non «aspetta».
    SpotifyArchiveUnreadable = "spotify.archiveUnreadable", Spotify, Error, Never, None, { path: String, detail: Option<String> };
    /// Lo zip si apre, ma dentro non c'è niente che Aether sappia leggere.
    ///
    /// È il caso di chi sbaglia archivio — quello dei dati dell'account e quello
    /// della cronologia estesa arrivano separati, e in mezzo Spotify ne manda
    /// altri che non riguardano la musica. Porta l'elenco di quel che c'era
    /// dentro: senza, «archivio non riconosciuto» non dice a nessuno quale dei
    /// due file scaricati sia quello giusto.
    SpotifyArchiveEmpty = "spotify.archiveEmpty", Spotify, Warning, Never, None, { trovati: Vec<String> };
    /// Manca l'identificativo dell'applicazione Spotify.
    ///
    /// La via OAuth è l'unica cosa in Aether che chiede all'utente di
    /// registrare qualcosa da sé, e la ragione è fuori dal nostro controllo: le
    /// applicazioni in Development Mode accettano cinque utenti, e una chiave
    /// distribuita nel binario li esaurirebbe con i primi cinque che la usano.
    /// L'altra via — l'archivio — non chiede niente a nessuno, ed è il motivo
    /// per cui restano due.
    SpotifyAccountNotConfigured = "spotify.accountNotConfigured", Spotify, Warning, Never, None;
    /// Spotify ha risposto «no» a un account che ha dato il consenso.
    ///
    /// Il guasto più probabile di tutta questa funzione, e quello che senza un
    /// codice suo sembrerebbe un errore di Aether. In Development Mode significa
    /// una di due cose, e Spotify non dice quale: l'utente non è fra i cinque
    /// registrati nella dashboard, oppure **il proprietario dell'applicazione
    /// non ha più Premium** — da febbraio 2026 è un requisito, e quando
    /// l'abbonamento scade l'applicazione smette di funzionare senza nessun
    /// avviso.
    SpotifyAccountForbidden = "spotify.accountForbidden", Spotify, Warning, Never, None;
    /// Il consenso non vale più: si ricomincia dalla schermata di Spotify.
    ///
    /// Distinto da [`Self::SpotifyAccountForbidden`] perché la risposta è
    /// diversa: qui basta ricollegarsi, là c'è da sistemare qualcosa nella
    /// dashboard. Non ritentabile — un token rifiutato viene rifiutato anche la
    /// seconda volta.
    SpotifyAccountAuthExpired = "spotify.accountAuthExpired", Spotify, Warning, Never, None;
    /// La quota giornaliera dello sviluppatore è esaurita.
    ///
    /// Un `429` come gli altri, ma con `reason: "QUOTA_EXCEEDED"` dentro, e la
    /// differenza conta: un limite di frequenza passa aspettando qualche
    /// secondo, una quota esaurita no. Ritentarla vorrebbe dire tenere occupato
    /// chi guarda per il tempo di tre tentativi e poi dirgli la stessa cosa.
    /// Da luglio 2026 la quota si conta per **account sviluppatore**, non più
    /// per applicazione.
    SpotifyQuotaExceeded = "spotify.quotaExceeded", Spotify, Warning, Never, None, { retry_after_ms: Option<u64> };

    // ── skin ────────────────────────────────────────────────────────────────
    // Nessuno è ritentabile: un pacchetto non valido resta non valido. Portano
    // invece un dettaglio preciso, perché chi crea una skin deve sapere COSA è
    // stato rifiutato, non solo che è stato rifiutato.
    /// Il manifesto non è valido.
    SkinManifestInvalid = "skin.manifestInvalid", Skin, Warning, Never, None, { detail: Option<String> };
    /// Versione di formato non supportata.
    SkinFormatUnsupported = "skin.formatUnsupported", Skin, Warning, Never, None, { found: u32, supported: u32 };
    /// L'archivio è corrotto.
    SkinPackageCorrupt = "skin.packageCorrupt", Skin, Warning, Never, None, { detail: Option<String> };
    /// Una risorsa dell'archivio è stata rifiutata.
    SkinAssetRejected = "skin.assetRejected", Skin, Warning, Never, None, { asset: String, reason: String };
    /// L'archivio supera la dimensione ammessa.
    SkinTooLarge = "skin.tooLarge", Skin, Warning, Never, None, { bytes: u64, limit_bytes: u64 };
    /// Esiste già una skin con questo identificativo.
    SkinIdConflict = "skin.idConflict", Skin, Warning, Never, None, { id: String };
    /// Effetto sconosciuto.
    SkinUnknownEffect = "skin.unknownEffect", Skin, Warning, Never, None, { effect_type: String };
    /// Valore non valido per un token.
    SkinTokenInvalid = "skin.tokenInvalid", Skin, Warning, Never, None, { token: String, value: String };
    /// Lo scafale della skin non monta un widget senza il quale l'app non è usabile.
    SkinLayoutIncomplete = "skin.layoutIncomplete", Skin, Warning, Never, None, { widget: String };
    /// La skin non è installata.
    SkinNotFound = "skin.notFound", Skin, Warning, Never, None, { id: String };
    /// Le skin di serie non si modificano.
    SkinBuiltinReadOnly = "skin.builtinReadOnly", Skin, Info, Never, None, { id: String };

    // ── transfer ────────────────────────────────────────────────────────────
    /// L'accoppiamento è scaduto.
    TransferPairingExpired = "transfer.pairingExpired", Transfer, Info, Never, None;
    /// PIN sbagliato.
    TransferPinInvalid = "transfer.pinInvalid", Transfer, Warning, Never, None, { attempts_left: Option<u32> };
    /// Troppi tentativi di accoppiamento.
    TransferPairingRateLimited = "transfer.pairingRateLimited", Transfer, Warning, Always, None, { retry_after_ms: Option<u64> };
    /// L'altro dispositivo non risponde.
    TransferPeerUnreachable = "transfer.peerUnreachable", Transfer, Warning, Always, None, { host: Option<String> };
    /// L'altro dispositivo non è accoppiato.
    TransferPeerNotPaired = "transfer.peerNotPaired", Transfer, Warning, Never, None;
    /// C'è già un trasferimento in corso.
    TransferSessionBusy = "transfer.sessionBusy", Transfer, Info, Always, None;
    /// Trasferimento interrotto.
    TransferAborted = "transfer.aborted", Transfer, Warning, Always, None, { reason: Option<String> };
    /// I dati ricevuti non corrispondono all'impronta attesa.
    TransferIntegrityMismatch = "transfer.integrityMismatch", Transfer, Error, Always, None, { expected: String, actual: String };
    /// Metodo non supportato dall'altra estremità.
    TransferMethodNotSupported = "transfer.methodNotSupported", Transfer, Info, Never, Some("LAN_METHOD_NOT_SUPPORTED"), { method: String };

    // ── sync ────────────────────────────────────────────────────────────────
    /// L'autorizzazione Google è scaduta.
    SyncAuthExpired = "sync.authExpired", Sync, Warning, Never, None;
    /// Il file remoto non è leggibile.
    SyncRemoteCorrupt = "sync.remoteCorrupt", Sync, Error, Never, None;
    /// Conflitto irrisolvibile in automatico.
    SyncConflict = "sync.conflict", Sync, Warning, Never, None, { detail: Option<String> };
    /// C'è già un salvataggio o un ripristino in corso.
    ///
    /// `Info` e ritentabile, come `TransferSessionBusy`: non è un guasto, è che
    /// due operazioni che parlano con Drive non devono intrecciarsi. Chi la
    /// riceve riprova fra poco e trova il turno libero.
    SyncBusy = "sync.busy", Sync, Info, Always, None;

    // ── settings ────────────────────────────────────────────────────────────
    /// Le impostazioni salvate sono illeggibili.
    SettingsCorrupt = "settings.corrupt", Settings, Error, Never, None, { quarantined_as: Option<String> };
    /// Un segreto non è recuperabile dal portachiavi.
    SettingsSecretUnavailable = "settings.secretUnavailable", Settings, Warning, Never, None, { key: String };
    /// Last.fm non è configurato.
    SettingsLastfmNotConfigured = "settings.lastfmNotConfigured", Settings, Info, Never, Some("LASTFM_NOT_CONFIGURED");
    /// Nessuna richiesta Last.fm in attesa.
    ///
    /// Il consenso di Last.fm è a due tempi — si chiede un token, l'utente lo
    /// approva nel browser, poi lo si scambia per una sessione — e in mezzo
    /// l'applicazione può essere stata chiusa. Questo codice è quel «in mezzo»:
    /// non è un guasto, è che il primo tempo va rifatto.
    SettingsLastfmNoPendingToken = "settings.lastfmNoPendingToken", Settings, Info, Never, Some("LASTFM_NO_PENDING_TOKEN");
    /// ListenBrainz non è configurato.
    ///
    /// Un codice suo e non uno condiviso con Last.fm: quel che manca è diverso —
    /// là una chiave, un segreto e un consenso nel browser, qui un token
    /// incollato da una pagina — e il messaggio che porta l'utente a rimediare
    /// non può essere lo stesso.
    SettingsListenbrainzNotConfigured = "settings.listenbrainzNotConfigured", Settings, Info, Never, None;
    /// Il servizio di scrobbling ha rifiutato le credenziali.
    ///
    /// Non ritentabile, e qui è la distinzione che conta: una coda di ascolti
    /// che riprova all'infinito con una sessione revocata è una coda che non si
    /// svuota più. Chi lo riceve **scollega** e lo dice.
    SettingsScrobbleAuthRejected = "settings.scrobbleAuthRejected", Settings, Warning, Never, None, { service: String };
    /// Il servizio di scrobbling ha rifiutato l'ascolto.
    ///
    /// L'ascolto, non le credenziali: un artista vuoto, una data fuori
    /// dall'intervallo ammesso, un documento troppo grande. Rimandarlo darebbe
    /// lo stesso rifiuto per sempre, quindi non si rimanda — si dice quale e
    /// perché.
    SettingsScrobbleRejected = "settings.scrobbleRejected", Settings, Warning, Never, None, { service: String, detail: Option<String> };
    /// L'autenticazione Spotify è fallita.
    SettingsSpotifyAuthFailed = "settings.spotifyAuthFailed", Settings, Warning, Always, Some("SPOTIFY_AUTH_FAILED"), { status: String };

    // ── ipc ─────────────────────────────────────────────────────────────────
    /// Nessun gestore registrato per questo canale.
    IpcHandlerMissing = "ipc.handlerMissing", Ipc, Error, Never, None, { channel: String };
    /// Il nucleo non risponde.
    IpcBackendUnreachable = "ipc.backendUnreachable", Ipc, Error, Always, Some("BACKEND_UNREACHABLE");
    /// Gli argomenti non hanno la forma attesa.
    IpcPayloadInvalid = "ipc.payloadInvalid", Ipc, Error, Never, None, { channel: String, detail: Option<String> };

    // ── internal ────────────────────────────────────────────────────────────
    /// Il ripiego universale.
    ///
    /// Esiste perché la promessa del nucleo è che QUALSIASI guasto abbia una
    /// rappresentazione valida. Se compare nei log, è un candidato a diventare
    /// un codice proprio.
    InternalUnexpected = "internal.unexpected", Internal, Error, Never, None, { detail: Option<String> };
    /// Non ancora implementato.
    InternalNotImplemented = "internal.notImplemented", Internal, Error, Never, None, { what: String };
    /// Un'invariante è stata violata: da qui in poi lo stato non è affidabile.
    InternalInvariantViolated = "internal.invariantViolated", Internal, Fatal, Never, None, { what: String };
    /// Operazione annullata.
    ///
    /// `Info`, non `Error`: un annullamento voluto non deve inquinare la
    /// diagnostica. E non è ritentabile — ritentare ciò che è stato annullato è
    /// il contrario di quello che è stato chiesto. Nel vecchio albero arrivava
    /// come `new Error('Aborted')` e finiva nei log accanto ai guasti veri.
    InternalAborted = "internal.aborted", Internal, Info, Never, None, { what: Option<String> };
    /// Scadenza superata da un'operazione con limite di tempo.
    InternalTimeout = "internal.timeout", Internal, Warning, Always, None, { what: String, timeout_ms: u64 };
}

impl ErrorCode {
    /// Ritentare ha senso?
    ///
    /// Quasi sempre è una proprietà del solo codice. L'unica eccezione è HTTP,
    /// dove dipende dallo stato: e sta scritta qui, in un posto, invece che
    /// nella funzione di classificazione di ciascun chiamante — che è come il
    /// vecchio albero è finito con due default opposti.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self.kind().retry_rule() {
            RetryRule::Always => true,
            RetryRule::Never => false,
            RetryRule::HttpStatus => match self {
                Self::NetHttp { status, .. } => http_retryable(*status),
                _ => false,
            },
        }
    }

    /// La chiave di traduzione del messaggio da mostrare.
    #[must_use]
    pub const fn i18n_key(&self) -> &'static str {
        self.kind().i18n_key()
    }
}

impl ErrorCodeKind {
    /// Ritrova il codice a partire da quello che usava il vecchio albero.
    ///
    /// I codici a parametro viaggiavano come `PREFISSO:payload` (per esempio
    /// `BINARY_MISSING:yt-dlp:resources/bin`), quindi dopo il confronto esatto
    /// si prova anche la sola parte prima dei due punti. L'ordine conta: un
    /// codice esatto non deve essere scavalcato da un prefisso.
    #[must_use]
    pub fn from_legacy_code(raw: &str) -> Option<Self> {
        if let Some(exact) = Self::ALL.iter().find(|k| k.legacy_code() == Some(raw)) {
            return Some(*exact);
        }
        let prefix = raw.split(':').next().unwrap_or(raw);
        Self::ALL
            .iter()
            .find(|k| k.legacy_code() == Some(prefix))
            .copied()
    }
}
