//! Le prove dello Studio: il registro, la validazione, la bozza.

use super::*;
use crate::skin::DI_SERIE;

#[test]
fn il_registro_dice_tutto_quel_che_serve_a_un_editor() {
    let registro = studio_registro();
    assert_eq!(registro.tokens.len(), rymembra_skin::TOKENS.len());
    assert_eq!(registro.parts.len(), rymembra_skin::parts::PARTS.len());
    // Tutti gli effetti, ognuno col suo costo vero. Se un effetto nuovo
    // nascesse e `esempio()` non lo conoscesse, questo test lo direbbe
    // invece di lasciarlo sparire in silenzio dall'elenco.
    assert_eq!(registro.effects.len(), rymembra_skin::Effect::NAMES.len());
    let sfoca = registro
        .effects
        .iter()
        .find(|e| e.name == "blurBehind")
        .expect("blurBehind");
    assert_eq!(
        sfoca.cost, registro.budget,
        "una sfocatura da sola è tutto il budget"
    );

    // L'esemplare minimo è quel che «Aggiungi livello» scrive nel documento.
    // Se non si lasciasse riparsare, l'editor comporrebbe un livello che la
    // validazione rifiuta un istante dopo averlo aggiunto.
    for effetto in &registro.effects {
        let documento = format!(
            r##"{{"format":1,"id":"esempio","meta":{{"name":"E","author":"A","version":"1.0.0"}},
                     "tokens":{{}},"patterns":{{"x":{}}}}}"##,
            effetto.esempio
        );
        let riletto = rymembra_skin::parse_skin_json(&documento)
            .unwrap_or_else(|err| panic!("«{}»: {err:?}", effetto.name));
        let (_, quale) = riletto.patterns.first().expect("il motivo");
        assert_eq!(quale.name(), effetto.name);
    }
}

#[test]
fn la_validazione_non_fallisce_mai_su_un_documento_rotto() {
    // A metà di una parentesi il JSON non è JSON, ed è il caso normale
    // mentre si scrive: se questo comando tornasse un errore IPC, l'editor
    // dovrebbe distinguere «il comando è andato storto» da «non ho ancora
    // finito di scrivere».
    let esito = studio_valida("{ \"format\": 1, ".to_owned(), None);
    assert_eq!(esito.errori.len(), 1);
    assert!(esito.css.is_empty(), "niente foglio da un documento rotto");
}

/// Il difetto che si vedeva peggio: un errore di sintassi arrivava come
/// `{ code, path: "", message: "", forse: [] }`, cioè un pannello rosso
/// permanente che non diceva né dove né cosa. La frase che l'editor mostrava
/// al suo posto veniva dal catalogo ed era scritta per il toast dell'app:
/// «Aprila nello Studio: i problemi vengono elencati riga per riga» —
/// letta da dentro lo Studio, e falsa.
#[test]
fn un_errore_di_sintassi_dice_dove_e_cosa() {
    // Una virgola che manca fra due campi, alla riga tre.
    let json = "{
  \"format\": 1
  \"id\": \"prova\"
}";
    let esito = studio_valida(json.to_owned(), None);
    let solo = match esito.errori.as_slice() {
        [solo] => solo,
        altri => panic!("un errore di sintassi è uno solo: {altri:#?}"),
    };
    assert_eq!(solo.code, "skin.manifestInvalid");
    assert_eq!(solo.riga, Some(3), "{solo:#?}");
    assert!(solo.colonna.is_some());
    assert!(
        solo.message.contains("virgola"),
        "il messaggio dice cosa manca, non «il manifest non è JSON»: {solo:#?}"
    );
}

/// Venti problemi erano venti frasi dentro **un** messaggio, unite da punti
/// e virgola, con il percorso del primo. Il piede che conta «{n} errori»,
/// l'elenco che li mostra e il giro di correzione della chat sono tutti e
/// tre scritti per una lista: adesso ne ricevono una.
#[test]
fn ogni_problema_e_un_problema_e_porta_la_sua_riga() {
    // Solo nel primo `tokens`: dal formato 2 ce n'è un secondo, nella
    // variante, e lì gli stessi tre sbagli sarebbero altri tre errori.
    let json = rymembra_skin::PLAIN_SOURCE.replacen(
        "\"tokens\": {",
        "\"tokens\": {
    \"color.accents\": \"#ff0000\",
    \"radius.cards\": \"4px\",
    \"font.mainly\": [\"X\"],",
        1,
    );
    let esito = studio_valida(json, None);
    assert_eq!(
        esito.errori.len(),
        3,
        "tre token inesistenti sono tre errori: {:#?}",
        esito.errori
    );

    // Nell'ordine del **documento**, non in quello alfabetico in cui li
    // consegna la mappa di `serde_json`: si correggono dall'alto in basso.
    let percorsi: Vec<&str> = esito.errori.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(
        percorsi,
        [
            "tokens.color.accents",
            "tokens.radius.cards",
            "tokens.font.mainly"
        ]
    );

    // Righe vere, crescenti, e distinte: è quel che l'euristica a `indexOf`
    // non sapeva dare — cercando `"cards"` o `"accents"` prendeva la prima
    // occorrenza del file, che è quasi sempre un'altra riga.
    let righe: Vec<Option<u32>> = esito.errori.iter().map(|e| e.riga).collect();
    assert!(righe.iter().all(Option::is_some), "{righe:?}");
    assert!(righe.windows(2).all(|due| due[0] < due[1]), "{righe:?}");

    // E il suggerimento arriva come dato anche per un token, non solo per
    // una parte: senza, il bottone che corregge non si può costruire.
    let accento = &esito.errori[0];
    assert!(
        accento.forse.contains(&"color.accent".to_owned()),
        "{accento:#?}"
    );
}

/// Un avviso ha un percorso come un errore, quindi ha un posto: il bottone
/// che ci porta il cursore è lo stesso, e non aveva un numero da usare.
#[test]
fn anche_un_avviso_sa_su_che_riga_sta() {
    let json = rymembra_skin::PLAIN_SOURCE.replace(
        "\"capabilities\": {",
        "\"patterns\": { \"mai-usato\": { \"effect\": \"vignette\", \"color\": \"#000\" } }, \"capabilities\": {",
    );
    let esito = studio_valida(json, None);
    let avviso = esito
        .avvisi
        .iter()
        .find(|a| a.kind == "unusedPattern")
        .unwrap_or_else(|| panic!("{:#?}", esito.avvisi));
    assert_eq!(avviso.path, "patterns.mai-usato");
    assert!(avviso.riga.is_some(), "{avviso:#?}");
}

