//! Le prove della cache su disco.

use std::sync::atomic::{AtomicUsize, Ordering};

use rymembra_net::flusso::FlussoHttp;

use super::*;

/// Le richieste che una sorgente finta ha ricevuto.
type Richieste = Arc<Mutex<Vec<(u64, u64)>>>;

/// Un file in memoria che ricorda ogni richiesta.
#[derive(Debug)]
struct Finta {
    byte: Vec<u8>,
    dichiara: bool,
    richieste: Richieste,
}

impl Sorgente for Finta {
    fn pezzo(&self, da: u64, quanti: u64) -> Result<Pezzo, AppError> {
        self.richieste.lock().unwrap().push((da, quanti));
        let inizio = usize::try_from(da).unwrap().min(self.byte.len());
        let fine = inizio
            .saturating_add(usize::try_from(quanti).unwrap())
            .min(self.byte.len());
        Ok(Pezzo {
            byte: self.byte[inizio..fine].to_vec(),
            totale: self
                .dichiara
                .then(|| u64::try_from(self.byte.len()).unwrap()),
            url_finale: None,
        })
    }
}

/// I byte del brano finto `n`: diversi da un brano all'altro.
fn contenuto(quanti: usize, semina: u8) -> Vec<u8> {
    (0..quanti)
        .map(|n| u8::try_from(n % 251).unwrap().wrapping_add(semina))
        .collect()
}

/// Una sorgente finta, e le sue richieste.
fn finta(byte: &[u8], dichiara: bool) -> (Box<dyn Sorgente>, Richieste) {
    let richieste = Arc::new(Mutex::new(Vec::new()));
    let sorgente = Finta {
        byte: byte.to_vec(),
        dichiara,
        richieste: Arc::clone(&richieste),
    };
    (Box::new(sorgente), richieste)
}

fn chiave(id: &str, versione: &str) -> Chiave {
    Chiave {
        conto: "onedrive-k3x7q2ab".to_owned(),
        id: id.to_owned(),
        versione: versione.to_owned(),
    }
}

/// Aspetta che il filo della cache finisca quel che ha in coda: salvataggi e
/// ricomposizioni.
fn aspetta(cache: &Cache) {
    let (risposta, arrivo) = mpsc::sync_channel(1);
    cache.dentro.manda(Lavoro::Segnale(risposta));
    arrivo.recv().unwrap();
}

/// I file della cartella con queste estensioni.
fn file_con(cartella: &Path, estensioni: &[&str]) -> Vec<String> {
    let mut nomi: Vec<String> = std::fs::read_dir(cartella)
        .unwrap()
        .flatten()
        .map(|v| v.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| estensioni.contains(&e))
        })
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    nomi.sort();
    nomi
}

#[test]
fn un_pezzo_arrivato_si_serve_dal_disco_con_la_lunghezza() {
    let cartella = tempfile::tempdir().unwrap();
    let cache = Cache::apri(cartella.path(), TETTO_DI_SERIE, true).unwrap();
    let byte = contenuto(100_000, 0);
    let (sorgente, richieste) = finta(&byte, true);
    let con = cache.con(Some(chiave("F1", "v1")), sorgente);

    let primo = con.pezzo(0, 1000).unwrap();
    let secondo = con.pezzo(0, 1000).unwrap();
    assert_eq!(primo.byte, byte[..1000]);
    assert_eq!(secondo.byte, byte[..1000]);
    assert_eq!(
        secondo.totale,
        Some(100_000),
        "senza la lunghezza un flusso aperto dalla cache perderebbe il cursore"
    );
    assert_eq!(richieste.lock().unwrap().len(), 1, "la seconda dal disco");
    assert_eq!(cache.stato(), (1000, TETTO_DI_SERIE, true));
}

#[test]
fn intervalli_sparsi_e_sovrapposti_non_si_scaricano_due_volte() {
    let cartella = tempfile::tempdir().unwrap();
    let cache = Cache::apri(cartella.path(), TETTO_DI_SERIE, true).unwrap();
    let byte = contenuto(100_000, 0);
    let (sorgente, richieste) = finta(&byte, true);
    let con = cache.con(Some(chiave("F1", "v1")), sorgente);

    con.pezzo(0, 1000).unwrap();
    con.pezzo(5000, 1000).unwrap();
    // Da 500 c'è già: si serve dal disco quel che c'è di fila, e basta.
    let corto = con.pezzo(500, 5000).unwrap();
    assert_eq!(corto.byte, byte[500..1000]);
    // Da 1000 manca fino a 5000: dalla rete solo il buco.
    let buco = con.pezzo(1000, 4500).unwrap();
    assert_eq!(buco.byte, byte[1000..5000]);
    // Adesso [0, 6000) è tutto su disco, in tre pezzi.
    let tutto = con.pezzo(0, 6000).unwrap();
    assert_eq!(tutto.byte, byte[..6000]);

    assert_eq!(
        *richieste.lock().unwrap(),
        vec![(0, 1000), (5000, 1000), (1000, 4000)]
    );
    assert_eq!(cache.stato().0, 6000, "nessun byte due volte");
}

#[test]
fn senza_lunghezza_una_sovrapposizione_scrive_solo_quel_che_manca() {
    let cartella = tempfile::tempdir().unwrap();
    let cache = Cache::apri(cartella.path(), TETTO_DI_SERIE, true).unwrap();
    let byte = contenuto(10_000, 0);
    let (sorgente, richieste) = finta(&byte, false);
    let con = cache.con(Some(chiave("F1", "v1")), sorgente);
    con.pezzo(0, 1000).unwrap();
    // Senza lunghezza un pezzo corto direbbe «fine del file»: la richiesta va
    // alla rete intera, ma su disco va solo quel che mancava.
    let sovrapposto = con.pezzo(500, 1000).unwrap();
    assert_eq!(sovrapposto.byte, byte[500..1500]);
    assert_eq!(cache.stato().0, 1500);
    let dal_disco = con.pezzo(0, 1500).unwrap();
    assert_eq!(dal_disco.byte, byte[..1500]);
    assert_eq!(dal_disco.totale, None);
    assert_eq!(richieste.lock().unwrap().len(), 2);
}

