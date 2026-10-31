//! Le prove della riproduzione. Molte leggono `mod.rs` e i file di `SORGENTI` come
//! testo e cercano lì, per nome, i comandi e alcune funzioni: spostarli in un file
//! che non leggono le fa fallire, o le lascia passare senza guardare.

use super::seguito::aspetta_il_contatore;
use super::*;

/// L'annuncio del brano avviato non sposta la coda, nemmeno quando un gesto
/// l'ha già portata altrove: due scelte dalla coda a un secondo l'una
/// dall'altra mandavano l'host una voce oltre la seconda.
#[test]
fn l_annuncio_del_brano_avviato_non_sposta_la_coda() {
    // Avviato 42, la coda lo indica, in canna il dopo.
    assert_eq!(
        dopo_l_annuncio(42, Some(42), Some(42), Some(43)),
        DopoLAnnuncio::Resta
    );
    // Avviato 42; prima del suo primo campione un gesto ha scelto 44.
    assert_eq!(
        dopo_l_annuncio(42, Some(42), Some(44), Some(43)),
        DopoLAnnuncio::Resta
    );
    // `[A, A, B]`: l'inizio del primo A non è quello del secondo in canna.
    assert_eq!(
        dopo_l_annuncio(7, Some(7), Some(7), Some(7)),
        DopoLAnnuncio::Resta
    );
}

#[test]
fn gli_altri_annunci_spostano_la_coda_come_prima() {
    // Il gapless: niente di atteso, il motore attacca quello in canna.
    assert_eq!(
        dopo_l_annuncio(43, None, Some(42), Some(43)),
        DopoLAnnuncio::AvanzaDallaCanna
    );
    // `[A, A, B]`, il secondo A attaccato senza buchi.
    assert_eq!(
        dopo_l_annuncio(7, None, Some(7), Some(7)),
        DopoLAnnuncio::AvanzaDallaCanna
    );
    // Avviato 42, che il motore ha saltato: comincia 43, già in canna.
    assert_eq!(
        dopo_l_annuncio(43, Some(42), Some(42), Some(43)),
        DopoLAnnuncio::AvanzaDallaCanna
    );
    // Un entrante che la coda non indica, fuori dalla canna.
    assert_eq!(
        dopo_l_annuncio(50, None, Some(42), None),
        DopoLAnnuncio::Avanza
    );
    // Il brano che la coda indica, senza attese: niente da fare.
    assert_eq!(
        dopo_l_annuncio(42, None, Some(42), None),
        DopoLAnnuncio::Resta
    );
}

/// «Precedente» premuto mentre il brano nuovo si apre torna indietro: il
/// punto del motore è ancora quello del brano di prima, e oltre i tre
/// secondi faceva ricominciare il brano vecchio.
#[test]
fn precedente_durante_l_apertura_torna_indietro() {
    // Il motore suona ancora 41 a 3:20, la coda dice già 42.
    assert_eq!(
        dove_per_il_precedente(Some(41), Some(42), NESSUN_BRANO, 200_000),
        0
    );
    // Il motore ha già 42, ma la casella dice che lo si sta ancora aprendo.
    assert_eq!(dove_per_il_precedente(Some(42), Some(42), 42, 200_000), 0);
    // Motore vuoto.
    assert_eq!(dove_per_il_precedente(None, Some(42), NESSUN_BRANO, 0), 0);
}

#[test]
fn precedente_a_brano_avviato_misura_dal_motore() {
    assert_eq!(
        dove_per_il_precedente(Some(42), Some(42), NESSUN_BRANO, 200_000),
        200_000
    );
    // La casella è di un altro brano, che un gesto più nuovo ha superato:
    // questo suona, e i suoi secondi contano.
    assert_eq!(dove_per_il_precedente(Some(42), Some(42), 43, 5_000), 5_000);
}