#[test]
fn un_nome_di_parte_sbagliato_arriva_col_suo_suggerimento() {
    let json = rymembra_skin::PLAIN_SOURCE.replace(
        "\"layout\": {",
        "\"parts\": { \"section-cards\": { \"radius\": \"10px\" } }, \"layout\": {",
    );
    let esito = studio_valida(json, None);
    let primo = esito.errori.first().expect("un errore");
    assert!(
        primo.forse.contains(&"section-card".to_owned()),
        "il suggerimento di nearest_parts deve arrivare come dato, non solo \
             dentro la frase: {primo:?}"
    );
}

#[test]
fn la_skin_di_serie_passa_dallo_studio_senza_errori() {
    let esito = studio_valida(rymembra_skin::PLAIN_SOURCE.to_owned(), None);
    assert!(esito.errori.is_empty(), "{:#?}", esito.errori);
    assert!(esito.avvisi.is_empty(), "{:#?}", esito.avvisi);
    assert!(!esito.css.is_empty());
    assert_eq!(esito.costo, 0);
    // E la tabella dei contrasti è piena: è quel che lo Studio mostra a
    // destra, e una tabella vuota vorrebbe dire che nessun colore si è
    // lasciato risolvere.
    assert!(esito.contrasti.len() > 10);
    assert!(
        esito.contrasti.iter().all(|c| c.passa),
        "{:#?}",
        esito.contrasti
    );
}

#[test]
fn una_bozza_non_puo_uscire_dalla_sua_cartella() {
    for cattivo in ["../../altrove", "..\\fuori", "c:/assoluto", ""] {
        assert!(!id_sicuro(cattivo), "«{cattivo}» non è stato respinto");
    }
    assert!(id_sicuro("ruggine"));
    assert!(id_sicuro("fork_di_plain-2"));
}

// ── le istantanee ───────────────────────────────────────────────────────

#[test]
fn il_nome_di_unistantanea_si_legge_e_si_riscrive_uguale() {
    // Il nome del file **è** l'indice: se andata e ritorno non combaciano,
    // un'istantanea si scrive e non si ritrova più, e il difetto si vede
    // solo dopo — quando serve.
    let nome = "1754121234567-salvata-34-41.json";
    let letta = istantanea_da(nome).expect("un nome buono");
    assert_eq!(letta.quando, 1_754_121_234_567);
    assert_eq!(letta.causa, "salvata");
    assert_eq!((letta.parti, letta.token), (34, 41));
    assert_eq!(nome_istantanea(&letta), nome);

    for storto in [
        "1754121234567-salvata-34.json",       // manca un pezzo
        "1754121234567-battezzata-34-41.json", // una causa che non esiste
        "adesso-salvata-34-41.json",           // un istante che non è un numero
        "1754121234567-salvata-34-41-42.json", // un pezzo di troppo
        "1754121234567-salvata-34-41.txt",     // non è un manifest
    ] {
        assert!(istantanea_da(storto).is_none(), "«{storto}» è passato");
    }
}

#[test]
fn unistantanea_si_prende_si_rilegge_e_si_ripristina_identica() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    prendi_istantanea(dir.path(), rymembra_skin::PLAIN_SOURCE, Causa::Salvata).expect("presa");

    let elenco = elenco_istantanee(dir.path());
    assert_eq!(elenco.len(), 1);
    let sola = elenco.first().expect("una");
    assert_eq!(sola.causa, "salvata");
    // I conteggi sono quelli del documento, presi al momento della scrittura.
    assert_eq!(sola.token, {
        let documento = rymembra_skin::parse_skin_json(rymembra_skin::PLAIN_SOURCE).expect("plain");
        documento.tokens.iter().count()
    });

    assert_eq!(
        ripristina_istantanea(dir.path(), sola.quando).expect("ripristinata"),
        rymembra_skin::PLAIN_SOURCE,
        "il file è il manifest e basta: si copia fuori e si apre"
    );
}

#[test]
fn una_bozza_a_meta_si_puo_comunque_fotografare() {
    // Una bozza si salva anche quando non è valida — è un lavoro in corso —
    // e allora anche la sua istantanea deve poter esistere. I conteggi non
    // si sanno, e l'elenco dice zero invece di rifiutare la fotografia.
    let dir = tempfile::tempdir().expect("cartella temporanea");
    prendi_istantanea(dir.path(), "{ \"format\": 1, ", Causa::Manuale).expect("presa");
    let sola = elenco_istantanee(dir.path()).first().copied().expect("una");
    assert_eq!((sola.parti, sola.token), (0, 0));
}

#[test]
fn oltre_il_ventesimo_giro_la_piu_vecchia_sparisce() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    for _ in 0..(QUANTE_ISTANTANEE + 5) {
        prendi_istantanea(dir.path(), rymembra_skin::PLAIN_SOURCE, Causa::Manuale).expect("presa");
    }
    let elenco = elenco_istantanee(dir.path());
    assert_eq!(
        elenco.len(),
        QUANTE_ISTANTANEE,
        "la potatura non ha tagliato"
    );
    // E quelle rimaste sono le **ultime**, in ordine dalla più recente.
    assert!(
        elenco.windows(2).all(|coppia| {
            coppia
                .first()
                .is_some_and(|prima| coppia.get(1).is_some_and(|dopo| prima.quando > dopo.quando))
        }),
        "l'ordine non è dalla più recente: {elenco:#?}"
    );
}

#[test]
fn due_istantanee_nello_stesso_istante_non_si_sovrascrivono() {
    // «Salva e usa» ed «Esporta» possono capitare a un battito di distanza,
    // e due file con lo stesso nome vorrebbero dire perderne una in
    // silenzio.
    let dir = tempfile::tempdir().expect("cartella temporanea");
    prendi_istantanea(dir.path(), rymembra_skin::PLAIN_SOURCE, Causa::Salvata).expect("una");
    prendi_istantanea(dir.path(), rymembra_skin::PLAIN_SOURCE, Causa::Salvata).expect("due");
    assert_eq!(elenco_istantanee(dir.path()).len(), 2);
}

