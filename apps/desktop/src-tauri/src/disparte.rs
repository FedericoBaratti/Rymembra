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
//! lettura del lucchetto da un comando sincrono è senza limite. Per questo
//! **nessun comando che può aspettare è sincrono**, su nessuna piattaforma: ogni
//! `#[tauri::command]` su una `fn` non asincrona sta nell'elenco `IMMEDIATI`
//! della prova `nessun_comando_sincrono_puo_aspettare` — puro, atomico, un
//! `send` su un canale senza limite, una chiamata alla finestra, o un `Mutex`
//! privato letto punto per punto — con il suo motivo; tutti gli altri sono
//! `async fn` che fanno il lavoro con [`in_disparte`] (o [`comanda`]). La
//! pagina non aspetta nessun lucchetto, di nessun filo.
//!
//! # E l'ordine di arrivo?
//!
//! Un comando sincrono girava sul filo della finestra o del ponte, e due
//! comandi si eseguivano uno dopo l'altro **nell'ordine in cui la pagina li
//! aveva mandati**. Sul pool bloccante due comandi in fila su un lucchetto
//! occupato si risvegliano nell'ordine che sceglie il sistema operativo: un
//! volume, una stella, un fattore di zoom possono arrivare scambiati, e resta il
//! valore sbagliato. I comandi che scrivono uno stato dove l'ultimo vince
//! passano quindi da una **corsia** ([`crate::corsia`]), un filo con una coda
//! per risorsa — il lettore, la libreria — invece che da [`in_disparte`]. La
//! prova non lo pretende: lo dice il motivo scritto accanto a ogni comando, e
//! `corsia::prove` prova che la corsia tiene l'ordine anche sotto un
//! lucchetto occupato.
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
//! Fino a FC3 gli altri `#[tauri::command(async)]` su una `fn` non asincrona
//! restavano dove erano, con la scusa che «la maggior parte fa una query di
//! microsecondi»: una ottantina, di cui una cinquantina prendevano il lucchetto
//! della libreria o del lettore su un *worker* del runtime, e abbastanza
//! comandi fermi su un lucchetto fermano ogni risposta, anche quelle che non
//! toccano niente. Oggi sono tutti `async fn` in disparte, tranne quelli
//! dell'elenco `IMMEDIATI` della prova `nessun_comando_sincrono_puo_aspettare`,
//! che li pretende — e il suo elenco, al posto di un commento, è la fonte.

use rymembra_domain::errors::{AppError, ErrorCode};

use crate::errore::{Esito, errore};
use crate::nucleo::ComeNucleo;

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

/// [`in_disparte`] per un comando: il lavoro restituisce un [`Esito`] e quello
/// che torna alla finestra è lo stesso `Esito`, con la caduta del lavoro
/// diventata `internal.aborted`.
///
/// È la forma di quasi ogni comando che prima era sincrono: il corpo di
/// sempre, dentro una chiusura che possiede l'`AppHandle` e prende da lì gli
/// stati ([`stato_di`]), e questa funzione che la manda sul pool bloccante.
/// Per i comandi dove conta l'ordine di arrivo c'è
/// [`crate::corsia::Corsia::comanda`].
///
/// # Errori
///
/// Quelli del lavoro; `internal.aborted` se il lavoro cade.
pub async fn comanda<T: Send + 'static>(
    nome: &'static str,
    lavoro: impl FnOnce() -> Esito<T> + Send + 'static,
) -> Esito<T> {
    in_disparte(nome, lavoro).await.map_err(errore)?
}

/// Lo stato gestito `T`, preso dall'`AppHandle` dentro un lavoro che gira in
/// disparte, con l'errore già nella forma della finestra.
///
/// È [`crate::importa::gestito`] — `try_state` e non `state`, per la ragione
/// scritta su [`leggi_la_libreria`]: il pool bloccante può ritrovarsi vivo
/// mentre gli stati gestiti vengono lasciati cadere, e lì un panico non è la
/// risposta — con il nome dello stato al posto della causa a mano, e `Esito` al
/// posto di `AppError`, così nel corpo di un comando basta un `?`.
///
/// # Errori
///
/// `internal.aborted` se lo stato non c'è più: la finestra si sta chiudendo.
pub fn stato_di<T: Send + Sync + 'static>(app: &tauri::AppHandle) -> Esito<tauri::State<'_, T>> {
    let nome = std::any::type_name::<T>();
    crate::importa::gestito::<T>(app, nome.rsplit("::").next().unwrap_or(nome)).map_err(errore)
}