/// Le strade che rimettono un brano a metà passano tutte da [`consegna`]:
/// l'«Annulla» della coda, «Riprova» e la ripresa dopo il dispositivo
/// ritrovato facevano ancora `suona` + `vai_a` (+ `pausa`), col «tic»
/// dall'inizio del brano e senza guardare la pausa premuta nel frattempo.
#[test]
fn le_riaperture_passano_dalla_consegna() {
    let riapertura = senza_prove(include_str!("riapertura.rs"));
    let dov_era = riapertura
        .find("fn riprendi_dov_era(")
        .map(|inizio| &riapertura[inizio..])
        .and_then(|resto| resto.find("\n}\n").map(|fine| &resto[..fine]))
        .expect("riprendi_dov_era");
    let corpi = [
        ("riprendi_dov_era", Some(dov_era)),
        // I nuclei: i comandi `async` li passano al pool bloccante e basta.
        (
            "riprova_corrente",
            corpo(include_str!("riapertura.rs"), "riprova_corrente_ora"),
        ),
        (
            "coda_ripristina",
            corpo(include_str!("coda.rs"), "coda_ripristina_ora"),
        ),
    ];
    for (nome, corpo) in corpi {
        let corpo = corpo.unwrap_or_else(|| panic!("manca {nome}"));
        assert!(corpo.contains("consegna("), "{nome} non passa da consegna");
        assert!(
            !corpo.contains("motore.vai_a(") && !corpo.contains("motore.pausa()"),
            "{nome} rimette la puntina a mano"
        );
    }
}

/// Un brano che non si apre per colpa sua si salta; uno a cui manca la
/// sorgente no: il brano dopo fallirebbe uguale, e la coda scorrerebbe via.
#[test]
fn si_salta_solo_un_brano_che_non_si_apre_per_colpa_sua() {
    let cartella = tempfile::tempdir().expect("cartella");
    let sparito = cartella.path().join("sparito.flac");
    let nel_vuoto = cartella.path().join("staccato").join("sparito.flac");
    let file = |percorso: &std::path::Path| {
        AppError::new(ErrorCode::FsNotFound {
            path: percorso.to_string_lossy().into_owned(),
        })
    };
    assert!(si_salta(&file(&sparito)), "il file manca, la cartella c'è");
    assert!(
        !si_salta(&file(&nel_vuoto)),
        "manca anche la cartella: è il disco a mancare"
    );
    // Com'è davvero un file che non si apre: `playback.sourceUnavailable`
    // col suo percorso. La stessa regola della cartella.
    let non_si_apre = |percorso: &std::path::Path| {
        AppError::new(ErrorCode::PlaybackSourceUnavailable {
            track_id: Some(7),
            path: Some(percorso.to_string_lossy().into_owned()),
        })
    };
    assert!(si_salta(&non_si_apre(&sparito)));
    assert!(!si_salta(&non_si_apre(&nel_vuoto)));
    // La riga che non c'è più.
    assert!(si_salta(&AppError::new(
        ErrorCode::PlaybackSourceUnavailable {
            track_id: Some(7),
            path: None,
        }
    )));
    assert!(si_salta(&AppError::new(ErrorCode::CloudNotFound)));
    assert!(si_salta(&AppError::new(
        ErrorCode::PlaybackFormatUnsupported { format: None }
    )));
    for sorgente in [
        ErrorCode::FsNetworkUnavailable { path: None },
        ErrorCode::CloudAuthExpired {
            servizio: "onedrive".to_owned(),
        },
        ErrorCode::CloudNotConnected {
            servizio: "dropbox".to_owned(),
        },
    ] {
        let err = AppError::new(sorgente);
        assert!(
            !si_salta(&err),
            "{} non è colpa del brano",
            err.code().kind().code()
        );
    }
}

/// Il filo che riprova parte solo dove la coda può avanzare da sé: mai per
/// chi segue una sessione (vedi `dopo_il_fermato`), e il salto stesso lo
/// richiede di nuovo, sotto il lucchetto.
#[test]
fn chi_segue_non_salta() {
    assert!(!dopo_il_fermato(false, true, false, Some(1)).avanza_sul_non_aperto);
    let lettore = senza_prove(include_str!("mod.rs"));
    let corpo = &lettore[lettore
        .find("fn riprova_o_salta(")
        .expect("riprova_o_salta")..];
    let corpo = &corpo[..corpo.find("\n}\n").expect("fine")];
    assert!(corpo.contains("insieme::segue(lettore)"));
    assert!(corpo.contains("SALTI_DI_FILA"));
}

/// I sorgenti dove stanno i comandi del lettore.
const SORGENTI: [(&str, &str); 6] = [
    ("mod.rs", include_str!("mod.rs")),
    ("coda.rs", include_str!("coda.rs")),
    ("fili.rs", include_str!("fili.rs")),
    ("suono.rs", include_str!("suono.rs")),
    ("uscite.rs", include_str!("uscite.rs")),
    ("riapertura.rs", include_str!("riapertura.rs")),
];

