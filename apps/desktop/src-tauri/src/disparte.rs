//! Il lavoro che aspetta il disco si fa in disparte.
//!
//! # Il difetto che questo modulo toglie
//!
//! `#[tauri::command]` senza altro genera un comando **sincrono**: il corpo gira
//! sul filo principale, quello che pompa i messaggi della finestra. Su una
//! condivisione di rete che non risponde, una `File::open` lì dentro vale i
//! quaranta secondi di timeout di Windows, e in quei quaranta secondi la
//! finestra non si ridisegna: il sistema la dichiara «non risponde», e chi
//! guarda termina il processo.
//!
//! `#[tauri::command(async)]` sembra la risposta e non lo è del tutto. Su una
//! `fn` **non** asincrona, `tauri-macros` avvolge il corpo in un blocco `async`
//! e lo manda a `tauri::async_runtime::spawn`: il corpo bloccante finisce su un
//! *worker* del runtime, e i worker sono tanti quanti i processori. Il filo
//! principale è salvo — ed è già molto — ma tre o quattro comandi fermi su una
//! share morta saturano il runtime, e da lì in poi non parte più **nessun**
//! comando asincrono, nemmeno quelli che non toccano il disco.
//!
//! [`in_disparte`] li manda invece sul pool bloccante, che esiste per questo e
//! cresce fino a centinaia di fili: un lavoro fermo lì non toglie il posto a
//! nessuno.
//!
//! # Su Android un comando sincrono non gira sul filo principale, e ferma la pagina
//!
//! Il «filo della finestra» di cui parla il paragrafo sopra è quello di Windows.
//! Su Android Tauri non usa il protocollo `ipc://` — `canUseCustomProtocol =
//! osName !== 'android'` in `scripts/ipc-protocol.js` — ma `window.ipc.postMessage`,
//! cioè il metodo `@JavascriptInterface` di `Ipc.kt` (wry), che passa dal JNI
//! `ipc` (`android/binding.rs`) e chiama il gestore **sul posto**, sul filo del
//! ponte Java. Un comando sincrono vi gira intero, e la pagina — che ha chiamato
//! un metodo di quel ponte — aspetta che torni: finché non torna non risponde ai
//! tocchi, non risponde a CDP (`Runtime.evaluate` non ritorna), non ridisegna, e
//! il filo Java principale resta tranquillo nel suo `Looper`, senza un ANR. È la
//! firma dei tre blocchi del collaudo del 18/11: la causa era un comando
//! sincrono (`lontano_specchio_stato`) che aspettava un lucchetto che nessuno
//! avrebbe più lasciato — la prova `un_giro_che_non_riesce_…` in
//! `lontano/specchio.rs`. Un comando `async` torna appena il lavoro è partito, e
//! la pagina non aspetta niente.
//!
//! Il tetto dei quattro secondi di [`crate::stato::con_libreria`] vale per il
//! filo della finestra di Windows e **non** per quello del ponte: su Android una
//! lettura del lucchetto da un comando sincrono è senza limite. Per questo i
//! comandi di `lontano/` sono tutti `async` — la prova
//! `nessun_comando_di_lontano_e_sincrono` — e gli altri comandi sincroni che
//! prendono la libreria restano il rischio che resta, limitato alla durata di
//! ogni presa.
//!
//! # Perché non basta `rymembra_app::scadenza::con_scadenza`
//!
//! Perché rispondono a due domande diverse. `con_scadenza` dice **quanto** si è
//! disposti ad aspettare, e alla scadenza lascia il lavoro indietro; questo dice
//! **dove** il lavoro gira, e aspetta comunque la fine. Servono tutte e due, e in
//! genere insieme: la scansione ha una scadenza per ogni file *e* gira qui,
//! perché senza la prima aspetterebbe per sempre un file solo, e senza la
//! seconda aspetterebbe occupando un posto che serve a tutti gli altri comandi.
//!
//! # Quali comandi passano di qui
//!
//! Ogni comando che aspetta qualcosa che non è la CPU: un file in una cartella
//! che l'utente ha scelto e che può stare su una share — la scansione, il
//! pannello «Cartelle», il profilo, il testo accanto al brano — e ogni comando
//! che parla con la rete: le importazioni da un link o da un archivio, la
//! ricerca nei cataloghi, i testi, la conversazione con l'IA, la cronologia
//! mandata a ListenBrainz. Un elenco scritto qui invecchierebbe al primo
//! comando spostato, ed era successo: la fonte è `grep 'in_disparte("'`.
//!
//! Il caso peggiore è il profilo: apre un file che l'utente ha scelto, e le
//! due destinazioni naturali di un profilo sono una chiavetta e una cartella di
//! rete. Un archivio da trecento megabyte scritto su una share che smette di
//! rispondere è la scansione lenta, moltiplicata per la quantità di byte.
//!
//! # E le letture lunghe della libreria
//!
//! Dalla 3.0 passano di qui anche i comandi che **leggono molto**, attraverso
//! [`leggi_la_libreria`]: gli elenchi di brani, album e artisti, la ricerca e i
//! suoi conteggi, la cronologia, la Home, l'avvio, e le playlist — l'elenco con
//! i conteggi delle intelligenti, i loro brani, l'anteprima delle regole. Non
//! aspettano il disco, ma su cinquantamila brani l'elenco degli artisti o una
//! ricerca per prefisso durano decine di millisecondi, e un comando sincrono
//! quei millisecondi li toglie al filo che ridisegna la finestra: una lista che
//! scorre a scatti mentre la pagina successiva arriva. La regola che ne esce è
//! semplice: un comando che legge più di una riga per chiave non è sincrono.
//!
//! Le liste e i conteggi che la finestra rilegge a ogni cambiamento della libreria
//! (gli artisti, i brani, gli album, la pagina di un artista, la ricerca) passano
//! da [`leggi_a_parte`], che non chiede nemmeno il lucchetto: una connessione di
//! sola lettura, come quella con cui il servizio «lontano» risponde agli altri
//! dispositivi. Sotto il lucchetto, rifatte ogni 3 s durante la lettura di un
//! catalogo, queste letture impedivano a una scansione, all'analisi e allo specchio
//! di scrivere per tutta la loro durata (misure in `stato/misure.rs`).
//!
//! # E i comandi che aspettano un brano, un'uscita o la sessione
//!
//! Dalla 3.2 passano di qui anche i comandi del lettore che fanno partire un
//! brano o aprono un'uscita — `suona`, `riprendi`, `prossimo`, i gesti della
//! coda, «Riprova», la scelta e la riapertura dell'uscita, il primo stato —
//! attraverso `riproduzione::col_lettore_in_disparte`: un file su una share che
//! si sveglia vale quindici secondi, un dispositivo venti, e quattro clic su
//! brani diversi bastavano a fermare tutti i worker. Lo stesso per i comandi
//! di «Ascolta insieme», che aspettano il filo della sessione fino a venti
//! secondi, e per «Tieni» dell'esplorazione, «Mostra nella cartella» e
//! l'importazione dal vecchio database. Il lavoro sta in un nucleo sincrono,
//! che chiamano anche i tasti multimediali e la sessione; la prova
//! `riproduzione::prove::nessun_comando_che_apre_un_brano_gira_su_un_worker`
//! cerca le attese lunghe nei comandi sincroni rimasti.
//!
//! Gli altri `#[tauri::command(async)]` restano dove sono: spostarli tutti
//! insieme sarebbe un gesto che nessuna prova copre, e la maggior parte fa una
//! query di microsecondi. Un elenco di quelli che toccano ancora il disco o la
//! rete qui invecchierebbe come quello dei comandi spostati — ed era già
//! falso: la fonte è `grep 'tauri::command(async)'` con il corpo sotto gli
//! occhi.