#[test]
fn le_istantanee_di_una_bozza_che_non_ne_ha_sono_nessuna() {
    // Nessuna cartella non è un guasto: è il caso di ogni bozza prima della
    // prima prova, ed è quel che la colonna sinistra chiede all'apertura.
    let dir = tempfile::tempdir().expect("cartella temporanea");
    assert!(elenco_istantanee(&dir.path().join("mai-esistita")).is_empty());
}

// ── il pacchetto ────────────────────────────────────────────────────────

#[test]
fn lalbero_del_pacchetto_dice_quel_che_lesportazione_scriverebbe() {
    // La skin di serie non ha risorse: una voce sola, il manifest. Se un
    // giorno l'albero mostrasse un file che `write_skin_package` non
    // riscrive, direbbe una bugia a chi poi prova a esportare.
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let da = da_lavorare(dir.path(), DI_SERIE).expect("la skin di serie");
    assert_eq!(da.sorgente, rymembra_skin::PLAIN_SOURCE);
    assert!(da.preview.is_none());
    assert!(da.assets.is_empty());
}

#[test]
fn ogni_skin_di_serie_si_apre_e_si_deriva() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    for (id, testo) in rymembra_skin::DI_SERIE {
        let da = da_lavorare(dir.path(), id).expect("una skin di serie");
        assert_eq!(da.sorgente, *testo, "{id}");
    }
    assert!(da_lavorare(dir.path(), "non-esiste").is_err());
}

#[test]
fn una_bozza_vince_sul_pacchetto_ma_le_risorse_restano_quelle() {
    // È l'invariante che tiene insieme l'albero e l'esportazione: il
    // manifest può venire da una bozza, le risorse no — una bozza è un
    // `skin.json` e basta.
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let bozza = cartella_bozze(dir.path()).join("plain");
    std::fs::create_dir_all(&bozza).expect("cartella");
    let modificata =
        rymembra_skin::PLAIN_SOURCE.replace("\"name\": \"Plain\"", "\"name\": \"Mia\"");
    std::fs::write(bozza.join("skin.json"), &modificata).expect("scritta");

    let da = da_lavorare(dir.path(), DI_SERIE).expect("la bozza");
    assert_eq!(da.sorgente, modificata, "la bozza deve vincere sul binario");
}

#[test]
fn ogni_effetto_porta_le_sue_manopole() {
    // La tabella dei parametri è scritta a mano perché `Effect` è un enum di
    // varianti tipizzate e non c'è niente da cui leggerla a macchina. Questo
    // test è il prezzo di quella scelta: un effetto nuovo senza la sua riga
    // arriverebbe all'editor come un livello che si aggiunge e non si può
    // aprire, che è il difetto da cui `params` è nato.
    let registro = studio_registro();
    assert_eq!(
        registro.effects.len(),
        rymembra_skin::effects::Effect::NAMES.len()
    );
    for effetto in &registro.effects {
        assert!(
            !effetto.params.is_empty(),
            "«{}» non dichiara nessuna manopola",
            effetto.name
        );
    }
}

#[test]
fn le_manopole_dichiarate_esistono_davvero_nell_esemplare() {
    // Un parametro obbligatorio deve comparire nell'esemplare minimo: se non
    // ci fosse, l'editor mostrerebbe un controllo vuoto per un campo che il
    // parser pretende, e il primo salvataggio fallirebbe.
    for effetto in studio_registro().effects {
        let esempio: serde_json::Value =
            serde_json::from_str(effetto.esempio).expect("l'esemplare è JSON");
        for parametro in effetto.params.iter().filter(|p| !p.optional) {
            assert!(
                esempio.get(parametro.name).is_some(),
                "«{}» dichiara «{}» obbligatorio e non lo scrive nell'esemplare",
                effetto.name,
                parametro.name
            );
        }
    }
}

#[test]
fn buttata_la_bozza_torna_a_parlare_il_pacchetto() {
    // Il comando vive dietro `con_libreria` e qui non si può chiamare, ma
    // quel che deve garantire sì: tolta la cartella della bozza,
    // `da_lavorare` — la stessa funzione che decide all'apertura — deve
    // tornare a leggere il pacchetto. Senza questo, «butta la bozza»
    // lascerebbe l'editor su un documento che non sta più da nessuna parte.
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let bozza = cartella_bozze(dir.path()).join(DI_SERIE);
    std::fs::create_dir_all(&bozza).expect("cartella");
    let modificata =
        rymembra_skin::PLAIN_SOURCE.replace("\"name\": \"Plain\"", "\"name\": \"Mia\"");
    std::fs::write(bozza.join("skin.json"), &modificata).expect("scritta");
    assert_eq!(
        da_lavorare(dir.path(), DI_SERIE)
            .expect("la bozza")
            .sorgente,
        modificata
    );

    std::fs::remove_dir_all(&bozza).expect("buttata");
    assert_eq!(
        da_lavorare(dir.path(), DI_SERIE)
            .expect("il pacchetto")
            .sorgente,
        rymembra_skin::PLAIN_SOURCE
    );
}

#[test]
fn la_conversazione_sta_con_la_bozza_e_ne_segue_la_sorte() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    assert_eq!(leggi_chat(dir.path(), "mia").expect("letta"), None);

    scrivi_chat(dir.path(), "mia", r#"{"versione":1,"battute":[]}"#).expect("scritta");
    assert_eq!(
        leggi_chat(dir.path(), "mia").expect("letta").as_deref(),
        Some(r#"{"versione":1,"battute":[]}"#)
    );

    // Una rinomina la porta con sé.
    rinomina_bozza(dir.path(), "mia", "nuova", None).expect("rinominata");
    assert_eq!(leggi_chat(dir.path(), "mia").expect("letta"), None);
    assert!(leggi_chat(dir.path(), "nuova").expect("letta").is_some());

    // Vuota, si toglie; due volte non è un guasto.
    scrivi_chat(dir.path(), "nuova", "").expect("tolta");
    scrivi_chat(dir.path(), "nuova", "").expect("già tolta");
    assert_eq!(leggi_chat(dir.path(), "nuova").expect("letta"), None);
}

#[test]
fn una_conversazione_storta_non_si_scrive() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    for cattiva in ["non json", "[1,2]", "\"testo\""] {
        assert!(
            scrivi_chat(dir.path(), "mia", cattiva).is_err(),
            "«{cattiva}»"
        );
    }
    let enorme = format!(r#"{{"x":"{}"}}"#, "a".repeat(CHAT_MASSIMA));
    assert!(scrivi_chat(dir.path(), "mia", &enorme).is_err());
    for cattivo in ["../fuori", ""] {
        assert!(scrivi_chat(dir.path(), cattivo, "{}").is_err());
        assert!(leggi_chat(dir.path(), cattivo).is_err());
    }
}

#[test]
fn un_id_storto_non_raggiunge_ne_il_pacchetto_ne_le_istantanee() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    for cattivo in ["../../altrove", "..\\fuori", "c:/assoluto", ""] {
        assert!(
            da_lavorare(dir.path(), cattivo).is_err(),
            "«{cattivo}» ha raggiunto un pacchetto"
        );
    }
}