/// I comandi di trasporto e di coda: ognuno chiede la strada al gesto. Con
/// loro `imposta_ripeti`, che non è un comando ma la stessa porta per i
/// gesti dei membri.
const GESTI: [&str; 20] = [
    "imposta_ripeti",
    "suona",
    "radio",
    "pausa",
    "riprendi",
    "alterna",
    "prossimo",
    "precedente",
    "vai_a",
    "autoplay",
    "spegnimento",
    "coda_accoda",
    "coda_dopo",
    "coda_vai",
    "coda_togli",
    "coda_riordina",
    "coda_svuota",
    "coda_ripristina",
    "ripeti",
    "mescola",
];

/// I comandi che **non** sono gesti della sessione, ognuno col suo perché.
/// Un comando nuovo che non sta né qui né in [`GESTI`] fa fallire la prova:
/// è il modo di obbligare chi lo scrive a decidere.
const NON_GESTI: [&str; 24] = [
    // Domande, o preferenze di questo PC: volume, equalizzatore, uscita e
    // latenza sono di ciascuno, e restano locali sempre.
    "riproduzione_stato",
    "dissolvenza",
    "latenza",
    "volume",
    "equalizzatore",
    "eq_preset_elenco",
    "eq_preset_salva",
    "eq_preset_cancella",
    "normalizzazione",
    "normalizzazione_modo",
    "normalizzazione_modo_scegli",
    "normalizzazione_avanzamento",
    "spettro",
    "spettro_bande",
    "spettro_bande_scegli",
    "spettro_visibile",
    "spettro_visibile_scegli",
    "spettro_qualita",
    "spettro_qualita_scegli",
    "formato_visibile",
    "formato_visibile_scegli",
    "dispositivi_audio",
    "scegli_dispositivo_audio",
    "riapri_audio",
];

/// Più tre che toccano la coda senza essere gesti, e si difendono da sé:
/// le schede delle righe (le maniglie le risolve `ospiti`), la coda del
/// giro guidato (che in sessione non mette niente) e «Riprova» (che
/// riprende passando da [`riprendi`], o riapre il brano corrente).
const DIFESI_DA_SE: [&str; 3] = ["brani_per_id", "coda_prepara", "riprova_corrente"];

/// Il sorgente senza le sue prove: le prove nominano i comandi e le firme
/// come testo, e la ricerca le scambierebbe per codice.
fn senza_prove(testo: &str) -> &str {
    testo
        .split(
            "
#[cfg(test)]",
        )
        .next()
        .unwrap_or(testo)
}

/// Il corpo di `pub fn nome(`, dalla firma alla graffa che lo chiude a
/// inizio riga.
fn corpo<'a>(testo: &'a str, nome: &str) -> Option<&'a str> {
    let testo = senza_prove(testo);
    let inizio = [format!("pub fn {nome}("), format!("pub async fn {nome}(")]
        .iter()
        .find_map(|firma| testo.find(firma.as_str()))?;
    let resto = &testo[inizio..];
    let fine = resto.find("\n}\n")?;
    Some(&resto[..fine])
}

/// Chi ospita: ogni gesto che sostituisce o accorcia la coda di sessione
/// toglie dal registro le voci uscite, **dopo** aver cambiato la coda.
/// Senza, il registro cresceva di mille voci a ogni «suona» fino a
/// «Termina» (chi segue lo fa già in `insieme::specchia`).
#[test]
fn i_gesti_che_accorciano_la_coda_dimenticano_le_voci_uscite() {
    let lettore = senza_prove(include_str!("mod.rs"));
    let coda = senza_prove(include_str!("coda.rs"));
    for (testo, firma, cambio) in [
        (
            lettore,
            "pub fn suona_ora(",
            "lettore.coda.play_tracks(brani, indice, seme());",
        ),
        (coda, "pub fn coda_svuota(", "lettore.coda.clear();"),
        (
            coda,
            "pub fn coda_togli_ora(",
            "lettore.coda.remove_at(indice);",
        ),
        (
            coda,
            "pub(crate) fn sgombra_dalla_coda(",
            "lettore.coda.remove_tracks(brani);",
        ),
    ] {
        let inizio = testo.find(firma).expect(firma);
        let resto = &testo[inizio..];
        let corpo = &resto[..resto.find("\n}\n").unwrap_or(resto.len())];
        let cambiata = corpo.find(cambio);
        let dimenticate = corpo.find("insieme::dimentica_le_uscite(");
        assert!(
            cambiata.is_some() && dimenticate > cambiata,
            "{firma}: le voci uscite dalla coda restano nel registro"
        );
        let tenute = corpo.find("insieme::tenute_dal_lettore(");
        assert!(
            tenute.is_some() && tenute < cambiata,
            "{firma}: le voci tenute si leggono prima del cambio"
        );
    }
}