use rymembra_domain::errors::{AppError, ErrorCode};

/// Esegue `lavoro` sul pool bloccante e ne aspetta l'esito.
///
/// `nome` è quel che finisce nella causa dell'errore se il lavoro non arriva mai
/// a un risultato, quindi va scritto per essere letto in un diario: «scansione»,
/// non «task 3».
///
/// # Errori
///
/// `internal.aborted` quando il join fallisce, cioè quando il lavoro è caduto in
/// panico o il runtime è stato spento sotto di lui. Un errore e non un panico
/// nostro: il gancio dei panici ha già scritto cos'era successo, e chi ha chiesto
/// il comando merita una risposta invece di una finestra che si chiude.
pub async fn in_disparte<T: Send + 'static>(
    nome: &'static str,
    lavoro: impl FnOnce() -> T + Send + 'static,
) -> Result<T, AppError> {
    tauri::async_runtime::spawn_blocking(lavoro)
        .await
        .map_err(|err| {
            AppError::new(ErrorCode::InternalAborted {
                what: Some(nome.to_owned()),
            })
            .with_cause(err.to_string())
        })
}

/// Una lettura della libreria, in disparte: [`in_disparte`] più il lucchetto.
///
/// È la forma di quasi tutti i comandi che leggono molto, scritta una volta:
/// prendere lo stato dal registro di Tauri, prendere il lucchetto, leggere.
/// Prende un `AppHandle` e non uno `State` per la ragione scritta su
/// `comandi::cartelle_candidate`: quel che va sul pool dev'essere `'static`,
/// e un prestito dello stato lì dentro non entra.
///
/// # Errori
///
/// `internal.aborted` se lo stato non c'è più — la finestra si sta chiudendo —
/// o se la lettura cade; altrimenti quello della lettura.
pub async fn leggi_la_libreria<T: Send + 'static>(
    app: tauri::AppHandle,
    nome: &'static str,
    lettura: impl FnOnce(&mut crate::stato::Libreria) -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    in_disparte(nome, move || {
        use tauri::Manager as _;
        // `try_state` e non `state`: questo corpo gira sul pool bloccante, che
        // può ritrovarsi vivo mentre gli stati gestiti vengono lasciati cadere.
        let stato = app.try_state::<crate::stato::Stato>().ok_or_else(|| {
            AppError::new(ErrorCode::InternalAborted {
                what: Some(nome.to_owned()),
            })
            .with_cause("la libreria non è più fra gli stati gestiti")
        })?;
        crate::stato::con_libreria(&stato, lettura)
    })
    .await?
}