#[test]
fn dopo_un_riavvio_la_cache_e_ancora_li() {
    let cartella = tempfile::tempdir().unwrap();
    let byte = contenuto(50_000, 0);
    {
        let cache = Cache::apri(cartella.path(), TETTO_DI_SERIE, true).unwrap();
        let (sorgente, _) = finta(&byte, true);
        let con = cache.con(Some(chiave("F1", "v1")), sorgente);
        con.pezzo(0, 2000).unwrap();
        con.pezzo(40_000, 2000).unwrap();
    }
    let cache = Cache::apri(cartella.path(), TETTO_DI_SERIE, true).unwrap();
    assert_eq!(cache.stato().0, 4000);
    let (sorgente, richieste) = finta(&byte, true);
    let con = cache.con(Some(chiave("F1", "v1")), sorgente);
    assert_eq!(con.pezzo(40_000, 2000).unwrap().byte, byte[40_000..42_000]);
    assert_eq!(con.pezzo(0, 2000).unwrap().byte, byte[..2000]);
    assert!(richieste.lock().unwrap().is_empty(), "tutto dal disco");
}

#[test]
fn un_indice_che_promette_byte_che_non_ci_sono_si_butta() {
    let cartella = tempfile::tempdir().unwrap();
    let byte = contenuto(50_000, 0);
    {
        let cache = Cache::apri(cartella.path(), TETTO_DI_SERIE, true).unwrap();
        let (sorgente, _) = finta(&byte, true);
        cache
            .con(Some(chiave("F1", "v1")), sorgente)
            .pezzo(0, 2000)
            .unwrap();
    }
    let dati = file_con(cartella.path(), &[DATI]);
    std::fs::write(cartella.path().join(&dati[0]), b"corto").unwrap();
    std::fs::write(cartella.path().join("orfano.dati"), b"nessuno").unwrap();
    std::fs::write(cartella.path().join("a-meta.indice.tmp"), b"{").unwrap();

    let cache = Cache::apri(cartella.path(), TETTO_DI_SERIE, true).unwrap();
    assert_eq!(cache.stato().0, 0);
    assert!(
        file_con(cartella.path(), &[DATI, INDICE, PROVVISORIO]).is_empty(),
        "via la copia rotta, l'orfano e il provvisorio"
    );
    let (sorgente, richieste) = finta(&byte, true);
    let pezzo = cache
        .con(Some(chiave("F1", "v1")), sorgente)
        .pezzo(0, 2000)
        .unwrap();
    assert_eq!(pezzo.byte, byte[..2000], "si riscarica, giusto");
    assert_eq!(richieste.lock().unwrap().len(), 1);
}

#[test]
fn oltre_il_tetto_se_ne_va_il_meno_usato() {
    let cartella = tempfile::tempdir().unwrap();
    let cache = Cache::apri(cartella.path(), 3000, true).unwrap();
    // Un brano non oltre il doppio del tetto: uno più lungo non entra.
    let byte = contenuto(6000, 0);
    let leggi = |id: &str| {
        let (sorgente, richieste) = finta(&byte, true);
        cache
            .con(Some(chiave(id, "v1")), sorgente)
            .pezzo(0, 1500)
            .unwrap();
        richieste.lock().unwrap().len()
    };
    assert_eq!(leggi("A"), 1);
    assert_eq!(leggi("B"), 1);
    assert_eq!(leggi("A"), 0, "A dal disco: adesso è la più recente");
    assert_eq!(leggi("C"), 1);
    assert_eq!(cache.stato().0, 3000, "dentro il tetto");
    assert_eq!(leggi("A"), 0, "A resta");
    assert_eq!(leggi("B"), 1, "B era la meno usata, e se n'è andata");
}

#[test]
fn una_copia_completa_resta_finche_non_e_misurata() {
    let cartella = tempfile::tempdir().unwrap();
    let cache = Cache::apri(cartella.path(), 3000, true).unwrap();
    let avvisi = Arc::new(Mutex::new(Vec::new()));
    let registro = Arc::clone(&avvisi);
    cache.al_completamento(move |chiave| registro.lock().unwrap().push(chiave.clone()));

    let brano = contenuto(2000, 7);
    let (sorgente, _) = finta(&brano, true);
    let a = chiave("A", "v1");
    cache.con(Some(a.clone()), sorgente).pezzo(0, 4000).unwrap();
    assert_eq!(
        *avvisi.lock().unwrap(),
        vec![a.clone()],
        "completa: si avvisa"
    );
    let copia = cache.copia_completa(&a).expect("la copia da misurare");
    assert_eq!(
        std::fs::read(&copia).unwrap(),
        brano,
        "un file come un altro"
    );

    // Un altro brano supera il tetto: la copia da misurare resta.
    let altro = contenuto(2000, 1);
    let (sorgente, _) = finta(&altro, true);
    cache
        .con(Some(chiave("B", "v1")), sorgente)
        .pezzo(0, 1000)
        .unwrap();
    assert_eq!(cache.stato().0, 3000);
    let (sorgente, _) = finta(&altro, true);
    cache
        .con(Some(chiave("C", "v1")), sorgente)
        .pezzo(0, 1500)
        .unwrap();
    assert!(cache.stato().0 > 3000, "sopra il tetto, ma sotto il doppio");
    assert!(
        cache.copia_completa(&a).is_some(),
        "non ancora misurata: resta"
    );

    cache.segna_misurata(&a);
    assert!(
        cache.copia_completa(&a).is_none(),
        "misurata: se ne va per prima"
    );
    assert!(cache.stato().0 <= 3000);
}