#[test]
fn ogni_comando_di_trasporto_e_di_coda_chiede_la_strada_per_primo() {
    for nome in GESTI {
        // Il nucleo, se il comando ne ha uno: il comando `async` lo passa
        // al pool bloccante e basta, ed è il nucleo che chiede la strada.
        // Vedi `col_lettore_in_disparte`.
        let nucleo = format!("{nome}_ora");
        let trovati: Vec<&str> = SORGENTI
            .iter()
            .filter_map(|(_, testo)| corpo(testo, &nucleo).or_else(|| corpo(testo, nome)))
            .collect();
        assert_eq!(trovati.len(), 1, "«{nome}»: un corpo solo, e dev'esserci");
        let corpo = trovati[0];
        let strada = corpo
            .find("strada(")
            .unwrap_or_else(|| panic!("«{nome}» non chiede la strada al gesto"));
        // Prima di qualunque lucchetto del lettore, del risveglio del
        // flusso e della libreria: la strada si chiede a mani vuote.
        for presa in [
            "con_lettore(",
            "gesto_di_coda(",
            "sveglia_fuori_dal_lucchetto(",
            "libreria_di(",
            "cambia_preferenza(",
            "prendi_il_lettore(",
        ] {
            if let Some(dove) = corpo.find(presa) {
                assert!(
                    strada < dove,
                    "«{nome}» prende «{presa}» prima di chiedere la strada"
                );
            }
        }
    }
}

#[test]
fn ogni_comando_del_lettore_ha_deciso_se_e_un_gesto() {
    for (file, testo) in SORGENTI {
        // A inizio riga: nei commenti l'attributo si nomina, fra apici.
        let mut resto = senza_prove(testo);
        while let Some(dove) = resto.find(
            "
#[tauri::command",
        ) {
            resto = &resto[dove..];
            // `fn ` e non `pub fn `: un comando può essere un `async fn`,
            // e la prima `fn ` dopo l'attributo è la sua.
            let firma = resto.find("fn ").map(|i| &resto[i + 3..]);
            let nome: String = firma
                .map(|f| {
                    f.chars()
                        .take_while(|c| *c == '_' || c.is_alphanumeric())
                        .collect()
                })
                .unwrap_or_default();
            assert!(
                GESTI.contains(&nome.as_str())
                    || NON_GESTI.contains(&nome.as_str())
                    || DIFESI_DA_SE.contains(&nome.as_str()),
                "{file}: il comando «{nome}» non dice se è un gesto della sessione"
            );
            resto = &resto[1..];
        }
    }
}

/// I comandi fuori dal lettore che leggono o scrivono per id di brano: la
/// guardia degli id ≤ 0 sta in testa al corpo, prima di qualunque presa
/// della libreria. Un `UPDATE … WHERE id = ?` con una maniglia non
/// toccherebbe niente e risponderebbe «fatto»: è la bugia che la guardia
/// toglie, dicendo `library.trackNotFound`.
#[test]
fn le_guardie_degli_id_stanno_in_testa_ai_comandi_di_libreria() {
    let comandi: [(&str, &str, &[&str]); 5] = [
        (
            "comandi.rs",
            include_str!("../comandi.rs"),
            &[
                "preferito",
                "brano_mostra_nella_cartella",
                "brano_apri_pagina",
                "valutazione",
            ],
        ),
        (
            "metadati.rs",
            include_str!("../metadati.rs"),
            &["metadati_correggi", "metadati_conferma"],
        ),
        (
            "testi.rs",
            include_str!("../testi.rs"),
            &[
                "testo_brano",
                "testo_scarto",
                "testo_cerca",
                "testo_cerca_di_nuovo",
                "testo_candidati",
                "testo_scegli",
                "testo_rifiuta",
                "testo_aggancia",
                "testo_salva",
                "testo_pubblica",
            ],
        ),
        (
            "playlist.rs",
            include_str!("../playlist.rs"),
            &["playlist_aggiungi", "playlist_crea_e_aggiungi"],
        ),
        (
            "cancellazione.rs",
            include_str!("../cancellazione.rs"),
            &["brani_togli", "brani_su_rete", "brani_elimina"],
        ),
    ];
    for (file, testo, nomi) in comandi {
        for nome in nomi {
            let firma_sync = format!("pub fn {nome}(");
            let firma_async = format!("pub async fn {nome}(");
            let inizio = testo
                .find(&firma_sync)
                .or_else(|| testo.find(&firma_async))
                .unwrap_or_else(|| panic!("{file}: «{nome}» non c'è"));
            let corpo = &testo[inizio..];
            let corpo = &corpo[..corpo
                .find(
                    "
}
",
                )
                .unwrap_or(corpo.len())];
            let guardia = corpo
                .find("riga_di_libreria(")
                .or_else(|| corpo.find("righe_di_libreria("))
                .unwrap_or_else(|| panic!("{file}: «{nome}» non ha la guardia"));
            for presa in [
                "con_libreria(",
                "leggi_la_libreria(",
                "in_disparte(",
                "stato_di(",
            ] {
                if let Some(dove) = corpo.find(presa) {
                    assert!(guardia < dove, "{file}: «{nome}» tocca «{presa}» prima");
                }
            }
        }
    }
}