/// Una lettura della libreria **senza il suo lucchetto**, in disparte: su una
/// connessione di sola lettura aperta apposta ([`crate::stato::leggi_o_aspetta`]).
///
/// È la forma delle liste e dei conteggi che la finestra rilegge a ogni
/// cambiamento della libreria. Con la pagina di un artista aperta ogni
/// rilettura rifà gli artisti — un raggruppamento su tutta la tabella dei brani —
/// e il conto delle partecipazioni, e durante la lettura del catalogo di un altro
/// dispositivo succede ogni 3 s: sotto il lucchetto, per tutto il tempo, la
/// scansione, l'analisi e lo specchio non scrivevano (misure in
/// `stato/misure.rs`). In WAL chi legge da una connessione sua vede l'ultima
/// transazione chiusa, e non ferma né è fermato da chi scrive.
///
/// Per le letture che **solo leggono** — la funzione prende una `&Connection` —
/// e che non dipendono da quel che una connessione ha in sé e l'altra no (una
/// tabella temporanea, un'impostazione dell'apertura): `stato/prove_a_parte.rs`
/// le prova su una connessione nuova. Quel che legge e poi scrive resta su
/// [`leggi_la_libreria`].
///
/// # Errori
///
/// Come [`leggi_la_libreria`].
pub async fn leggi_a_parte<T: Send + 'static>(
    app: tauri::AppHandle,
    nome: &'static str,
    lettura: impl FnOnce(&rusqlite::Connection) -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    in_disparte(nome, move || {
        use tauri::Manager as _;
        let stato = app.try_state::<crate::stato::Stato>().ok_or_else(|| {
            AppError::new(ErrorCode::InternalAborted {
                what: Some(nome.to_owned()),
            })
            .with_cause("la libreria non è più fra gli stati gestiti")
        })?;
        crate::stato::leggi_o_aspetta(&stato, lettura)
    })
    .await?
}

#[cfg(test)]
mod prove {
    /// I sorgenti dei comandi, letti dal disco: tutti i file `.rs` sotto `src`.
    fn sorgenti() -> Vec<(std::path::PathBuf, String)> {
        fn visita(cartella: &std::path::Path, tutti: &mut Vec<(std::path::PathBuf, String)>) {
            let Ok(voci) = std::fs::read_dir(cartella) else {
                return;
            };
            for voce in voci.flatten() {
                let percorso = voce.path();
                if percorso.is_dir() {
                    visita(&percorso, tutti);
                } else if percorso.extension().is_some_and(|e| e == "rs")
                    && let Ok(testo) = std::fs::read_to_string(&percorso)
                {
                    tutti.push((percorso, testo));
                }
            }
        }
        let mut tutti = Vec::new();
        visita(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut tutti,
        );
        tutti
    }