#[test]
fn oltre_il_doppio_del_tetto_anche_la_copia_da_misurare_se_ne_va() {
    // Tre brani, ognuno sotto il tetto (uno più lungo del doppio non entra
    // nemmeno: vedi `un_brano_oltre_il_doppio_del_tetto_non_entra_in_cache`).
    // A e B completi e da misurare, poi C a metà: 900 + 900 + 600 = 2400,
    // oltre il doppio di 1000.
    let cartella = tempfile::tempdir().unwrap();
    let cache = Cache::apri(cartella.path(), 1000, true).unwrap();
    let a = chiave("A", "v1");
    let b = chiave("B", "v1");
    for (chiave, semina) in [(&a, 3), (&b, 4)] {
        let brano = contenuto(900, semina);
        let (sorgente, _) = finta(&brano, true);
        cache
            .con(Some(chiave.clone()), sorgente)
            .pezzo(0, 900)
            .unwrap();
    }
    assert!(cache.copia_completa(&a).is_some());
    assert!(cache.copia_completa(&b).is_some());
    assert_eq!(cache.stato().0, 1800, "sopra il tetto, ma sotto il doppio");
    let terzo = contenuto(1000, 5);
    let (sorgente, _) = finta(&terzo, true);
    cache
        .con(Some(chiave("C", "v1")), sorgente)
        .pezzo(0, 600)
        .unwrap();
    assert!(
        cache.copia_completa(&a).is_none(),
        "oltre il doppio del tetto la misura non vale la cache intera"
    );
    assert!(
        cache.copia_completa(&b).is_some(),
        "se ne va solo quel che serve a tornare sotto il doppio"
    );
    assert_eq!(cache.stato().0, 1500);
}

#[test]
fn un_brano_oltre_il_doppio_del_tetto_non_entra_in_cache() {
    // Un'immagine CD da 450 MB col tetto di 200: oltre il doppio del tetto
    // `fai_posto` la butterebbe comunque al primo pezzo del brano dopo, e
    // intanto avrebbe svuotato la cache e riempito il disco. Passa diretta.
    let cartella = tempfile::tempdir().unwrap();
    let cache = Cache::apri(cartella.path(), 1000, true).unwrap();
    let altro = contenuto(800, 2);
    let (sorgente, _) = finta(&altro, true);
    let b = chiave("B", "v1");
    cache.con(Some(b.clone()), sorgente).pezzo(0, 800).unwrap();
    assert_eq!(cache.stato().0, 800);

    let grosso = contenuto(2500, 8);
    let (sorgente, richieste) = finta(&grosso, true);
    let con = cache.con(Some(chiave("A", "v1")), sorgente);
    assert_eq!(con.pezzo(0, 1000).unwrap().byte, grosso[..1000]);
    assert_eq!(con.pezzo(1000, 1500).unwrap().byte, grosso[1000..]);
    assert_eq!(con.pezzo(0, 1000).unwrap().byte, grosso[..1000]);
    assert_eq!(
        richieste.lock().unwrap().len(),
        3,
        "ogni pezzo dalla rete: niente su disco"
    );
    assert_eq!(cache.stato().0, 800, "solo l'altro brano, che resta");
    assert!(
        cache.copia_completa(&b).is_some(),
        "l'altro non è stato buttato"
    );
    // L'indice di B lo salva il filo.
    aspetta(&cache);
    assert_eq!(
        file_con(cartella.path(), &[DATI, INDICE]).len(),
        2,
        "i due file dell'altro brano, nessuno del grosso"
    );
}

#[test]
fn una_copia_arrivata_a_pezzi_sparsi_si_ricompone_in_ordine() {
    let cartella = tempfile::tempdir().unwrap();
    let cache = Cache::apri(cartella.path(), TETTO_DI_SERIE, true).unwrap();
    let brano = contenuto(300_000, 9);
    let (sorgente, _) = finta(&brano, true);
    let a = chiave("A", "v1");
    let con = cache.con(Some(a.clone()), sorgente);
    // Come un decodificatore: la coda, la testa, poi il resto in mezzo.
    con.pezzo(299_872, 128).unwrap();
    con.pezzo(0, 65_536).unwrap();
    assert!(cache.copia_completa(&a).is_none(), "ancora a metà");
    con.pezzo(65_536, 300_000).unwrap();
    // La ricompone il filo della cache, non chi ha scritto l'ultimo pezzo.
    aspetta(&cache);
    let copia = cache.copia_completa(&a).expect("completa");
    assert_eq!(
        std::fs::read(&copia).unwrap(),
        brano,
        "in ordine, senza buchi"
    );
    assert_eq!(cache.stato().0, 300_000);
    // E si legge ancora dalla cache come prima.
    assert_eq!(con.pezzo(299_000, 1000).unwrap().byte, brano[299_000..]);
}