// ── il registro del movimento ───────────────────────────────────────────

/// Un documento di prova con un'animazione dal fotogramma dato.
fn con_fotogramma(campo: &str, valore: &str) -> String {
    rymembra_skin::PLAIN_SOURCE.replacen(
        "\"motion\": {",
        &format!(
            "\"motion\": {{ \"animations\": {{ \"prova\": {{ \"duration\": \"200ms\", \
                 \"frames\": [{{ \"at\": 0, \"{campo}\": {valore} }}, {{ \"at\": 100, \"opacity\": 1 }}] }} }},"
        ),
        1,
    )
}

#[test]
fn gli_estremi_dei_fotogrammi_sono_quelli_del_parser() {
    let registro = studio_registro();
    assert_eq!(registro.stati.len(), rymembra_skin::PartState::ALL.len());
    assert!(
        registro
            .inneschi
            .iter()
            .any(|i| i.name == "hover" && i.ciclo && i.stato)
    );
    assert!(
        registro
            .inneschi
            .iter()
            .any(|i| i.name == "exit" && i.vista && !i.ciclo)
    );
    assert!(!registro.animazioni_pronte.is_empty());
    // Lo schema arriva intero: il completamento del codice ci conta.
    assert!(registro.chiavi["aspetto"].contains(&"focusRing"));
    assert!(registro.chiavi["documento"].contains(&"variant"));
    assert!(registro.parole["easing"].contains(&"ease-out"));
    assert!(registro.parole["borderStyle"].contains(&"dashed"));
    assert!(!registro.sorgenti.is_empty());
    for campo in &registro.fotogramma {
        let (Some(min), Some(max)) = (campo.min, campo.max) else {
            continue;
        };
        let scrivi = |n: f64| match campo.kind {
            "length" => format!("\"{n}px\""),
            _ => n.to_string(),
        };
        for dentro in [min, max] {
            let testo = con_fotogramma(campo.name, &scrivi(dentro));
            assert!(
                rymembra_skin::parse_skin_json(&testo).is_ok(),
                "«{}» = {dentro} dovrebbe passare: {:?}",
                campo.name,
                rymembra_skin::parse_skin_json(&testo).err()
            );
        }
        let fuori = con_fotogramma(campo.name, &scrivi(max + 1.0));
        assert!(
            rymembra_skin::parse_skin_json(&fuori).is_err(),
            "«{}» = {} dovrebbe essere rifiutato",
            campo.name,
            max + 1.0
        );
    }
}

#[test]
fn la_validazione_scrive_per_la_tela_e_dice_i_costi() {
    let esito = studio_valida(
        rymembra_skin::di_serie("vinile")
            .expect("vinile")
            .to_owned(),
        None,
    );
    assert!(esito.errori.is_empty(), "{:?}", esito.errori);
    assert!(esito.css.contains(":root[data-skin='vinile'] ."));
    assert!(esito.css.contains("[data-forza~='hover']"));
    let colonna = esito
        .costi
        .iter()
        .find(|c| c.parte == "column-art")
        .expect("column-art");
    assert!(colonna.movimento > 0);
    assert!(esito.costo_finestra > 0);
}

// ── le risorse della bozza ──────────────────────────────────────────────

#[test]
fn il_nome_di_una_risorsa_si_mette_in_regola() {
    assert_eq!(
        nome_risorsa(Path::new("C:/x/Titoli Grassetto.WOFF2")),
        "titoli-grassetto.woff2"
    );
    assert_eq!(nome_risorsa(Path::new("/x/foto.JPEG")), "foto.jpg");
    assert_eq!(nome_risorsa(Path::new("/x/__.png")), "risorsa.png");
}

#[test]
fn una_risorsa_si_aggiunge_si_rinomina_se_occupato_e_si_rifiuta_se_mente() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let fuori = tempfile::tempdir().expect("cartella dei file");
    let buono = fuori.path().join("Trama.png");
    std::fs::write(&buono, b"\x89PNG\r\n\x1a\n resto").expect("scritto");
    let voce = aggiungi_risorsa(dir.path(), DI_SERIE, &buono).expect("aggiunta");
    assert_eq!(voce.nome, "assets/trama.png");
    let seconda = aggiungi_risorsa(dir.path(), DI_SERIE, &buono).expect("aggiunta");
    assert_eq!(seconda.nome, "assets/trama-2.png");

    let bugiardo = fuori.path().join("finta.png");
    std::fs::write(&bugiardo, b"GIF89a").expect("scritto");
    assert!(aggiungi_risorsa(dir.path(), DI_SERIE, &bugiardo).is_err());

    let da = da_lavorare(dir.path(), DI_SERIE).expect("la bozza");
    let nomi: Vec<&str> = da.assets.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(nomi, ["trama-2.png", "trama.png"]);
}