/// Il difetto visto nella prova a due istanze: segno a 50 s, Ospita,
/// Riprendi, Prossimo, Pausa, Termina, e il segno tornava a zero.
///
/// `esci` ferma il motore e rimette la coda dell'utente nello stesso giro
/// di lucchetto; il `Fermato` di quello stop arriva dopo, e trova come
/// corrente la riga dell'utente. Senza la bandiera la regola diceva «segno
/// a zero» per quella riga, che è il difetto; con la bandiera non tocca
/// niente.
#[test]
fn il_fermato_dell_uscita_dalla_sessione_non_tocca_il_segno_dell_utente() {
    let brano_dell_utente = Some(7902);
    // Il difetto: senza sapere di chi è, il Fermato azzera il segno.
    assert_eq!(
        dopo_il_fermato(false, false, false, brano_dell_utente).segno_a_zero,
        brano_dell_utente
    );
    // Con la bandiera dell'uscita: niente ascolto, niente coda, niente
    // segno.
    let uscita = dopo_il_fermato(true, false, false, brano_dell_utente);
    assert!(!uscita.tocca_l_utente);
    assert!(!uscita.avanza_sul_non_aperto);
    assert_eq!(uscita.segno_a_zero, None);
    // Dentro la sessione una maniglia non ha segno, e chi segue non fa
    // avanzare la sua coda specchio.
    let in_sessione = dopo_il_fermato(false, true, false, Some(-4));
    assert_eq!(in_sessione.segno_a_zero, None);
    assert!(!in_sessione.avanza_sul_non_aperto);
    // Una rete caduta tiene il punto a metà, come sempre.
    assert_eq!(
        dopo_il_fermato(false, false, true, brano_dell_utente).segno_a_zero,
        None
    );
}

/// Il cablaggio della bandiera dell'uscita, che la prova qui sopra non
/// vede: `esci` la alza **dopo** lo stop, `fermato` la consuma per prima
/// cosa, e ogni brano dato al motore la abbassa. Se una delle tre righe
/// sparisse, la prova della regola continuerebbe a passare e il segno
/// tornerebbe a zero.
#[test]
fn la_bandiera_dell_uscita_e_alzata_consumata_e_abbassata() {
    let ponte = senza_prove(include_str!("insieme.rs"));
    let esci = &ponte[ponte.find("pub fn esci(").unwrap_or(0)..];
    let stop = esci.find("lettore.ferma();").expect("esci ferma il motore");
    let alza = esci
        .find("lettore.fermato_della_sessione = true;")
        .expect("esci alza la bandiera");
    assert!(stop < alza, "la bandiera va alzata dopo lo stop");
    let lettore = senza_prove(include_str!("mod.rs"));
    let fermato = corpo(lettore, "fermato").unwrap_or_default();
    assert!(fermato.is_empty(), "`fermato` non è pub: si cerca a mano");
    let fermato = &lettore[lettore.find("\nfn fermato(").unwrap_or(0)..];
    let consuma = fermato
        .find("std::mem::take(&mut lettore.fermato_della_sessione)")
        .expect("fermato consuma la bandiera");
    let primo_effetto = fermato.find("chiudi_ascolto(").unwrap_or(usize::MAX);
    assert!(
        consuma < primo_effetto,
        "la bandiera si guarda prima di tutto"
    );
    let avvio = &lettore[lettore.find("fn dopo_un_avvio(").unwrap_or(0)..];
    let avvio = &avvio[..avvio.find("\n    }").unwrap_or(avvio.len())];
    assert!(
        avvio.contains("self.fermato_della_sessione = false;"),
        "un brano nuovo abbassa la bandiera"
    );
}