#[test]
fn una_copia_arrivata_di_fila_non_si_ricompone() {
    // Come un flusso che suona dall'inizio: finestre una dopo l'altra. I
    // pezzi contigui nel brano e in `.dati` sono un segmento solo, e alla fine
    // la copia è già in ordine: niente ricomposizione, cioè niente brano
    // riletto e riscritto per intero col lucchetto della cache in mano.
    let cartella = tempfile::tempdir().unwrap();
    let cache = Cache::apri(cartella.path(), TETTO_DI_SERIE, true).unwrap();
    let brano = contenuto(3000, 6);
    let (sorgente, _) = finta(&brano, true);
    let a = chiave("A", "v1");
    let con = cache.con(Some(a.clone()), sorgente);
    let segmenti = || cache.dentro.blocca().copie[&a.nome()].segmenti.clone();

    con.pezzo(0, 1000).unwrap();
    con.pezzo(1000, 1000).unwrap();
    assert_eq!(
        segmenti(),
        vec![Segmento {
            da: 0,
            fino: 2000,
            pos: 0
        }],
        "due finestre di fila sono un segmento solo"
    );
    assert!(cache.copia_completa(&a).is_none(), "ancora a metà");
    con.pezzo(2000, 1000).unwrap();
    assert_eq!(segmenti().len(), 1);
    let copia = cache.copia_completa(&a).expect("completa");
    assert_eq!(std::fs::read(&copia).unwrap(), brano);
    assert_eq!(cache.stato().0, 3000);
}

#[test]
fn un_pezzo_che_cade_a_meta_non_sbilancia_il_conto() {
    let cartella = tempfile::tempdir().unwrap();
    let cache = Cache::apri(cartella.path(), TETTO_DI_SERIE, true).unwrap();
    // A: una copia qualunque, che resta.
    let (sorgente, _) = finta(&contenuto(1000, 1), true);
    cache
        .con(Some(chiave("A", "v1")), sorgente)
        .pezzo(0, 1000)
        .unwrap();
    // B senza lunghezza: due pezzi con un buco in mezzo, poi una richiesta
    // che li copre tutti e due e va alla rete intera.
    let brano = contenuto(10_000, 2);
    let (sorgente, _) = finta(&brano, false);
    let con = cache.con(Some(chiave("B", "v1")), sorgente);
    con.pezzo(0, 100).unwrap();
    con.pezzo(200, 100).unwrap();
    assert_eq!(cache.stato().0, 1200);

    // Due buchi da scrivere, [100, 200) e [300, 400): il disco si riempie
    // dopo il primo.
    SCRITTURE_PRIMA_DEL_GUASTO.with(|resta| resta.set(Some(1)));
    let pezzo = con.pezzo(0, 400);
    SCRITTURE_PRIMA_DEL_GUASTO.with(|resta| resta.set(None));
    assert_eq!(
        pezzo.unwrap().byte,
        brano[..400],
        "chi ascolta non sente il guasto"
    );
    assert_eq!(
        cache.stato().0,
        1000,
        "B se n'è andata, e dal conto è uscito quel che c'era entrato: resta A"
    );
    assert_eq!(file_con(cartella.path(), &[DATI]).len(), 1);
}

#[test]
fn spenta_passa_diretta_senza_scrivere() {
    let cartella = tempfile::tempdir().unwrap();
    let cache = Cache::apri(cartella.path(), TETTO_DI_SERIE, false).unwrap();
    let byte = contenuto(1000, 0);
    let (sorgente, richieste) = finta(&byte, true);
    let a = chiave("A", "v1");
    let con = cache.con(Some(a.clone()), sorgente);
    con.pezzo(0, 1000).unwrap();
    con.pezzo(0, 1000).unwrap();
    assert_eq!(richieste.lock().unwrap().len(), 2);
    assert!(file_con(cartella.path(), &[DATI, INDICE]).is_empty());
    assert_eq!(cache.stato(), (0, TETTO_DI_SERIE, false));
    assert!(cache.copia_completa(&a).is_none());

    // Riaccesa, ricomincia a tenere.
    cache.imposta(TETTO_DI_SERIE, true);
    con.pezzo(0, 1000).unwrap();
    con.pezzo(0, 1000).unwrap();
    assert_eq!(richieste.lock().unwrap().len(), 3);
}

#[test]
fn una_versione_nuova_non_legge_la_copia_vecchia_e_la_butta() {
    let cartella = tempfile::tempdir().unwrap();
    let cache = Cache::apri(cartella.path(), TETTO_DI_SERIE, true).unwrap();
    let vecchio = contenuto(1000, 1);
    let nuovo = contenuto(1000, 2);
    let (sorgente, _) = finta(&vecchio, true);
    cache
        .con(Some(chiave("A", "v1")), sorgente)
        .pezzo(0, 1000)
        .unwrap();
    assert_eq!(file_con(cartella.path(), &[DATI]).len(), 1);

    let (sorgente, richieste) = finta(&nuovo, true);
    let con = cache.con(Some(chiave("A", "v2")), sorgente);
    assert_eq!(
        cache.stato().0,
        0,
        "la copia della versione vecchia è andata"
    );
    assert_eq!(con.pezzo(0, 1000).unwrap().byte, nuovo);
    assert_eq!(
        richieste.lock().unwrap().len(),
        1,
        "dalla rete, non dal disco"
    );
    assert!(cache.copia_completa(&chiave("A", "v1")).is_none());
}

#[test]
fn senza_versione_non_si_tiene_niente() {
    let voce = VoceRemota {
        id: "A".to_owned(),
        genitore: None,
        nome: "a.flac".to_owned(),
        cartella: false,
        dimensione: 1000,
        modificato_ms: 0,
        versione: None,
        tag: None,
    };
    assert_eq!(Chiave::di("onedrive-k3x7q2ab", &voce), None);
    let cartella = tempfile::tempdir().unwrap();
    let cache = Cache::apri(cartella.path(), TETTO_DI_SERIE, true).unwrap();
    let byte = contenuto(1000, 0);
    let (sorgente, richieste) = finta(&byte, true);
    let con = cache.con(None, sorgente);
    con.pezzo(0, 100).unwrap();
    con.pezzo(0, 100).unwrap();
    assert_eq!(richieste.lock().unwrap().len(), 2);
    assert_eq!(cache.stato().0, 0);
}