#[test]
fn le_risorse_del_pacchetto_passano_nella_bozza_all_apertura() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let sorgente = rymembra_skin::PLAIN_SOURCE
        .replace("\"id\": \"plain\"", "\"id\": \"nocturne\"")
        .replacen(
            "\"id\": \"nocturne\",",
            "\"id\": \"nocturne\", \"fonts\": { \"Prova\": { \"src\": \"prova.woff2\" } },",
            1,
        );
    let bytes = rymembra_skin::write_skin_package(&rymembra_skin::package::WritePackageInput {
        source: &sorgente,
        preview: None,
        assets: &[rymembra_skin::package::SkinAsset {
            name: "prova.woff2".to_owned(),
            mime: "font/woff2",
            bytes: b"wOF2 finto".to_vec(),
        }],
    })
    .expect("impacchettata");
    let skin = dir.path().join("skin");
    std::fs::create_dir_all(&skin).expect("cartella");
    std::fs::write(skin.join("nocturne.ryskin"), bytes).expect("installata");

    apri_documento(dir.path(), "nocturne", true).expect("aperta");
    assert!(
        cartella_risorse(dir.path(), "nocturne")
            .join("prova.woff2")
            .is_file()
    );
    // E «Salva e usa» ha di nuovo il carattere da impacchettare.
    let (_, assets) = corredo_della_bozza(dir.path(), "nocturne").expect("corredo");
    assert_eq!(assets.len(), 1);
}

// ── le istantanee fuori dalla bozza ─────────────────────────────────────

#[test]
fn scartare_la_bozza_non_porta_via_le_istantanee() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    prendi_istantanea(
        &istantanee_di(dir.path(), DI_SERIE),
        rymembra_skin::PLAIN_SOURCE,
        Causa::Manuale,
    )
    .expect("presa");
    let bozza = cartella_bozze(dir.path()).join(DI_SERIE);
    std::fs::create_dir_all(&bozza).expect("bozza");
    std::fs::write(bozza.join("skin.json"), rymembra_skin::PLAIN_SOURCE).expect("scritta");
    std::fs::remove_dir_all(&bozza).expect("scartata");
    assert_eq!(
        elenco_istantanee(&istantanee_di(dir.path(), DI_SERIE)).len(),
        1
    );
}

#[test]
fn le_istantanee_della_2x_traslocano_fuori_dalla_bozza() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let vecchia = cartella_bozze(dir.path()).join(DI_SERIE).join("istantanee");
    prendi_istantanea(&vecchia, rymembra_skin::PLAIN_SOURCE, Causa::Salvata).expect("presa");
    let elenco = elenco_istantanee(&istantanee_di(dir.path(), DI_SERIE));
    assert_eq!(elenco.len(), 1);
    assert!(!vecchia.exists());
}

#[test]
fn una_skin_del_formato_1_si_apre_migrata_e_ricorda_com_era() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let v1 = include_str!("../../../../../core/rymembra-skin/tests/oro/scafalatura.v1.json");
    let id = rymembra_skin::parse_skin_json(v1).expect("si legge").id;
    let bozza = cartella_bozze(dir.path()).join(&id);
    std::fs::create_dir_all(&bozza).expect("bozza");
    std::fs::write(bozza.join("skin.json"), v1).expect("scritta");

    let aperta = apri_documento(dir.path(), &id, true).expect("aperta");
    assert!(aperta.contains("\"format\": 2"));
    let istantanee = elenco_istantanee(&istantanee_di(dir.path(), &id));
    assert_eq!(istantanee.first().map(|i| i.causa), Some("migrata"));
    // La seconda apertura trova la bozza già migrata: niente istantanea nuova.
    apri_documento(dir.path(), &id, true).expect("riaperta");
    assert_eq!(elenco_istantanee(&istantanee_di(dir.path(), &id)).len(), 1);
}

#[test]
fn rinominare_la_bozza_porta_con_se_le_istantanee() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let bozza = cartella_bozze(dir.path()).join("vecchio");
    std::fs::create_dir_all(&bozza).expect("bozza");
    std::fs::write(bozza.join("skin.json"), "{}").expect("scritta");
    prendi_istantanea(&istantanee_di(dir.path(), "vecchio"), "{}", Causa::Manuale).expect("presa");

    rinomina_bozza(dir.path(), "vecchio", "nuovo", None).expect("rinominata");
    assert!(
        cartella_bozze(dir.path())
            .join("nuovo")
            .join("skin.json")
            .is_file()
    );
    assert_eq!(
        elenco_istantanee(&istantanee_di(dir.path(), "nuovo")).len(),
        1
    );
    // Sopra una skin di serie non si scrive.
    assert!(rinomina_bozza(dir.path(), "nuovo", "plain", None).is_err());
}

// ── WP31: lo Studio che non perde lavoro ────────────────────────────────

/// Il manifest di serie con un altro id.
fn con_id(id: &str) -> String {
    rymembra_skin::PLAIN_SOURCE.replace("\"id\": \"plain\"", &format!("\"id\": \"{id}\""))
}

/// Il manifest di serie con un altro id e un carattere del pacchetto.
fn con_carattere(id: &str) -> String {
    con_id(id).replacen(
        &format!("\"id\": \"{id}\","),
        &format!("\"id\": \"{id}\", \"fonts\": {{ \"Prova\": {{ \"src\": \"prova.woff2\" }} }},"),
        1,
    )
}

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n resto";
const WOFF2: &[u8] = b"wOF2 finto con la firma giusta";

fn installa_qui(dir: &Path, sorgente: &str, assets: &[rymembra_skin::package::SkinAsset]) {
    let bytes = rymembra_skin::write_skin_package(&rymembra_skin::package::WritePackageInput {
        source: sorgente,
        preview: Some(PNG),
        assets,
    })
    .expect("impacchettata");
    crate::skin::installa(dir, &bytes).expect("installata");
}

fn carattere() -> rymembra_skin::package::SkinAsset {
    rymembra_skin::package::SkinAsset {
        name: "prova.woff2".to_owned(),
        mime: "font/woff2",
        bytes: WOFF2.to_vec(),
    }
}

#[test]
fn la_tela_si_compila_sotto_l_id_della_bozza() {
    // Il documento dice «nuovo-nome», la bozza sta ancora in «vecchio»:
    // i selettori e gli indirizzi delle risorse seguono la cartella.
    let sorgente = con_carattere("nuovo-nome");
    let esito = studio_valida(sorgente.clone(), Some("vecchio".to_owned()));
    assert!(esito.errori.is_empty(), "{:?}", esito.errori);
    assert_eq!(esito.radice, "vecchio");
    assert!(esito.css.contains(":root[data-skin='vecchio']"));
    assert!(
        esito.css.contains("/bozza/vecchio/prova.woff2"),
        "{}",
        esito.css
    );
    assert!(!esito.css.contains("data-skin='nuovo-nome'"));

    // Un id che il foglio non sa scrivere così com'è ricade sul documento.
    let ricaduta = studio_valida(sorgente.clone(), Some("con_trattino".to_owned()));
    assert_eq!(ricaduta.radice, "nuovo-nome");

    // La stessa domanda ha la stessa risposta, dai ricordi: anche i
    // millisecondi sono quelli della prima volta.
    let ancora = studio_valida(sorgente, Some("vecchio".to_owned()));
    assert_eq!(ancora.css, esito.css);
    assert_eq!(ancora.compilato_ms, esito.compilato_ms);
}