    /// Nessun comando con un percorso scelto dall'utente gira su un *worker*
    /// del runtime (R10-5).
    ///
    /// Un `#[tauri::command(async)]` su una `fn` non asincrona manda il corpo
    /// su un *worker*, e i worker sono tanti quanti i processori: un archivio
    /// di Spotify su un NAS, una skin da una share che si sveglia, un `.m3u` da
    /// una chiavetta lenta li tenevano fermi, e con loro ogni comando asincrono
    /// della libreria. Chi legge o scrive dove l'utente ha scelto va in
    /// disparte, nella forma di `playlist::playlist_file_importa`.
    #[test]
    fn nessun_comando_con_un_percorso_gira_su_un_worker() {
        let tutti = sorgenti();
        assert!(tutti.len() > 50, "i sorgenti non si sono trovati");
        let mut sbagliati = Vec::new();
        for (file, testo) in &tutti {
            for (dove, _) in testo.match_indices(concat!("\n#[tauri::", "command(async)]")) {
                let resto = &testo[dove + 1..];
                let firma = &resto[..resto.find('{').unwrap_or(resto.len())];
                if firma.contains("async fn ") || !firma.contains("percorso:") {
                    continue;
                }
                let nome: String = firma
                    .find("fn ")
                    .map(|i| &firma[i + 3..])
                    .unwrap_or_default()
                    .chars()
                    .take_while(|c| *c == '_' || c.is_alphanumeric())
                    .collect();
                let file = file.file_name().unwrap_or_default().to_string_lossy();
                sbagliati.push(format!("{file}: {nome}"));
            }
        }
        assert!(
            sbagliati.is_empty(),
            "comandi con un percorso su un worker del runtime: {sbagliati:#?}"
        );
    }

    /// Nessun comando rilegge i file dei brani o conta la libreria intera sul
    /// filo della finestra o su un *worker* (R24-6, R24-7): «Dimentica
    /// l'arricchimento» rilegge un file per ogni brano arricchito, e lo stato
    /// dell'arricchimento e del backup conta tutta la libreria (o legge il
    /// portachiavi), fino a quattro secondi con una scansione che tiene il
    /// lucchetto.
    #[test]
    fn chi_rilegge_o_conta_la_libreria_non_sta_sul_runtime() {
        const REGOLE: [(&str, &[&str]); 3] = [
            (
                "arricchimento.rs",
                &[
                    concat!("rileggi_i", "_file("),
                    concat!("stato", "_ipc("),
                    concat!("concludi", "("),
                ],
            ),
            (
                "nuvola.rs",
                &[concat!("stato", "_ipc("), concat!("concludi", "(")],
            ),
            ("importa.rs", &[concat!("import_esterno::", "rapport")]),
        ];
        let tutti = sorgenti();
        let mut sbagliati = Vec::new();
        for (nome_file, aghi) in REGOLE {
            let Some((_, testo)) = tutti
                .iter()
                .find(|(percorso, _)| percorso.file_name().is_some_and(|n| n == nome_file))
            else {
                sbagliati.push(format!("{nome_file}: non trovato"));
                continue;
            };
            for (dove, _) in testo.match_indices(concat!("\n#[tauri::", "command")) {
                let resto = &testo[dove + 1..];
                let firma = &resto[..resto.find('{').unwrap_or(resto.len())];
                let corpo = &resto[..resto.find("\n}\n").unwrap_or(resto.len())];
                if !aghi.iter().any(|ago| corpo.contains(ago)) {
                    continue;
                }
                if firma.contains("async fn ") && corpo.contains(concat!("in_", "disparte(")) {
                    continue;
                }
                if firma.contains("async fn ") && corpo.contains(concat!("leggi_la", "_libreria("))
                {
                    continue;
                }
                let nome: String = firma
                    .find("fn ")
                    .map(|i| &firma[i + 3..])
                    .unwrap_or_default()
                    .chars()
                    .take_while(|c| *c == '_' || c.is_alphanumeric())
                    .collect();
                sbagliati.push(format!("{nome_file}: {nome}"));
            }
        }
        assert!(
            sbagliati.is_empty(),
            "comandi che rileggono o contano la libreria fuori dal pool bloccante: {sbagliati:#?}"
        );
    }