#[test]
fn svuota_toglie_tutto() {
    let cartella = tempfile::tempdir().unwrap();
    let cache = Cache::apri(cartella.path(), TETTO_DI_SERIE, true).unwrap();
    let byte = contenuto(1000, 0);
    for id in ["A", "B"] {
        let (sorgente, _) = finta(&byte, true);
        cache
            .con(Some(chiave(id, "v1")), sorgente)
            .pezzo(0, 500)
            .unwrap();
    }
    assert_eq!(cache.stato().0, 1000);
    cache.svuota().unwrap();
    assert_eq!(cache.stato().0, 0);
    assert!(file_con(cartella.path(), &[DATI, INDICE]).is_empty());
}

/// La chiave di un brano di un conto qualunque.
fn chiave_del_conto(conto: &str, id: &str) -> Chiave {
    Chiave {
        conto: conto.to_owned(),
        id: id.to_owned(),
        versione: "v1".to_owned(),
    }
}

#[test]
fn togliere_un_conto_toglie_solo_le_sue_copie() {
    // «Scollega» promette (`PRIVACY.md`) di togliere anche i brani in cache
    // del conto: due copie del conto A e una del B, e restano solo i file di B.
    let cartella = tempfile::tempdir().unwrap();
    let cache = Cache::apri(cartella.path(), TETTO_DI_SERIE, true).unwrap();
    let a1 = chiave_del_conto("onedrive-aaaaaaaa", "A1");
    let a2 = chiave_del_conto("onedrive-aaaaaaaa", "A2");
    let b1 = chiave_del_conto("dropbox-bbbbbbbb", "B1");
    for (chiave, quanti) in [(&a1, 1000), (&a2, 700), (&b1, 400)] {
        let (sorgente, _) = finta(&contenuto(2000, 1), true);
        cache
            .con(Some(chiave.clone()), sorgente)
            .pezzo(0, quanti)
            .unwrap();
    }
    assert_eq!(cache.stato().0, 2100);

    assert_eq!(cache.togli_conto("onedrive-aaaaaaaa"), 2);
    assert_eq!(cache.stato().0, 400, "restano solo i byte di B");
    let mut di_b = vec![
        format!("{}.{DATI}", b1.nome()),
        format!("{}.{INDICE}", b1.nome()),
    ];
    di_b.sort();
    aspetta(&cache);
    assert_eq!(file_con(cartella.path(), &[DATI, INDICE]), di_b);

    drop(cache);
    let cache = Cache::apri(cartella.path(), TETTO_DI_SERIE, true).unwrap();
    assert_eq!(cache.stato().0, 400, "e alla riapertura non tornano");
}

#[test]
fn un_brano_del_conto_tolto_che_sta_suonando_non_torna_in_cache() {
    // Dopo «Scollega» il flusso di un brano che sta suonando può continuare:
    // un pezzo in volo, o il resto di un brano di OneDrive da un indirizzo
    // firmato, che non chiede più il permesso. Non deve ricreare la copia.
    let cartella = tempfile::tempdir().unwrap();
    let cache = Cache::apri(cartella.path(), TETTO_DI_SERIE, true).unwrap();
    let a = chiave_del_conto("onedrive-aaaaaaaa", "A1");
    let brano = contenuto(2000, 3);
    let (sorgente, _) = finta(&brano, true);
    let suona = cache.con(Some(a.clone()), sorgente);
    suona.pezzo(0, 500).unwrap();
    assert_eq!(cache.stato().0, 500);

    cache.togli_conto("onedrive-aaaaaaaa");
    assert_eq!(suona.pezzo(500, 1500).unwrap().byte, brano[500..]);
    assert_eq!(cache.stato().0, 0, "il resto del brano passa diretto");
    assert!(file_con(cartella.path(), &[DATI, INDICE]).is_empty());

    // Lo stesso conto ricollegato: un flusso nuovo torna a scrivere.
    let (sorgente, _) = finta(&brano, true);
    cache.con(Some(a), sorgente).pezzo(0, 500).unwrap();
    assert_eq!(cache.stato().0, 500);
}

#[test]
fn una_copia_da_misurare_si_ripropone_alla_riapertura() {
    // L'avviso di `al_completamento` parte una volta, quando la copia si
    // completa. Se l'applicazione si chiude prima della misura il segno resta
    // nell'indice, e alla riapertura qualcuno la deve poter richiedere: senza,
    // la copia resterebbe trattenuta per sempre.
    let cartella = tempfile::tempdir().unwrap();
    let a = chiave("A", "v1");
    {
        let cache = Cache::apri(cartella.path(), TETTO_DI_SERIE, true).unwrap();
        let brano = contenuto(2000, 7);
        let (sorgente, _) = finta(&brano, true);
        cache.con(Some(a.clone()), sorgente).pezzo(0, 4000).unwrap();
        // B a metà: non è completa, e non c'è niente da misurare.
        let (sorgente, _) = finta(&brano, true);
        cache
            .con(Some(chiave("B", "v1")), sorgente)
            .pezzo(0, 1000)
            .unwrap();
        assert_eq!(cache.da_misurare(), vec![a.clone()]);
    }
    let cache = Cache::apri(cartella.path(), TETTO_DI_SERIE, true).unwrap();
    assert_eq!(
        cache.da_misurare(),
        vec![a.clone()],
        "riaperta, la copia completa e non misurata si ripropone"
    );
    cache.segna_misurata(&a);
    assert!(cache.da_misurare().is_empty(), "misurata: non più");
    drop(cache);
    let cache = Cache::apri(cartella.path(), TETTO_DI_SERIE, true).unwrap();
    assert!(
        cache.da_misurare().is_empty(),
        "nemmeno alla riapertura dopo"
    );
}