#[test]
fn la_prova_sull_app_porta_l_id_della_bozza() {
    let skin = compila_per_la_prova(&con_id("altro"), "mia").expect("compilata");
    assert_eq!(skin.id, "mia");
    assert!(skin.css.contains(":root[data-skin='mia']"));
    assert!(compila_per_la_prova("{ rotto", "mia").is_err());
}

#[test]
fn salvare_segue_l_id_del_documento_e_non_prende_quello_di_un_altro() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let manifest = |id: &str| cartella_bozze(dir.path()).join(id).join("skin.json");

    let primo = salva_bozza(dir.path(), "prima", &con_id("prima"), None).expect("salvata");
    assert_eq!(primo.id, "prima");
    assert!(primo.rinomina.is_none());

    // Il documento cambia id: la bozza lo segue, in un passo solo.
    let seconda = con_id("seconda");
    let esito = salva_bozza(dir.path(), "prima", &seconda, None).expect("salvata");
    assert_eq!(esito.id, "seconda");
    assert_eq!(
        std::fs::read_to_string(manifest("seconda")).expect("letta"),
        seconda
    );
    assert!(!cartella_bozze(dir.path()).join("prima").exists());

    // L'id di una skin installata di un altro: il testo si salva dov'era,
    // e il rifiuto torna indietro col suo codice invece di sparire.
    installa_qui(dir.path(), &con_id("altrui"), &[]);
    let rubata = con_id("altrui");
    let esito = salva_bozza(dir.path(), "seconda", &rubata, None).expect("salvata");
    assert_eq!(esito.id, "seconda");
    assert_eq!(
        esito.rinomina.as_ref().map(|e| e.code.as_str()),
        Some("skin.idConflict")
    );
    assert_eq!(
        std::fs::read_to_string(manifest("seconda")).expect("letta"),
        rubata
    );
    assert!(!cartella_bozze(dir.path()).join("altrui").exists());

    // Ma tornare all'id da cui lo Studio è partito si può.
    let esito = salva_bozza(dir.path(), "seconda", &rubata, Some("altrui")).expect("salvata");
    assert_eq!(esito.id, "altrui");
    assert!(esito.rinomina.is_none());

    // Un testo a metà si salva lo stesso, senza rinomine.
    let esito = salva_bozza(dir.path(), "altrui", "{ \"id\": ", None).expect("salvata");
    assert_eq!(esito.id, "altrui");
}

#[test]
fn una_skin_installata_che_non_passa_piu_si_apre_nello_studio() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let rotta = con_id("vecchia").replacen(
        "\"tokens\": {",
        "\"tokens\": { \"color.accents\": \"#ff0000\",",
        1,
    );
    let cartella = dir.path().join("skin");
    std::fs::create_dir_all(&cartella).expect("cartella");
    std::fs::write(
        cartella.join("vecchia.ryskin"),
        crate::skin::zip_a_mano(&[("skin.json", rotta.as_bytes()), ("assets/trama.png", PNG)]),
    )
    .expect("scritta");
    assert_eq!(
        apri_documento(dir.path(), "vecchia", true).expect("aperta"),
        rotta
    );
    assert!(
        cartella_risorse(dir.path(), "vecchia")
            .join("trama.png")
            .is_file()
    );
    // E la validazione dice dove.
    assert!(
        !studio_valida(rotta, Some("vecchia".to_owned()))
            .errori
            .is_empty()
    );

    // Un archivio che non si apre, con una bozza accanto: si apre la bozza.
    std::fs::write(cartella.join("guasta.ryskin"), b"non sono uno zip").expect("scritto");
    let bozza = cartella_bozze(dir.path()).join("guasta");
    std::fs::create_dir_all(&bozza).expect("bozza");
    std::fs::write(bozza.join("skin.json"), con_id("guasta")).expect("scritta");
    assert_eq!(
        da_lavorare(dir.path(), "guasta")
            .expect("la bozza")
            .sorgente,
        con_id("guasta")
    );
    // Senza la bozza il guasto si dice, invece di passare per un'assenza.
    std::fs::write(cartella.join("sola.ryskin"), b"non sono uno zip").expect("scritto");
    let guasto = da_lavorare(dir.path(), "sola").err().expect("un guasto");
    assert_eq!(guasto.code().kind().code(), "skin.packageCorrupt");
}

#[test]
fn un_tema_nuovo_porta_le_risorse_della_base_e_non_eredita_bozze_orfane() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    installa_qui(dir.path(), &con_carattere("base"), &[carattere()]);

    let figlia = con_carattere("figlia");
    let voce = crea_tema(dir.path(), "base", &figlia).expect("creato");
    assert_eq!(voce.id, "figlia");
    let letto = rymembra_skin::read_skin_package(
        &std::fs::read(dir.path().join("skin").join("figlia.ryskin")).expect("installato"),
    )
    .expect("si rilegge");
    assert_eq!(letto.assets.len(), 1, "il carattere della base");
    assert_eq!(
        letto.preview.as_deref(),
        Some(PNG),
        "la miniatura della base"
    );
    assert_eq!(
        std::fs::read_to_string(cartella_bozze(dir.path()).join("figlia").join("skin.json"))
            .expect("la bozza"),
        figlia
    );

    // Una bozza rimasta da una skin che non c'è più: il suo id è preso.
    std::fs::create_dir_all(cartella_bozze(dir.path()).join("orfana")).expect("orfana");
    let err = crea_tema(dir.path(), "base", &con_carattere("orfana")).expect_err("rifiutato");
    assert_eq!(err.code().kind().code(), "skin.idConflict");
    assert!(!dir.path().join("skin").join("orfana.ryskin").exists());
    let presi = occupati(dir.path());
    assert!(presi.contains(&"orfana".to_owned()) && presi.contains(&"figlia".to_owned()));
    // E quello di una skin installata, o di serie, pure.
    assert!(crea_tema(dir.path(), "base", &con_carattere("base")).is_err());
    assert!(crea_tema(dir.path(), "base", rymembra_skin::PLAIN_SOURCE).is_err());
}