/// Le strade che scrivono il segno di «riprendi dov'eri» sono tre, e
/// tutte e tre guardano di chi è il brano: l'uscita e l'orologio (solo
/// righe della libreria) e la fine della coda (solo righe, e mai lo stop
/// dell'uscita da una sessione). Una quarta strada fa fallire la prova, ed
/// è voluto: chi la aggiunge deve dire cosa fa in sessione.
/// Il testo da `inizio`, che comincia con un nome seguito da `(`, fino
/// alla parentesi che chiude quella chiamata.
fn fino_alla_parentesi(inizio: &str) -> &str {
    let mut aperte = 0_usize;
    for (dove, carattere) in inizio.char_indices() {
        match carattere {
            '(' => aperte += 1,
            ')' => {
                aperte = aperte.saturating_sub(1);
                if aperte == 0 {
                    return &inizio[..=dove];
                }
            }
            _ => {}
        }
    }
    inizio
}

/// Le chiamate che, dentro un comando, possono aspettare secondi: un brano
/// da aprire (i quindici di `APERTURA_FLUSSO` su una share o un servizio),
/// un dispositivo da aprire o da elencare (i venti di `APERTURA_USCITA`),
/// la libreria senza limite (`completa(.., None)`), il filo della sessione
/// (i venti di `ATTESA_COMANDO`), la scheda di un item dell'Archive, il
/// gestore file su una share, un database scelto dall'utente.
const ATTESE_LUNGHE: [&str; 12] = [
    "gesto_di_coda(",
    "avvia_corrente(",
    "riprendi_ora(",
    "riapri_su(",
    "brano_di(",
    "completa(",
    "dispositivi::elenco(",
    "chiedi(",
    "e_poi_il_pannello(",
    "per_tenere(",
    "reveal_item_in_dir(",
    "importa_interno(",
];

/// I sorgenti fuori dal lettore con comandi della stessa specie: la
/// sessione, l'esplorazione dei cataloghi, i comandi generali.
const ALTRI_SORGENTI: [(&str, &str); 3] = [
    ("insieme/mod.rs", include_str!("../insieme/mod.rs")),
    ("esplora.rs", include_str!("../esplora.rs")),
    ("comandi.rs", include_str!("../comandi.rs")),
];

/// Nessun comando sincrono aspetta secondi.
///
/// Un `#[tauri::command(async)]` su una `fn` sincrona non gira su un filo
/// suo: `tauri-macros` lo avvolge in un blocco `async` e lo manda a
/// `async_runtime::spawn`, cioè su un worker del runtime — uno per
/// processore, prestati a **tutti** i comandi asincroni. Quattro clic su
/// brani diversi con il NAS addormentato, su un PC a quattro fili logici,
/// fermavano ogni comando asincrono fino a quindici secondi: la libreria
/// non scorreva, la ricerca non rispondeva. Un comando che può aspettare è
/// un `async fn` che fa il lavoro con `crate::disparte::in_disparte`, sul
/// pool bloccante; senza `(async)` girerebbe sul filo della finestra, che
/// è peggio. Il lavoro vero sta in un nucleo sincrono, senza attributo,
/// che chiamano anche i tasti multimediali e la sessione.
#[test]
fn nessun_comando_che_apre_un_brano_gira_su_un_worker() {
    let mut sincroni = 0;
    let mut sbagliati = Vec::new();
    for (file, testo) in SORGENTI.iter().chain(ALTRI_SORGENTI.iter()) {
        let codice = senza_prove(testo);
        for (dove, _) in codice.match_indices("\n#[tauri::command") {
            let resto = &codice[dove + 1..];
            let firma = &resto[..resto.find('{').unwrap_or(resto.len())];
            if firma.contains("async fn ") {
                continue;
            }
            sincroni += 1;
            let nome: String = firma
                .find("fn ")
                .map(|i| &firma[i + 3..])
                .unwrap_or_default()
                .chars()
                .take_while(|c| *c == '_' || c.is_alphanumeric())
                .collect();
            let corpo = &resto[..resto.find("\n}\n").unwrap_or(resto.len())];
            if let Some(attesa) = ATTESE_LUNGHE.iter().find(|a| corpo.contains(**a)) {
                sbagliati.push(format!("{file}: «{nome}» ({attesa})"));
            }
        }
    }
    assert!(sincroni > 0, "la ricerca non ha trovato nessun comando");
    assert!(
        sbagliati.is_empty(),
        "comandi sincroni che possono aspettare secondi: {sbagliati:#?}"
    );
}