#[test]
fn un_flusso_dalla_cache_suona_due_volte_e_la_seconda_senza_rete() {
    let cartella = tempfile::tempdir().unwrap();
    let cache = Cache::apri(cartella.path(), TETTO_DI_SERIE, true).unwrap();
    let brano = contenuto(700_000, 5);
    let completati = Arc::new(AtomicUsize::new(0));
    let contatore = Arc::clone(&completati);
    cache.al_completamento(move |_| {
        contatore.fetch_add(1, Ordering::SeqCst);
    });
    for volta in 0..2 {
        let (sorgente, richieste) = finta(&brano, true);
        let con = cache.con(Some(chiave("A", "v1")), sorgente);
        let mut flusso = FlussoHttp::da(Box::new(con)).unwrap();
        assert_eq!(flusso.lunghezza(), Some(700_000));
        let mut letti = Vec::new();
        std::io::Read::read_to_end(&mut flusso, &mut letti).unwrap();
        assert_eq!(letti, brano);
        let fine = std::io::Seek::seek(&mut flusso, SeekFrom::End(-10)).unwrap();
        assert_eq!(fine, 699_990);
        if volta == 1 {
            assert!(richieste.lock().unwrap().is_empty(), "la seconda dal disco");
        }
    }
    assert_eq!(completati.load(Ordering::SeqCst), 1, "un avviso per copia");
}

#[test]
fn i_buchi_sono_quel_che_i_segmenti_non_coprono() {
    let presi = [(10, 20), (30, 40)];
    assert_eq!(buchi(&presi, 0, 50), vec![(0, 10), (20, 30), (40, 50)]);
    assert_eq!(buchi(&presi, 12, 18), vec![]);
    assert_eq!(buchi(&presi, 15, 35), vec![(20, 30)]);
    assert_eq!(buchi(&[], 5, 9), vec![(5, 9)]);
    // Segmenti e prenotazioni insieme possono sovrapporsi nell'elenco.
    assert_eq!(buchi(&[(10, 20), (15, 25)], 0, 30), vec![(0, 10), (25, 30)]);
}

#[test]
fn chi_chiede_byte_che_un_altro_sta_scrivendo_non_li_riscrive() {
    let cartella = tempfile::tempdir().unwrap();
    let cache = Cache::apri(cartella.path(), TETTO_DI_SERIE, true).unwrap();
    let brano = contenuto(10_000, 3);
    let a = chiave("A", "v1");
    let nome = a.nome();
    let pezzo = |da: usize, fino: usize| Pezzo {
        byte: brano[da..fino].to_vec(),
        totale: Some(10_000),
        url_finale: None,
    };
    // Il primo prenota [1000, 3000) e non ha ancora scritto.
    let primo = cache
        .dentro
        .prenota(&a, &nome, 0, 1000, &pezzo(1000, 3000))
        .unwrap();
    // Il secondo arriva con [0, 4000): gli restano solo i due lati.
    let secondo = cache
        .dentro
        .prenota(&a, &nome, 0, 0, &pezzo(0, 4000))
        .unwrap();
    let lati: Vec<(u64, u64)> = secondo.iter().map(|s| (s.da, s.fino)).collect();
    assert_eq!(lati, vec![(0, 1000), (3000, 4000)]);
    // Nessuno legge un posto prenotato e vuoto.
    assert!(matches!(
        cache.dentro.dal_disco(&nome, 1000, 100),
        DalDisco::Manca(_)
    ));
    let dati = percorso(cartella.path(), &nome, DATI);
    // Il secondo finisce per primo.
    scrivi_fuori(&dati, 0, &pezzo(0, 4000), &secondo).unwrap();
    cache.dentro.registra(&nome, &secondo, true);
    scrivi_fuori(&dati, 1000, &pezzo(1000, 3000), &primo).unwrap();
    cache.dentro.registra(&nome, &primo, true);
    assert_eq!(cache.stato().0, 4000, "ogni byte una volta sola");
    let (sorgente, richieste) = finta(&brano, true);
    let con = cache.con(Some(a), sorgente);
    let mut letti = Vec::new();
    while letti.len() < 4000 {
        let da = u64::try_from(letti.len()).unwrap();
        letti.extend(con.pezzo(da, 4000 - da).unwrap().byte);
    }
    assert_eq!(letti, brano[..4000]);
    assert!(richieste.lock().unwrap().is_empty(), "tutto dal disco");
}