    /// Le liste e i conteggi che la finestra rilegge a ogni cambiamento della
    /// libreria non chiedono il suo lucchetto (FR2).
    ///
    /// Sono quelle che con la pagina di un artista aperta si rifanno ogni 3 s
    /// durante la lettura di un catalogo (`caricaVista`): gli artisti, il conto
    /// delle partecipazioni, le pagine dei brani, degli album e dei risultati.
    /// Una che tornasse a `leggi_la_libreria` riporterebbe la lettura sotto il
    /// lucchetto, e la prova è scritta sul sorgente perché nessun'altra se ne
    /// accorgerebbe: il risultato è lo stesso, cambia chi aspetta.
    #[test]
    fn le_liste_della_finestra_passano_dalla_connessione_a_parte() {
        const DA_PARTE: [&str; 12] = [
            "cerca",
            "cerca_conteggio",
            "preferiti",
            "album_artista",
            "brani_artista",
            "partecipazioni",
            "partecipazioni_conta",
            "brani",
            "album",
            "artisti",
            "conteggi",
            "brani_album",
        ];
        let tutti = sorgenti();
        let (_, testo) = tutti
            .iter()
            .find(|(percorso, _)| percorso.file_name().is_some_and(|n| n == "comandi.rs"))
            .expect("comandi.rs");
        for nome in DA_PARTE {
            let firma = format!("\npub async fn {nome}(");
            let dove = testo
                .find(&firma)
                .unwrap_or_else(|| panic!("il comando {nome} non c'è più"));
            let resto = &testo[dove + 1..];
            let corpo = &resto[..resto.find("\n}\n").unwrap_or(resto.len())];
            assert!(
                corpo.contains(concat!("leggi_a", "_parte(")),
                "{nome} non legge dalla connessione a parte"
            );
            assert!(
                !corpo.contains(concat!("leggi_la", "_libreria(")),
                "{nome} chiede ancora il lucchetto"
            );
        }
    }

    /// Nessun comando di `lontano/` è sincrono (collaudo del 18/11: la WebView
    /// ferma, tre volte).
    ///
    /// Su Android un comando sincrono gira sul filo del ponte Java e la pagina ne
    /// aspetta il ritorno: se aspetta un lucchetto, la pagina è ferma finché quel
    /// lucchetto non si apre — vedi il `//!` di questo modulo. Quelli di `lontano/`
    /// parlano con il collegamento, lo specchio e le code, che hanno ognuno i suoi
    /// lucchetti e i suoi fili: un comando `async` che passa da
    /// [`in_disparte`](super::in_disparte) li prende altrove. La prova è sul
    /// sorgente perché il difetto non si vede in nessun altro posto: un comando
    /// sincrono risponde uguale, finché non c'è chi tiene il lucchetto.
    #[test]
    fn nessun_comando_di_lontano_e_sincrono() {
        let tutti = sorgenti();
        let mut visti = 0_usize;
        let mut sincroni = Vec::new();
        for (file, testo) in &tutti {
            if !file.components().any(|c| c.as_os_str() == "lontano") {
                continue;
            }
            for (dove, _) in testo.match_indices(concat!("\n#[tauri::", "command")) {
                let resto = &testo[dove + 1..];
                let firma = &resto[..resto.find('{').unwrap_or(resto.len())];
                visti += 1;
                if firma.contains("async fn ") {
                    continue;
                }
                let nome: String = firma
                    .find("fn ")
                    .map(|i| &firma[i + 3..])
                    .unwrap_or_default()
                    .chars()
                    .take_while(|c| *c == '_' || c.is_alphanumeric())
                    .collect();
                let file = file.file_name().unwrap_or_default().to_string_lossy();
                sincroni.push(format!("{file}: {nome}"));
            }
        }
        assert!(
            visti >= 25,
            "i comandi di lontano/ non si sono trovati ({visti})"
        );
        assert!(
            sincroni.is_empty(),
            "comandi sincroni in lontano/: {sincroni:#?}"
        );
    }
}