/// Lo stato `T` **del nucleo** (un handle `Clone`: la libreria, il lettore), con
/// l'errore già nella forma della finestra: la gemella di [`stato_di`] per gli
/// stati che il nucleo possiede e non solo Tauri.
///
/// # Errori
///
/// `internal.aborted` se lo stato non c'è.
pub fn stato_del_nucleo<T: Clone + Send + Sync + 'static>(app: &impl ComeNucleo) -> Esito<T> {
    let nome = std::any::type_name::<T>();
    app.come_nucleo()
        .gestito::<T>(nome.rsplit("::").next().unwrap_or(nome))
        .map_err(errore)
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
    app: impl ComeNucleo + Send,
    nome: &'static str,
    lettura: impl FnOnce(&mut crate::stato::Libreria) -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    let app = app.come_nucleo();
    in_disparte(nome, move || {
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

    /// I comandi che restano sincroni, ciascuno col suo motivo: (file sotto
    /// `src`, comando, perché è immediato).
    ///
    /// Un comando è qui solo se **non aspetta mai niente**: è puro, tocca solo
    /// bandiere atomiche, fa un `send` su un canale senza limite, è una chiamata
    /// alla finestra, oppure legge un `Mutex` privato i cui punti di presa sono
    /// stati letti uno per uno e non lo tengono attraverso I/O, un altro
    /// lucchetto o un'attesa. «Legge un `Mutex`» da solo non basta: era un
    /// comando di quella forma, `lontano_specchio_stato`, a fermare la pagina
    /// del telefono tre volte (collaudo del 18/11, vedi il `//!` di questo
    /// modulo).
    ///
    /// Va tenuto a mano, e la prova qui sotto lo pretende: un comando sincrono
    /// che non c'è nell'elenco la fa fallire, e una voce dell'elenco che non
    /// corrisponde più a un comando sincrono pure.
    const IMMEDIATI: &[(&str, &str, &str)] = &[
        // ── bandiere atomiche e canali senza limite ─────────────────────────
        (
            "avvio_con_windows.rs",
            "proposta_sottofondo_rimanda",
            "alza una bandiera atomica (`Rimando::imposta`)",
        ),
        (
            "esplora.rs",
            "esplora_annulla",
            "alza una bandiera atomica (`annullata`)",
        ),
        (
            "nuvola.rs",
            "nuvola_annulla",
            "alza una bandiera atomica (`annulla_collegamento`)",
        ),
        (
            "nuvola.rs",
            "nuvola_salva",
            "un `send` su un `mpsc::channel` senza limite: non aspetta",
        ),
        (
            "nuvola_musica.rs",
            "nuvola_conto_annulla",
            "alza una bandiera atomica (`annulla`)",
        ),
        (
            "procura.rs",
            "annulla_scarico",
            "alza una bandiera atomica (`StatoProcura::ferma`)",
        ),
        (
            "testi.rs",
            "testi_pannello",
            "un contatore atomico (`pannelli_aperti`)",
        ),
        (
            "testi.rs",
            "testi_ferma",
            "alza una bandiera atomica (`StatoTesti::ferma`)",
        ),
        (
            "maiusc.rs",
            "maiusc_all_avvio",
            "un `compare_exchange` su un atomico (`leggi_e_consuma`)",
        ),
        (
            "insieme/mod.rs",
            "insieme_cerca",
            "un `send` su un `mpsc::channel` senza limite: non aspetta",
        ),
        // ── puri ────────────────────────────────────────────────────────────
        (
            "ia.rs",
            "ia_operazioni",
            "pura: ricava le operazioni da un testo, senza stato, disco né rete",
        ),
        (
            "studio/registro.rs",
            "studio_registro",
            "pura: costruisce il registro dalle costanti di `rymembra-skin`",
        ),
        (
            "mobile/vassoio.rs",
            "vassoio_lingua",
            "non fa niente: sul telefono non c'è un menù (`let _ = (…)`)",
        ),
        // Le tre del validatore dello Studio sono pure — analizzano e
        // compilano un testo, senza disco né libreria — e ricordano l'ultimo
        // risultato in una cache (`Ricordi::o_calcola`, validazione.rs) il cui
        // `Mutex` si prende in due punti soli, per cercare e per inserire: il
        // calcolo sta fuori dal lucchetto, come dice il commento della funzione.
        (
            "studio/validazione.rs",
            "studio_valida",
            "pura (CPU) con una cache a `Mutex` privato: vedi sopra",
        ),
        (
            "studio/validazione.rs",
            "studio_migra",
            "pura (CPU) con una cache a `Mutex` privato: vedi sopra",
        ),
        (
            "studio/validazione.rs",
            "studio_compila_app",
            "pura (CPU) con una cache a `Mutex` privato: compila il solo testo della bozza, \
             senza leggere il disco (`compila_per_la_prova`)",
        ),
        // ── un `Mutex` privato, con i punti di presa letti ──────────────────
        (
            "chiusura.rs",
            "chiusura_da_scrivere",
            "scrive un bit nel `Mutex` privato di `CHIUSURA`: i cinque punti che \
             lo prendono (`Chiusura::stato`) leggono o scrivono un campo e basta, \
             nessuno attraversa I/O o un altro lucchetto. Resta sincrono anche per \
             l'ordine: un «sì» e un «no» di fila non devono scambiarsi, o la \
             chiusura salterebbe l'avviso di quel che non è stato scritto",
        ),
        (
            "chiusura.rs",
            "chiusura_scritto",
            "lo stesso `Mutex` di `chiusura_da_scrivere`, poi `finestra.close()` o \
             `esci_adesso`: due bandiere atomiche (`aggiornamenti::uscita_voluta`, \
             `spegnimento::chiedi`) e `app.exit`, che manda un messaggio al ciclo \
             degli eventi senza aspettarlo",
        ),
        (
            "ia.rs",
            "ia_ferma",
            "alza la bandiera atomica del turno sotto il `Mutex` privato `corrente`, \
             preso in quattro punti (`prendi_posto`, `ferma`, `occupato`, `lascia`) \
             per confrontare un numero: nessuno attraversa I/O",
        ),
        (
            "comandi.rs",
            "annulla_scansione",
            "legge `IN_CORSO` e alza una bandiera atomica: `IN_CORSO` si prende in \
             otto punti (`in_corso()`), tutti una lettura o un'assegnazione, e l'unico \
             in cui accanto c'è altro (`aspetta_il_turno_a_mano`) chiama due bandiere",
        ),
        (
            "comandi.rs",
            "scansione_avanzamento",
            "copia il valore di `IN_CORSO` (`Option` di `Copy`): vedi `annulla_scansione`",
        ),
        (
            "comandi.rs",
            "rimozioni_scarta",
            "azzera `IN_ATTESA`, preso solo per assegnare o per `take_if` \
             (`rimozioni_conferma`) e lasciato prima della scrittura",
        ),
        (
            "sorveglia.rs",
            "sorveglianza_stato",
            "clona il `quadro` della sorveglianza: i cinque punti che lo prendono \
             (`sorveglia.rs` e `sorveglia/filo.rs`) assegnano campi o clonano",
        ),
        (
            "manutenzione.rs",
            "manutenzione_salute",
            "clona la `Salute` sotto un `Mutex` privato preso in due punti \
             (`salute`, `scrivi_salute`) che clonano o assegnano",
        ),
        (
            "stato.rs",
            "fili_salute",
            "clona il registro dei fili (`salute()`), preso in `sorvegliato` solo \
             dopo il giro e in `salute_dei_fili`",
        ),
        (
            "stato.rs",
            "libreria_fase",
            "copia la fase di apertura (`Stato::fase`), un `Mutex` suo (`Apertura::fase`) \
             che nessuno tiene oltre una copia o un'assegnazione: chi aspetta l'apertura \
             (`aspetta_l_apertura_se`) dorme in `Condvar::wait_timeout` tenendo l'altro, \
             `Apertura::fili`, che questo comando non prende. Serve proprio alla \
             finestra appena nata, mentre la libreria si apre: non deve aspettarla",
        ),
        // ── la finestra ─────────────────────────────────────────────────────
        // Le API della finestra: sul filo principale `send_user_message`
        // esegue sul posto (`tauri-runtime-wry/src/lib.rs`, `send_user_message`),
        // fuori da lì manda un messaggio al ciclo degli eventi. I comandi della
        // finestra di Tauri stessi sono `async fn` (`tauri/src/window/plugin.rs`,
        // `getter!`/`setter!`), quindi chiamarle da un worker funziona; qui
        // restano sincroni perché sul desktop `comparsa::mostra` e compagne
        // devono vedere gli effetti nell'ordine in cui li chiedono, e sul
        // telefono sono vuote o mandano un `show()` che non aspetta.
        (
            "lib.rs",
            "pronto",
            "la finestra, più uno `swap` atomico (`COMPARSA.prendi`) e una bandiera \
             con un `send` senza limite (`aggiornamenti::finestra_pronta`)",
        ),
        ("lib.rs", "finestra_mostra", "la finestra"),
        (
            "lib.rs",
            "finestra_trascina",
            "la finestra (sul telefono: vuota)",
        ),
        (
            "lib.rs",
            "finestra_riduci",
            "la finestra (sul telefono: vuota)",
        ),
        (
            "lib.rs",
            "finestra_ingrandisci",
            "la finestra (sul telefono: vale `false`)",
        ),
        ("lib.rs", "finestra_ingrandita", "la finestra"),
        (
            "lib.rs",
            "finestra_schermo_intero",
            "la finestra (sul telefono: vale `false`)",
        ),
        (
            "lib.rs",
            "finestra_chiudi",
            "la finestra, più un bit sotto il `Mutex` privato di `CHIUSURA`",
        ),
    ];

    /// Quel che un comando immediato non può contenere: qualunque cosa che
    /// aspetti. Non è una prova che non aspetti — quella la fa chi ha scritto il
    /// motivo — ma impedisce che una voce dell'elenco resti lì dopo che il corpo
    /// ha preso un lucchetto o un disco.
    const ATTESE: &[&str] = &[
        "con_libreria",
        "con_lettore",
        "leggi_o_aspetta",
        "leggi_la_libreria",
        "leggi_a_parte",
        "in_disparte",
        ".lock(",
        ".recv(",
        "recv_timeout",
        "sleep(",
        ".join(",
        "block_on",
        "std::fs",
        "spawn",
    ];

    /// Nessun `#[tauri::command]` gira sul filo del ponte di Android o su quello
    /// della finestra se può aspettare (collaudo del 18/11, causa 1 e FC3).
    ///
    /// Un `#[tauri::command]` su una `fn` non asincrona gira **sul posto**: sul
    /// filo principale di Windows, sul filo del ponte JavascriptInterface di
    /// Android (vedi il `//!` di questo modulo). Se aspetta un lucchetto, il
    /// disco, la rete o un processo, tutta la pagina aspetta con lui. Quindi ogni
    /// comando sincrono sta in [`IMMEDIATI`], col suo motivo; tutti gli altri
    /// sono `async fn` e fanno il lavoro con [`in_disparte`](super::in_disparte)
    /// o con una delle corsie di [`crate::corsia`].
    ///
    /// Vale anche per i `#[tauri::command(async)]` su una `fn` non asincrona: la
    /// macro ne manda il corpo a `tauri::async_runtime::spawn`
    /// (`tauri-macros/src/command/wrapper.rs`, `respond_async_serialized`), che
    /// non è né il filo del ponte né quello della finestra, ma occupa un
    /// *worker* del runtime — uno per processore, prestati a **tutti** i
    /// comandi asincroni: abbastanza comandi fermi su un lucchetto e non parte
    /// più nessuna risposta, nemmeno quelle che non toccano niente. Per questo
    /// stanno nella stessa regola, e nello stesso elenco.
    #[test]
    fn nessun_comando_sincrono_puo_aspettare() {
        let tutti = sorgenti();
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut visti = 0_usize;
        let mut sincroni: Vec<(String, String, String)> = Vec::new();
        for (file, testo) in &tutti {
            let relativo = file
                .strip_prefix(&src)
                .unwrap_or(file)
                .to_string_lossy()
                .replace('\\', "/");
            for (dove, _) in testo.match_indices(concat!("\n#[tauri::", "command")) {
                let resto = &testo[dove + 1..];
                // Un attributo vero finisce con `]` o con `(`: una riga di una
                // stringa di un'altra prova che comincia con l'attributo, no.
                let dopo = resto[concat!("#[tauri::", "command").len()..]
                    .chars()
                    .next();
                if !matches!(dopo, Some(']' | '(')) {
                    continue;
                }
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
                let corpo = &resto[..resto.find("\n}\n").unwrap_or(resto.len())];
                sincroni.push((relativo.clone(), nome, corpo.to_owned()));
            }
        }
        assert!(visti >= 300, "i comandi non si sono trovati ({visti})");

        let mut da_convertire = Vec::new();
        for (file, nome, _) in &sincroni {
            if !IMMEDIATI.iter().any(|(f, n, _)| f == file && n == nome) {
                da_convertire.push(format!("{file}: {nome}"));
            }
        }
        da_convertire.sort();
        da_convertire.dedup();
        assert!(
            da_convertire.is_empty(),
            "comandi sincroni che non sono nell'elenco degli immediati, da rendere \
             `async fn` con `in_disparte` ({}): {da_convertire:#?}",
            da_convertire.len()
        );

        let mut vecchie = Vec::new();
        let mut con_attese = Vec::new();
        for (file, nome, motivo) in IMMEDIATI {
            assert!(!motivo.is_empty(), "{file}: {nome} non dice il perché");
            let corpi: Vec<&String> = sincroni
                .iter()
                .filter(|(f, n, _)| f == file && n == nome)
                .map(|(_, _, corpo)| corpo)
                .collect();
            if corpi.is_empty() {
                vecchie.push(format!("{file}: {nome}"));
                continue;
            }
            for corpo in corpi {
                for ago in ATTESE {
                    if corpo.contains(ago) {
                        con_attese.push(format!("{file}: {nome} contiene `{ago}`"));
                    }
                }
            }
        }
        assert!(
            vecchie.is_empty(),
            "voci degli immediati che non sono più comandi sincroni: {vecchie:#?}"
        );
        assert!(
            con_attese.is_empty(),
            "comandi «immediati» che aspettano: {con_attese:#?}"
        );
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