#[test]
fn una_copia_tolta_mentre_si_legge_se_ne_va_dopo_la_lettura() {
    let cartella = tempfile::tempdir().unwrap();
    let cache = Cache::apri(cartella.path(), TETTO_DI_SERIE, true).unwrap();
    let brano = contenuto(2000, 4);
    let a = chiave_del_conto("onedrive-aaaaaaaa", "A1");
    let (sorgente, _) = finta(&brano, true);
    cache.con(Some(a.clone()), sorgente).pezzo(0, 1000).unwrap();
    let nome = a.nome();
    aspetta(&cache);
    // Una lettura cominciata e non finita: la copia è in uso.
    let DalDisco::Leggi { tratti, .. } = cache.dentro.dal_disco(&nome, 0, 500) else {
        panic!("dal disco");
    };
    std::thread::scope(|filo| {
        // «Scollega», da un altro filo: segna la copia e aspetta la lettura.
        let scollega = filo.spawn(|| cache.togli_conto("onedrive-aaaaaaaa"));
        while !cache.dentro.blocca().copie[&nome].tolta {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(cache.stato().0, 0, "tolta: per il tetto non c'è più");
        assert!(cache.copia_completa(&a).is_none());
        assert_eq!(
            file_con(cartella.path(), &[DATI]).len(),
            1,
            "ma i byte restano finché la lettura non finisce"
        );
        let dati = percorso(cartella.path(), &nome, DATI);
        assert_eq!(leggi_tratti(&dati, &tratti).unwrap(), brano[..500]);
        assert!(!scollega.is_finished(), "«Scollega» aspetta la lettura");
        cache.dentro.letto(&nome, true);
        assert_eq!(scollega.join().unwrap(), 1);
    });
    assert!(
        file_con(cartella.path(), &[DATI, INDICE]).is_empty(),
        "quando «Scollega» torna, i file non ci sono più"
    );
    assert_eq!(cache.dentro.blocca().usati, 0);
    assert_eq!(cache.dentro.blocca().in_uscita, 0);
}

#[test]
fn l_indice_non_si_riscrive_a_ogni_pezzo() {
    let cartella = tempfile::tempdir().unwrap();
    let cache = Cache::apri(cartella.path(), TETTO_DI_SERIE, true).unwrap();
    let brano = contenuto(6 * 1024 * 1024, 2);
    let (sorgente, _) = finta(&brano, true);
    let a = chiave("A", "v1");
    let indice = vec![format!("{}.{INDICE}", a.nome())];
    let con = cache.con(Some(a.clone()), sorgente);
    con.pezzo(0, 1000).unwrap();
    con.pezzo(5000, 1000).unwrap();
    aspetta(&cache);
    assert!(
        file_con(cartella.path(), &[INDICE]).is_empty(),
        "due pezzi piccoli: l'indice aspetta"
    );
    drop(con);
    aspetta(&cache);
    assert_eq!(
        file_con(cartella.path(), &[INDICE]),
        indice,
        "il flusso si è chiuso: l'indice c'è"
    );

    let (sorgente, _) = finta(&brano, true);
    let b = chiave("B", "v1");
    let con = cache.con(Some(b.clone()), sorgente);
    con.pezzo(0, 5 * 1024 * 1024).unwrap();
    aspetta(&cache);
    let mut entrambi = vec![indice[0].clone(), format!("{}.{INDICE}", b.nome())];
    entrambi.sort();
    assert_eq!(
        file_con(cartella.path(), &[INDICE]),
        entrambi,
        "oltre i quattro megabyte l'indice si salva anche col flusso aperto"
    );
}

#[test]
fn una_copia_coperta_e_non_ricomposta_si_ricompone_alla_riapertura() {
    // Come dopo una chiusura fra l'ultimo pezzo e la ricomposizione: un
    // indice di pezzi in fila che coprono tutto il brano.
    let cartella = tempfile::tempdir().unwrap();
    let brano = contenuto(1000, 6);
    let a = chiave("A", "v1");
    let nome = a.nome();
    let mut in_fila = brano[500..].to_vec();
    in_fila.extend_from_slice(&brano[..500]);
    std::fs::write(percorso(cartella.path(), &nome, DATI), &in_fila).unwrap();
    let indice = Indice {
        conto: a.conto.clone(),
        id: a.id.clone(),
        versione: a.versione.clone(),
        totale: Some(1000),
        segmenti: vec![
            Segmento {
                da: 0,
                fino: 500,
                pos: 500,
            },
            Segmento {
                da: 500,
                fino: 1000,
                pos: 0,
            },
        ],
        usato_ms: 0,
        da_misurare: false,
        formato: FORMATO_INDICE,
    };
    scrivi_indice(cartella.path(), &nome, &indice).unwrap();

    let cache = Cache::apri(cartella.path(), TETTO_DI_SERIE, true).unwrap();
    aspetta(&cache);
    let copia = cache.copia_completa(&a).expect("ricomposta all'apertura");
    assert_eq!(std::fs::read(copia).unwrap(), brano);
    assert_eq!(cache.da_misurare(), vec![a]);
    assert_eq!(cache.stato().0, 1000);
}

#[test]
fn due_lettori_e_una_ricomposizione_non_si_pestano_i_piedi() {
    const LUNGO: usize = 3_000_000;
    let cartella = tempfile::tempdir().unwrap();
    let cache = Cache::apri(cartella.path(), TETTO_DI_SERIE, true).unwrap();
    let completati = Arc::new(AtomicUsize::new(0));
    let contatore = Arc::clone(&completati);
    cache.al_completamento(move |_| {
        contatore.fetch_add(1, Ordering::SeqCst);
    });
    let brano = contenuto(LUNGO, 8);
    let a = chiave("A", "v1");
    {
        // In fila e fuori ordine: la coda, poi la testa. Il resto lo
        // scrivono i due lettori, e chi chiude l'ultimo buco fa partire la
        // ricomposizione mentre l'altro legge ancora.
        let (sorgente, _) = finta(&brano, true);
        let con = cache.con(Some(a.clone()), sorgente);
        con.pezzo(2_900_000, 100_000).unwrap();
        con.pezzo(0, 700_000).unwrap();
    }
    std::thread::scope(|filo| {
        for lettore in 0..2_usize {
            let (cache, brano, a) = (&cache, &brano, &a);
            filo.spawn(move || {
                let (sorgente, _) = finta(brano, true);
                let con = cache.con(Some(a.clone()), sorgente);
                for giro in 0..3_usize {
                    let mut da = (lettore * 37_000 + giro * 11_000) % 200_000;
                    while da < LUNGO {
                        let pezzo = con.pezzo(u64::try_from(da).unwrap(), 96 * 1024).unwrap();
                        assert_eq!(pezzo.totale, Some(u64::try_from(LUNGO).unwrap()));
                        assert!(!pezzo.byte.is_empty());
                        let fine = da + pezzo.byte.len();
                        assert!(pezzo.byte == brano[da..fine], "byte giusti da {da}");
                        da = fine;
                    }
                }
            });
        }
    });
    aspetta(&cache);
    let copia = cache.copia_completa(&a).expect("ricomposta");
    assert!(std::fs::read(copia).unwrap() == brano, "in ordine");
    assert_eq!(cache.stato().0, u64::try_from(LUNGO).unwrap());
    assert_eq!(completati.load(Ordering::SeqCst), 1, "un avviso solo");
    let segmenti = cache.dentro.blocca().copie[&a.nome()].segmenti.clone();
    assert_eq!(segmenti.len(), 1);
}

#[test]
fn il_nome_dei_file_non_dice_cosa_c_e_dentro() {
    let a = chiave("01 Canzone segreta.flac", "v1");
    let nome = a.nome();
    assert_eq!(nome.len(), 32);
    assert!(nome.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_ne!(nome, chiave("01 Canzone segreta.flac", "v2").nome());
    // Le parti non si confondono fra loro.
    let unita = Chiave {
        conto: "ab".to_owned(),
        id: "c".to_owned(),
        versione: "v".to_owned(),
    };
    let spostata = Chiave {
        conto: "a".to_owned(),
        id: "bc".to_owned(),
        versione: "v".to_owned(),
    };
    assert_ne!(unita.nome(), spostata.nome());
}

/// Il difetto della 3.2.0 lasciato da un crash: la copia era già stata messa in
/// ordine, ma l'indice era ancora quello dei pezzi in fila. Ricomposta di nuovo
/// seguendo quell'indice, ne uscirebbe un brano mescolato, servito e misurato
/// così. Un indice senza formato è di prima della correzione: si butta, con la
/// sua copia, e il brano si riscarica (R06b-8).
#[test]
fn un_indice_a_pezzi_sopra_una_copia_gia_in_ordine_non_si_ricompone() {
    let cartella = tempfile::tempdir().unwrap();
    let brano = contenuto(1000, 9);
    let a = chiave("A", "v1");
    let nome = a.nome();
    // La copia è già in ordine…
    std::fs::write(percorso(cartella.path(), &nome, DATI), &brano).unwrap();
    // …ma l'indice, della 3.2.0 (senza `formato`), dice ancora dove stavano i
    // pezzi prima.
    let vecchio = serde_json::json!({
        "conto": a.conto,
        "id": a.id,
        "versione": a.versione,
        "totale": 1000,
        "segmenti": [
            { "da": 0, "fino": 500, "pos": 500 },
            { "da": 500, "fino": 1000, "pos": 0 },
        ],
        "usato_ms": 0,
        "da_misurare": false,
    });
    std::fs::write(
        percorso(cartella.path(), &nome, INDICE),
        serde_json::to_vec(&vecchio).unwrap(),
    )
    .unwrap();

    let cache = Cache::apri(cartella.path(), TETTO_DI_SERIE, true).unwrap();
    aspetta(&cache);
    if let Some(copia) = cache.copia_completa(&a) {
        assert_eq!(
            std::fs::read(copia).unwrap(),
            brano,
            "una copia ricomposta da un indice vecchio è un brano mescolato"
        );
    }
    let (sorgente, _) = finta(&brano, true);
    let con = cache.con(Some(a), sorgente);
    assert_eq!(con.pezzo(0, 1000).unwrap().byte, brano, "i byte del brano");
}

/// Due processi sulla stessa cartella (due avvii con la stessa
/// `RYMEMBRA_DATI`): il secondo non deve cancellare il `.dati` che il primo sta
/// scrivendo e non ha ancora indicizzato, né scriverci accanto. Si apre spento,
/// e lo dice (R28-8).
#[test]
fn una_seconda_apertura_della_stessa_cartella_si_rifiuta() {
    let cartella = tempfile::tempdir().unwrap();
    let byte = contenuto(50_000, 3);
    let prima = Cache::apri(cartella.path(), TETTO_DI_SERIE, true).unwrap();
    let (sorgente, _) = finta(&byte, true);
    let in_corso = prima.con(Some(chiave("F1", "v1")), sorgente);
    in_corso.pezzo(0, 2000).unwrap();
    let dati = file_con(cartella.path(), &[DATI]);
    assert_eq!(dati.len(), 1, "il primo processo sta scrivendo");

    let seconda = Cache::apri(cartella.path(), TETTO_DI_SERIE, true).unwrap();
    assert_eq!(
        file_con(cartella.path(), &[DATI]),
        dati,
        "il .dati del primo resta"
    );
    assert!(seconda.condivisa(), "la seconda sa di non essere sola");
    assert!(!seconda.stato().2, "e passa diretta");
    // Accenderla dalle impostazioni non la accende: la cartella è dell'altro.
    seconda.imposta(TETTO_DI_SERIE, true);
    assert!(!seconda.stato().2);
    let (sorgente, richieste) = finta(&byte, true);
    let diretta = seconda.con(Some(chiave("F2", "v1")), sorgente);
    assert_eq!(diretta.pezzo(0, 10).unwrap().byte, byte[..10]);
    assert_eq!(richieste.lock().unwrap().len(), 1);
    assert_eq!(
        file_con(cartella.path(), &[DATI]),
        dati,
        "la seconda non scrive nella cartella dell'altra"
    );
    drop(in_corso);
    drop(prima);
    drop(seconda);
    // Chiusa la prima, la cartella torna di chi la apre.
    let terza = Cache::apri(cartella.path(), TETTO_DI_SERIE, true).unwrap();
    assert!(!terza.condivisa());
    assert!(terza.stato().2);
}