/// Nessun lavoro del filo del seguito aspetta la libreria senza una
/// scadenza, né la scrive: dietro di lui aspettano tutti gli stati che
/// vengono dopo — il titolo del brano nuovo, la coda, la scheda di Windows,
/// il vassoio — e una scrittura ferma dietro una presa lunga li teneva
/// tutti sul brano di prima. Le scritture che nascono sotto il lucchetto
/// del lettore — l'ascolto, il segno a zero, la posizione all'uscita —
/// vanno sul filo delle scritture, in fila fra loro: vedi [`da_scrivere`].
#[test]
fn nessun_lavoro_del_seguito_aspetta_la_libreria_senza_scadenza() {
    let mut lavori = 0;
    for (file, testo) in SORGENTI {
        let codice = senza_prove(testo);
        for (dove, _) in codice.match_indices("in_ordine(") {
            if codice[..dove].ends_with("fn ") {
                continue;
            }
            lavori += 1;
            let lavoro = fino_alla_parentesi(&codice[dove..]);
            for vietato in ["con_libreria(", "scrivi_ascolto(", "save_posizione("] {
                assert!(
                    !lavoro.contains(vietato),
                    "{file}: un lavoro del seguito chiama «{vietato}»: {lavoro}"
                );
            }
        }
    }
    assert!(lavori > 0, "la ricerca non ha trovato nessun lavoro");
}

/// All'uscita le due file si aspettano entro una scadenza **sola**: due file
/// ferme non raddoppiano l'attesa che `ATTESA_USCENDO` tiene corta.
#[test]
fn le_due_file_si_aspettano_con_una_scadenza_sola() {
    let (_tieni_prima, prima) = std::sync::mpsc::channel::<()>();
    let (_tieni_seconda, seconda) = std::sync::mpsc::channel::<()>();
    let entro = Duration::from_millis(300);
    let inizio = std::time::Instant::now();
    assert!(!aspetta_le_risposte(&[prima, seconda], inizio + entro));
    let atteso = inizio.elapsed();
    assert!(atteso >= entro, "ha rinunciato prima: {atteso:?}");
    assert!(
        atteso < entro * 2 - Duration::from_millis(50),
        "le attese si sono sommate: {atteso:?}"
    );
    // E quando rispondono, non si aspetta niente di più.
    let (manda, risposta) = std::sync::mpsc::channel::<()>();
    manda.send(()).expect("il canale è aperto");
    assert!(aspetta_le_risposte(
        &[risposta],
        std::time::Instant::now() + Duration::from_secs(5)
    ));
}

/// Lo stato aspetta le scritture di prima della sua fotografia — e solo
/// fino alla scadenza: una scrittura ferma dietro la libreria non lo tiene
/// sul brano di prima.
#[test]
fn lo_stato_aspetta_le_scritture_di_prima_ma_non_oltre_la_scadenza() {
    let fatte = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    // Ferma: la scadenza vince.
    let inizio = std::time::Instant::now();
    assert!(!aspetta_il_contatore(
        &fatte,
        1,
        inizio + Duration::from_millis(100)
    ));
    assert!(inizio.elapsed() < Duration::from_secs(2));
    // Finita poco dopo: si aspetta lei, non la scadenza.
    let mano = std::sync::Arc::clone(&fatte);
    let scrittura = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        mano.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    });
    let inizio = std::time::Instant::now();
    assert!(aspetta_il_contatore(
        &fatte,
        1,
        inizio + Duration::from_secs(10)
    ));
    assert!(inizio.elapsed() < Duration::from_secs(5));
    scrittura.join().expect("la scrittura finta non cade");
}