// ── dusk: le rinomine, lo scarto, le istantanee ─────────────────────────

#[test]
fn l_id_dichiarato_segue_la_regola_del_formato() {
    // Due copie della stessa regola: questa prova le tiene legate. Un id
    // che il formato rifiuta non deve spostare la bozza, e uno che il
    // formato accetta sì.
    let lungo = "a".repeat(48);
    let troppo = "a".repeat(49);
    for candidato in [
        "a", "ab", "Ab", "a_b", "1ab", "-ab", "a-b", "notturno", "Notturno", "n0tte", &lungo,
        &troppo,
    ] {
        assert_eq!(
            id_del_formato(candidato),
            rymembra_skin::parse_skin_json(&con_id(candidato)).is_ok(),
            "«{candidato}»"
        );
    }
}

#[test]
fn un_id_con_maiuscole_non_sposta_la_bozza() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let esito = salva_bozza(dir.path(), "mia", &con_id("Notturno"), None).expect("salvata");
    assert_eq!(esito.id, "mia");
    assert!(esito.rinomina.is_none());
    assert!(
        cartella_bozze(dir.path())
            .join("mia")
            .join("skin.json")
            .is_file()
    );
    let nomi: Vec<String> = std::fs::read_dir(cartella_bozze(dir.path()))
        .expect("bozze")
        .flatten()
        .map(|voce| voce.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(nomi, ["mia"]);
}

#[test]
fn rinominare_solo_le_maiuscole_non_e_un_conflitto() {
    // Una bozza nata maiuscola dalle versioni di prima: correggerla in
    // minuscolo è rimettere a posto la propria cartella, non prendere
    // quella di un altro — su Windows le due sono la stessa.
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let vecchia = cartella_bozze(dir.path()).join("Notturno");
    std::fs::create_dir_all(&vecchia).expect("bozza");
    std::fs::write(vecchia.join("skin.json"), con_id("Notturno")).expect("scritta");
    prendi_istantanea(&istantanee_di(dir.path(), "Notturno"), "{}", Causa::Manuale).expect("presa");

    rinomina_bozza(dir.path(), "Notturno", "notturno", None).expect("rinominata");
    let nomi: Vec<String> = std::fs::read_dir(cartella_bozze(dir.path()))
        .expect("bozze")
        .flatten()
        .map(|voce| voce.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(nomi, ["notturno"]);
    assert_eq!(
        elenco_istantanee(&istantanee_di(dir.path(), "notturno")).len(),
        1
    );

    // E un tema nuovo non prende il nome di una cartella che differisce
    // solo per le maiuscole.
    let dir = tempfile::tempdir().expect("cartella temporanea");
    std::fs::create_dir_all(cartella_bozze(dir.path()).join("Notte")).expect("orfana");
    let err = crea_tema(dir.path(), DI_SERIE, &con_id("notte")).expect_err("rifiutato");
    assert_eq!(err.code().kind().code(), "skin.idConflict");
}

#[test]
fn rinominare_su_istantanee_orfane_non_le_mescola() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    // Le istantanee di una skin che non c'è più: né bozza, né pacchetto.
    prendi_istantanea(&istantanee_di(dir.path(), "notte"), "{}", Causa::Manuale).expect("presa");
    let bozza = cartella_bozze(dir.path()).join("vecchio");
    std::fs::create_dir_all(&bozza).expect("bozza");
    std::fs::write(bozza.join("skin.json"), "{}").expect("scritta");
    prendi_istantanea(&istantanee_di(dir.path(), "vecchio"), "{ }", Causa::Manuale).expect("presa");

    let err = rinomina_bozza(dir.path(), "vecchio", "notte", None).expect_err("rifiutata");
    assert_eq!(err.code().kind().code(), "skin.idConflict");
    assert!(bozza.join("skin.json").is_file(), "la bozza si è spostata");
    assert_eq!(
        elenco_istantanee(&istantanee_di(dir.path(), "vecchio")).len(),
        1
    );
    assert_eq!(
        elenco_istantanee(&istantanee_di(dir.path(), "notte")).len(),
        1
    );

    // Tornare all'id da cui lo Studio è partito sì: quelle istantanee sono
    // sue, e le due serie si uniscono voce per voce.
    rinomina_bozza(dir.path(), "vecchio", "notte", Some("notte")).expect("rinominata");
    assert_eq!(
        elenco_istantanee(&istantanee_di(dir.path(), "notte")).len(),
        2
    );
    assert!(!cartella_istantanee(dir.path(), "vecchio").exists());
}

#[test]
fn un_salvataggio_che_fallisce_non_sposta_la_bozza() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    // `skin.json` è una cartella: la scrittura del file fallisce su ogni
    // sistema.
    std::fs::create_dir_all(cartella_bozze(dir.path()).join("prima").join("skin.json"))
        .expect("cartella al posto del file");
    assert!(salva_bozza(dir.path(), "prima", &con_id("seconda"), None).is_err());
    assert!(cartella_bozze(dir.path()).join("prima").exists());
    assert!(!cartella_bozze(dir.path()).join("seconda").exists());
}

/// La seconda metà dello stesso guasto: la cartella si è spostata, e sono
/// le istantanee a non seguirla. Su Windows una cartella con dentro un file
/// aperto senza condivisione non si rinomina.
#[cfg(windows)]
#[test]
fn istantanee_che_non_si_spostano_non_lasciano_due_bozze() {
    use std::os::windows::fs::OpenOptionsExt as _;
    let dir = tempfile::tempdir().expect("cartella temporanea");
    salva_bozza(dir.path(), "prima", &con_id("prima"), None).expect("salvata");
    let istantanee = istantanee_di(dir.path(), "prima");
    prendi_istantanea(&istantanee, &con_id("prima"), Causa::Manuale).expect("presa");
    let una = std::fs::read_dir(&istantanee)
        .expect("istantanee")
        .flatten()
        .next()
        .expect("una")
        .path();
    let tenuta = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&una)
        .expect("aperta");

    let esito = salva_bozza(dir.path(), "prima", &con_id("seconda"), None).expect("salvata");
    drop(tenuta);
    assert_eq!(esito.id, "seconda", "la bozza sta in «seconda»: {esito:?}");
    assert!(!cartella_bozze(dir.path()).join("prima").exists());
    assert!(
        cartella_bozze(dir.path())
            .join("seconda")
            .join("skin.json")
            .is_file()
    );
}