#[test]
fn il_segno_si_scrive_solo_da_strade_guardate() {
    let mut strade = Vec::new();
    for (file, testo) in SORGENTI {
        let codice = senza_prove(testo);
        for (dove, _) in codice.match_indices("playback::save_posizione(") {
            strade.push((file, dove));
        }
    }
    assert_eq!(
        strade.iter().map(|(file, _)| *file).collect::<Vec<_>>(),
        ["mod.rs", "fili.rs", "fili.rs"],
        "le strade del segno sono cambiate: {strade:?}"
    );
    let fili = senza_prove(include_str!("fili.rs"));
    let mut prima = 0;
    for (dove, _) in fili.match_indices("playback::save_posizione(") {
        let davanti = &fili[prima..dove];
        let funzione = &davanti[davanti.rfind("\nfn ").unwrap_or(0)..];
        assert!(
            funzione.contains("ospiti::e_di_libreria(*id)"),
            "una strada di fili.rs scrive il segno anche per le maniglie"
        );
        prima = dove;
    }
    let lettore = senza_prove(include_str!("mod.rs"));
    let fine = &lettore[..lettore.find("playback::save_posizione(").unwrap_or(0)];
    let fine = &fine[fine.rfind("fn fermato(").unwrap_or(0)..];
    assert!(
        fine.contains(".segno_a_zero"),
        "la fine della coda scrive il segno senza chiedere a `dopo_il_fermato`"
    );
}

/// Il guasto di rete di un flusso parla di rete, non di cartelle.
///
/// La frase di `fs.networkUnavailable` manda a controllare il server, la
/// VPN e il cavo di una cartella di rete: su un brano dell'Internet Archive
/// è la frase sbagliata, e fino alla 2.4.0 era quella che si leggeva. La
/// causa resta, perché al diario serve; e resta ritentabile, perché
/// «Riprova» è il gesto giusto.
#[test]
fn un_flusso_interrotto_non_parla_di_cartelle() {
    let cartella = AppError::new(ErrorCode::FsNetworkUnavailable { path: None })
        .with_cause("os error 64".to_owned());
    let flusso = interruzione_di_flusso(cartella);
    assert_eq!(
        flusso.code().kind(),
        ErrorCodeKind::PlaybackStreamInterrupted
    );
    assert_eq!(flusso.cause(), Some("os error 64"));
    assert!(flusso.code().is_retryable());

    // Ogni altro guasto resta com'è: un formato che non si apre non è la
    // rete, e dirlo interrotto offrirebbe «Riprova» dove non serve.
    let formato = AppError::new(ErrorCode::PlaybackFormatUnsupported { format: None });
    assert_eq!(
        interruzione_di_flusso(formato).code().kind(),
        ErrorCodeKind::PlaybackFormatUnsupported
    );
}

/// Un file che fa cadere il riconoscimento del formato non è la rete.
///
/// Era il difetto: la caduta chiudeva il canale della scadenza, l'apertura
/// riceveva lo stesso «niente» di una cartella che non risponde, e la
/// finestra offriva «Riprova» per sempre su un file locale rovinato.
#[test]
fn un_formato_che_fa_cadere_l_apertura_non_si_riprova() {
    use rymembra_app::scadenza::Mancato;

    let caduto = apertura_mancata(
        &Mancato::Caduto {
            messaggio: Some("intestazione storta".to_owned()),
        },
        APERTURA_BRANO,
        false,
        Some(r"C:\Musica\rotto.flac".to_owned()),
    );
    assert_eq!(
        caduto.code().kind(),
        ErrorCodeKind::PlaybackFormatUnsupported
    );
    assert!(!caduto.is_retryable(), "un file rovinato non si riprova");
    assert!(
        caduto
            .cause()
            .is_some_and(|c| c.contains("intestazione storta")),
        "il messaggio del panico deve arrivare al diario"
    );
    // Anche quando il brano arriva da un catalogo: è il contenitore a non
    // leggersi, non la rete a non rispondere.
    assert!(
        !apertura_mancata(
            &Mancato::Caduto { messaggio: None },
            APERTURA_FLUSSO,
            true,
            None
        )
        .is_retryable()
    );

    // La scadenza vera resta quella di sempre: ritentabile, col codice
    // della cartella per un file e della rete per un flusso.
    let scaduto = apertura_mancata(
        &Mancato::Scaduto,
        APERTURA_FLUSSO,
        false,
        Some(r"\\nas\musica\a.flac".to_owned()),
    );
    assert_eq!(scaduto.code().kind(), ErrorCodeKind::FsNetworkUnavailable);
    assert!(scaduto.is_retryable());
    let flusso = apertura_mancata(&Mancato::Scaduto, APERTURA_FLUSSO, true, None);
    assert_eq!(flusso.code().kind(), ErrorCodeKind::NetTimeout);
    assert!(flusso.is_retryable());
}