#[test]
fn scartare_una_bozza_senza_pacchetto_non_cancella_niente() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    // Derivata da Plain, con l'id cambiato in «mia»: nessun pacchetto ha
    // quell'id, e nessuna skin di serie.
    let esito = salva_bozza(dir.path(), DI_SERIE, &con_id("mia"), Some(DI_SERIE)).expect("salvata");
    assert_eq!(esito.id, "mia");
    scrivi_chat(dir.path(), "mia", "{}").expect("chat");

    let err = scarta_bozza(dir.path(), "mia").expect_err("niente a cui tornare");
    assert_eq!(err.code().kind().code(), "skin.notFound");
    let bozza = cartella_bozze(dir.path()).join("mia");
    assert!(bozza.join("skin.json").is_file());
    assert!(bozza.join(FILE_CHAT).is_file());

    // Un pacchetto che non si legge non è un posto a cui tornare.
    let skin = dir.path().join("skin");
    std::fs::write(skin.join("mia.ryskin"), b"non sono uno zip").expect("scritto");
    assert!(scarta_bozza(dir.path(), "mia").is_err());
    assert!(bozza.join("skin.json").is_file());
}

#[test]
fn l_istantanea_della_migrazione_non_si_pota() {
    // È la sola copia del testo del formato 1: le venti istantanee che
    // girano non la devono spingere fuori.
    let dir = tempfile::tempdir().expect("cartella temporanea");
    prendi_istantanea(dir.path(), "{ \"format\": 1 }", Causa::Migrata).expect("presa");
    for giro in 0..(QUANTE_ISTANTANEE + 3) {
        prendi_istantanea(
            dir.path(),
            &format!("{{ \"giro\": {giro} }}"),
            Causa::Manuale,
        )
        .expect("presa");
    }
    let elenco = elenco_istantanee(dir.path());
    assert!(
        elenco.iter().any(|i| i.causa == "migrata"),
        "la migrazione è stata potata"
    );
    assert_eq!(
        elenco.iter().filter(|i| i.causa != "migrata").count(),
        QUANTE_ISTANTANEE
    );
}

#[test]
fn una_risorsa_dal_nome_lungo_si_misura_col_suo_nome() {
    use rymembra_skin::package::limits::{MAX_ARCHIVE_BYTES, MAX_ENTRY_BYTES};
    // La stima della cornice di una voce vale per nomi fino a ottanta
    // caratteri: con nomi più lunghi la risorsa entrava nella bozza e il
    // pacchetto poi non si scriveva.
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let fuori = tempfile::tempdir().expect("cartella dei file");
    let lungo = |n: char| n.to_string().repeat(120);
    let png = |byte: u64| {
        let mut dati = PNG.to_vec();
        dati.resize(usize::try_from(byte).expect("piccolo"), 0);
        dati
    };
    let risorse = cartella_risorse(dir.path(), DI_SERIE);
    std::fs::create_dir_all(&risorse).expect("cartella");
    for nome in ['a', 'b'] {
        std::fs::write(
            risorse.join(format!("{}.png", lungo(nome))),
            png(MAX_ENTRY_BYTES),
        )
        .expect("scritta");
    }
    // Con la cornice stimata (236 byte a voce) l'archivio resta cento byte
    // sotto il tetto; con quella vera dei tre nomi lunghi lo passa.
    let stimato = |voce: u64| voce + 236;
    let manifesto = u64::try_from(rymembra_skin::PLAIN_SOURCE.len()).expect("piccolo");
    let gia = 22 + stimato(manifesto) + 2 * stimato(MAX_ENTRY_BYTES);
    let nuova = MAX_ARCHIVE_BYTES - 100 - gia - 236;
    let file = fuori.path().join(format!("{}.png", lungo('c')));
    std::fs::write(&file, png(nuova)).expect("scritta");

    let err = aggiungi_risorsa(dir.path(), DI_SERIE, &file).expect_err("troppo grande");
    assert_eq!(err.code().kind().code(), "skin.tooLarge");
    assert_eq!(risorse_della_bozza(&risorse).len(), 2);
}

#[test]
fn una_conversazione_rifiutata_dice_perche() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let dettaglio = |testo: &str| match scrivi_chat(dir.path(), "mia", testo)
        .expect_err("rifiutata")
        .code()
    {
        ErrorCode::IpcPayloadInvalid { detail, .. } => detail.clone().unwrap_or_default(),
        altro => panic!("{altro:?}"),
    };
    let storta = dettaglio("non json");
    assert!(
        storta.contains("JSON") && !storta.contains("MB"),
        "{storta}"
    );
    let enorme = dettaglio(&format!(r#"{{"x":"{}"}}"#, "a".repeat(CHAT_MASSIMA)));
    assert!(
        enorme.contains("MB") && !enorme.contains("JSON"),
        "{enorme}"
    );
}

#[test]
fn scartare_la_bozza_di_una_skin_del_formato_1_non_aggiunge_istantanee() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let v1 = include_str!("../../../../../core/rymembra-skin/tests/oro/scafalatura.v1.json");
    let bytes = rymembra_skin::write_skin_package(&rymembra_skin::package::WritePackageInput {
        source: v1,
        preview: None,
        assets: &[],
    })
    .expect("impacchettata");
    crate::skin::installa(dir.path(), &bytes).expect("installata");
    let aperta = apri_documento(dir.path(), "scafalatura", true).expect("aperta");
    let serie = || elenco_istantanee(&istantanee_di(dir.path(), "scafalatura")).len();
    assert_eq!(serie(), 1, "la fotografia del testo del formato 1");
    let riaperta = scarta_bozza(dir.path(), "scafalatura").expect("scartata");
    assert_eq!(riaperta, aperta);
    assert_eq!(
        serie(),
        1,
        "lo scarto non fotografa di nuovo lo stesso testo"
    );
}
