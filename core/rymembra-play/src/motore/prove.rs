//! Le prove del motore, venute dal fondo di `motore.rs`.

use std::sync::mpsc::Sender;

use rymembra_domain::errors::ErrorCodeKind;

use super::maniglia::voce_su_filo;
use super::*;
use crate::volume_uscita;

#[test]
fn i_fotogrammi_da_campioni_interlacciati() {
    assert_eq!(fotogrammi(8, 2), 4);
    assert_eq!(fotogrammi(9, 2), 4);
    // Zero canali non deve dividere per zero.
    assert_eq!(fotogrammi(8, 0), 8);
}

#[test]
fn i_millisecondi_dai_fotogrammi() {
    assert_eq!(ms_da_fotogrammi(48_000, 48_000), 1_000);
    assert_eq!(ms_da_fotogrammi(0, 44_100), 0);
    // Frequenza zero non deve dividere per zero.
    assert_eq!(ms_da_fotogrammi(1, 0), 1_000);
}

/// I salti di un lotto, come restano dopo [`compatta`].
// ── l'avvio programmato di «Ascolta insieme» ────────────────────────────
//
// `Motore::riprendi_a` apre un dispositivo, quindi qui si prova il suo
// conto: `istante_d_avvio`, con l'orologio passato a mano.

#[test]
fn l_avvio_toglie_la_latenza_dichiarata() {
    // Sommata invece che tolta, chi ospita con 150 ms dichiarati suonerebbe
    // 300 ms fuori passo dai membri.
    assert_eq!(
        istante_d_avvio(10_000_000_000, 150, 9_000_000_000),
        Ok(9_850_000_000)
    );
    let q = 50_000_000_000u64;
    assert_eq!(istante_d_avvio(q, 150, q - 1), Ok(q - 150_000_000));
}

#[test]
fn una_latenza_negativa_sposta_l_avvio_avanti() {
    assert_eq!(
        istante_d_avvio(10_000_000_000, -50, 9_000_000_000),
        Ok(10_050_000_000)
    );
}

#[test]
fn fino_a_un_secondo_nel_passato_si_parte_oltre_si_rifiuta() {
    assert_eq!(
        istante_d_avvio(5_000_000_000, 0, 6_000_000_000),
        Ok(5_000_000_000)
    );
    assert_eq!(
        istante_d_avvio(5_000_000_000, 0, 6_000_000_001),
        Err(RifiutoAvvio::NelPassato)
    );
    let q = 50_000_000_000u64;
    assert_eq!(istante_d_avvio(q, 0, q + 999_000_000), Ok(q));
    assert_eq!(
        istante_d_avvio(q, 0, q + 1_000_000_001),
        Err(RifiutoAvvio::NelPassato)
    );
}

#[test]
fn un_avvio_che_cadrebbe_a_zero_diventa_uno() {
    // Zero è «nessun avvio»: un conto che ci cade non deve spegnere
    // l'avvio programmato.
    assert_eq!(istante_d_avvio(100, 1, 0), Ok(1));
    assert_eq!(istante_d_avvio(0, 10, 0), Ok(1));
    assert_eq!(
        istante_d_avvio(0, 10, 2_000_000_000),
        Err(RifiutoAvvio::NelPassato)
    );
}

fn salti(lotto: &[Comando]) -> Vec<u64> {
    lotto
        .iter()
        .filter_map(|c| match c {
            Comando::VaiA(ms) => Some(*ms),
            _ => None,
        })
        .collect()
}

#[test]
fn di_piu_salti_insieme_ne_resta_uno() {
    // Sei trascinate sulla barra del tempo.
    let mut lotto = vec![
        Comando::VaiA(10_000),
        Comando::VaiA(220_000),
        Comando::VaiA(48_000),
        Comando::VaiA(300_000),
        Comando::VaiA(91_000),
        Comando::VaiA(175_000),
    ];
    compatta(&mut lotto);
    assert_eq!(salti(&lotto), vec![175_000]);
    assert_eq!(lotto.len(), 1);
}

#[test]
fn fra_due_salti_quel_che_non_guarda_la_posizione_resta() {
    // Il volume non sa dove si è: non salva il salto che lo precede, e non
    // si fa buttare da quello che lo segue.
    let mut lotto = vec![
        Comando::VaiA(10_000),
        Comando::Volume {
            volume: 0.5,
            muto: false,
        },
        Comando::VaiA(90_000),
    ];
    compatta(&mut lotto);
    assert_eq!(salti(&lotto), vec![90_000]);
    assert!(matches!(lotto[0], Comando::Volume { .. }));
    assert_eq!(lotto.len(), 2);
}

#[test]
fn un_salto_che_finisce_sotto_un_altro_brano_non_si_fa() {
    // Chi è stufo di aspettare mette su un altro brano: il salto di prima
    // è lavoro che verrebbe buttato appena finito.
    let mut lotto = vec![Comando::VaiA(220_000), Comando::Ferma];
    compatta(&mut lotto);
    assert!(salti(&lotto).is_empty());
    assert_eq!(lotto.len(), 1);
}

#[test]
fn il_salto_dopo_il_cambio_di_brano_resta() {
    // L'ordine conta: questo salto è per il brano nuovo, ed è la ripresa
    // dal punto in cui lo si era lasciato.
    let mut lotto = vec![Comando::Ferma, Comando::VaiA(45_000)];
    compatta(&mut lotto);
    assert_eq!(salti(&lotto), vec![45_000]);
    assert_eq!(lotto.len(), 2);
}

#[test]
fn compattare_non_tocca_un_lotto_senza_salti() {
    let mut lotto = vec![
        Comando::Volume {
            volume: 1.0,
            muto: false,
        },
        Comando::Dissolvenza { ms: 20 },
        Comando::Ferma,
    ];
    compatta(&mut lotto);
    assert_eq!(lotto.len(), 3);
    let mut vuoto: Vec<Comando> = Vec::new();
    compatta(&mut vuoto);
    assert!(vuoto.is_empty());
}

#[test]
fn una_posizione_nuova_e_vuota() {
    let p = Posizione::default();
    assert_eq!(p.track_id, None);
    assert_eq!(p.ms, 0);
}

#[test]
fn la_curva_arriva_al_bersaglio_e_ci_resta() {
    let mut correnti = [0.0f32; BANDE];
    let bersaglio = [12.0f32; BANDE];
    for rimasti in (0..PASSI_EQ).rev() {
        avvicina(&mut correnti, &bersaglio, rimasti);
    }
    for (banda, valore) in correnti.iter().enumerate() {
        assert!(
            (valore - 12.0).abs() < f32::EPSILON,
            "banda {banda} è a {valore}, non a 12"
        );
    }
    // Un passo in più non la sposta.
    avvicina(&mut correnti, &bersaglio, 0);
    assert!((correnti.first().copied().unwrap_or(0.0) - 12.0).abs() < f32::EPSILON);
}

#[test]
fn i_passi_intermedi_stanno_in_mezzo() {
    // Il punto di tutto l'esercizio: nessun salto. Ogni passo si muove, e
    // nessuno arriva prima del tempo.
    let mut correnti = [0.0f32; BANDE];
    let bersaglio = [12.0f32; BANDE];
    let mut precedente = 0.0f32;
    for rimasti in (1..PASSI_EQ).rev() {
        avvicina(&mut correnti, &bersaglio, rimasti);
        let adesso = correnti.first().copied().unwrap_or(0.0);
        assert!(adesso > precedente, "fermo a {adesso}");
        assert!(adesso < 12.0, "arrivato troppo presto a {adesso}");
        precedente = adesso;
    }
}

// ── il passaggio fra due brani, con due decodificatori veri ─────────────
//
// Queste prove costruiscono un `Contesto` come quello che gira davvero —
// stessi decodificatori, stessa dissolvenza, stesso anello — e gli danno
// due WAV fatti a mano invece di due file. Manca solo il dispositivo, che
// qui non serve: quel che si vuole guardare è la sequenza di campioni che
// *sarebbe* uscita, ed è esattamente quel che finisce nell'anello.

/// Byte in memoria che si comportano come un file.
///
/// Il motore vuole un [`crate::decodifica::Flusso`] e non un percorso: è la
/// stessa giuntura che su Android porta i byte dallo Storage Access
/// Framework, e qui porta un WAV costruito a mano senza toccare il disco.
struct Byte(std::io::Cursor<Vec<u8>>);

impl std::io::Read for Byte {
    fn read(&mut self, dove: &mut [u8]) -> std::io::Result<usize> {
        std::io::Read::read(&mut self.0, dove)
    }
}

impl std::io::Seek for Byte {
    fn seek(&mut self, da: std::io::SeekFrom) -> std::io::Result<u64> {
        std::io::Seek::seek(&mut self.0, da)
    }
}

impl crate::decodifica::Flusso for Byte {
    fn lunghezza(&self) -> Option<u64> {
        u64::try_from(self.0.get_ref().len()).ok()
    }
}

/// Un WAV mono a 16 bit con i campioni dati.
#[expect(
    clippy::cast_possible_truncation,
    reason = "i campioni di prova stanno fra -1 e 1: per costruzione il \
              prodotto entra in un i16"
)]
fn wav(frequenza: u32, campioni: &[f32]) -> Vec<u8> {
    let dati = u32::try_from(campioni.len() * 2).unwrap_or(0);
    let mut byte = Vec::with_capacity(44 + campioni.len() * 2);
    byte.extend_from_slice(b"RIFF");
    byte.extend_from_slice(&(36 + dati).to_le_bytes());
    byte.extend_from_slice(b"WAVEfmt ");
    byte.extend_from_slice(&16u32.to_le_bytes());
    byte.extend_from_slice(&1u16.to_le_bytes()); // PCM
    byte.extend_from_slice(&1u16.to_le_bytes()); // un canale
    byte.extend_from_slice(&frequenza.to_le_bytes());
    byte.extend_from_slice(&(frequenza * 2).to_le_bytes());
    byte.extend_from_slice(&2u16.to_le_bytes()); // allineamento
    byte.extend_from_slice(&16u16.to_le_bytes()); // bit per campione
    byte.extend_from_slice(b"data");
    byte.extend_from_slice(&dati.to_le_bytes());
    for campione in campioni {
        byte.extend_from_slice(&((campione * 32_767.0) as i16).to_le_bytes());
    }
    byte
}

/// Un brano di un secondo, tutto allo stesso livello: si riconosce a
/// orecchio nell'elenco dei campioni.
fn brano(track_id: i64, frequenza: u32, livello: f32) -> Sorgente {
    brano_da(track_id, frequenza, livello, 1_000)
}

/// Un brano che dura quel che dice di durare.
///
/// [`brano`] e [`brano_da`] dichiarano sempre un secondo, che è quel che
/// serve alla maggior parte delle prove. Qui la durata dichiarata segue il
/// contenuto, perché alle prove dell'anello lungo servono brani di lunghezze
/// **diverse**: è la differenza fra la durata di un brano e quanto l'anello
/// tiene da parte a decidere se il brano che si sente e quello che si
/// decodifica siano lo stesso.
fn brano_onesto(track_id: i64, frequenza: u32, livello: f32, ms: u64) -> Sorgente {
    Sorgente {
        durata_ms: ms,
        ..brano_da(track_id, frequenza, livello, ms)
    }
}

/// Come sopra, ma `veri_ms` dice quanto il file **contiene**: la durata
/// dichiarata resta un secondo. I due valori diversi sono il caso di un
/// file che mente sulla propria durata, che è quel che il database si
/// ritrova per un MP3 a bitrate variabile senza intestazione Xing.
fn brano_da(track_id: i64, frequenza: u32, livello: f32, veri_ms: u64) -> Sorgente {
    let quanti = fotogrammi_da_ms(veri_ms, frequenza);
    let campioni = vec![livello; usize::try_from(quanti).unwrap_or(48_000)];
    Sorgente {
        track_id,
        media: Box::new(Byte(std::io::Cursor::new(wav(frequenza, &campioni)))),
        estensione: Some("wav".to_owned()),
        durata_ms: 1_000,
        replaygain_db: None,
        picco: None,
        segmento: None,
    }
}

/// Apre una sorgente di prova per l'uscita di questo contesto.
///
/// Da quando il motore riceve brani già aperti (vedi [`BranoAperto`]), le
/// prove devono fare quel che fa l'applicazione: aprire di qua dal confine.
/// Il `expect` è a posto perché queste sorgenti sono WAV costruiti due
/// funzioni più su — se non si aprissero, il guasto sarebbe nel banco di
/// prova e non in quel che si sta provando.
fn aperto(ctx: &Contesto, sorgente: Sorgente) -> BranoAperto {
    BranoAperto::apri(sorgente, ctx.formato).expect("la sorgente di prova si apre")
}

/// Il comando che manda [`Motore::suona`]: un brano dal suo inizio.
fn suona(brano: BranoAperto) -> Comando {
    Comando::Suona {
        brano: Box::new(brano),
        da_ms: 0,
    }
}

/// Il filo della decodifica senza il filo e senza il dispositivo.
struct Banco {
    ctx: Contesto,
    consumatore: rtrb::Consumer<f32>,
    /// Le caselle che il contesto condivide con la callback che qui non
    /// c'è: le prove del filo caduto ci leggono `perso` e la causa, quelle
    /// del motore abbandonato ci scrivono la bandiera.
    condiviso: Arc<Condiviso>,
    /// Da tenere vivi: il canale dei comandi e l'anello delle curve non
    /// vengono usati, ma se cadessero il contesto parlerebbe con dei morti.
    _manda: Sender<Comando>,
    _curve: rtrb::Consumer<Coefficienti>,
    /// La metà della callback che risponde allo svuotamento: vedi
    /// [`Stretta`].
    stretta: Stretta,
}

/// La stretta di mano dello svuotamento, senza la callback.
///
/// `Contesto::scarta_in_volo` alza `svuota` e aspetta che la callback lo
/// abbassi, e il filo non spinge niente nell'anello finché la bandiera
/// resta alta: un banco in cui nessuno la abbassa non suonerebbe più niente
/// dopo il primo svuotamento. Fino a qui ogni prova passava proprio dalla
/// scadenza della stretta di mano, che nel motore vero è il caso del
/// dispositivo che non risponde.
///
/// Questo filo fa la metà della callback che serve: abbassa la bandiera
/// appena la vede alzata. L'anello no — lo svuota chi prova, come ha sempre
/// fatto, ed è quel che le prove di questo file si aspettano.
struct Stretta {
    basta: Arc<AtomicBool>,
    filo: Option<std::thread::JoinHandle<()>>,
}

impl Stretta {
    fn nuova(condiviso: &Arc<Condiviso>) -> Self {
        let basta = Arc::new(AtomicBool::new(false));
        let filo = {
            let basta = Arc::clone(&basta);
            let condiviso = Arc::clone(condiviso);
            std::thread::spawn(move || {
                while !basta.load(Ordering::Acquire) {
                    if condiviso.svuota.load(Ordering::Acquire) {
                        condiviso.svuota.store(false, Ordering::Release);
                    }
                    std::thread::sleep(Duration::from_micros(200));
                }
            })
        };
        Self {
            basta,
            filo: Some(filo),
        }
    }

    /// Smette: da qui la stretta di mano la fa chi prova.
    fn ferma(&mut self) {
        self.basta.store(true, Ordering::Release);
        if let Some(filo) = self.filo.take() {
            let _ = filo.join();
        }
    }
}

impl Drop for Stretta {
    fn drop(&mut self) {
        self.ferma();
    }
}

fn banco(frequenza: u32) -> Banco {
    banco_con(frequenza, Box::new(|_| {}))
}

/// Come [`banco`], ma con un osservatore che si sceglie.
fn banco_con(frequenza: u32, osservatore: Box<dyn Fn(Evento) + Send>) -> Banco {
    banco_con_anello(frequenza, 16_384, osservatore)
}

/// Come sopra, ma l'anello è lungo quanto si vuole.
///
/// # Perché una prova dovrebbe volerlo più lungo
///
/// Perché i 16 384 campioni di [`banco_con`] sono un terzo di secondo a
/// 48 kHz, mentre l'anello vero ne tiene più di tre — [`crate::uscita`] è
/// dimensionata sul caso peggiore, 192 kHz a otto canali, e su un'uscita
/// normale quegli stessi campioni sono secondi di musica. Con un terzo di
/// secondo il brano che si sente e quello che si decodifica restano quasi
/// sempre lo stesso; con l'anello vero non lo sono per gli ultimi secondi di
/// **ogni** brano, ed è lì che vivono i difetti che questo banco serve a
/// riprodurre.
fn banco_con_anello(
    frequenza: u32,
    campioni: usize,
    osservatore: Box<dyn Fn(Evento) + Send>,
) -> Banco {
    // Le prove di sempre guardano l'evento, e lo guardano **subito**: la
    // voce del banco chiama l'osservatore sullo stesso filo, senza il
    // canale e senza «rymembra-eventi». Le prove del filo degli eventi lo
    // montano da sé, con [`banco_con_voce`].
    banco_con_voce(
        frequenza,
        campioni,
        Box::new(move |annuncio: Annuncio| osservatore(annuncio.evento)),
    )
}

/// Come [`banco_con_anello`], ma la voce si sceglie per intero.
fn banco_con_voce(frequenza: u32, campioni: usize, voce: Voce) -> Banco {
    let (manda, ricevi) = std::sync::mpsc::channel();
    let (produttore, consumatore) = rtrb::RingBuffer::<f32>::new(campioni);
    let (curve, prese) = rtrb::RingBuffer::<Coefficienti>::new(32);
    let condiviso = Condiviso::nuovo();
    let ctx = Contesto::nuovo(
        ricevi,
        produttore,
        curve,
        Arc::clone(&condiviso),
        Arc::new(std::sync::Mutex::new(Posizione::default())),
        FormatoUscita {
            frequenza,
            canali: 1,
        },
        voce,
        crate::uscita::Campanello::spento(),
    );
    let stretta = Stretta::nuova(&condiviso);
    Banco {
        ctx,
        consumatore,
        condiviso,
        _manda: manda,
        _curve: prese,
        stretta,
    }
}

/// Quanti campioni stanno a un livello, entro un errore che copre la
/// quantizzazione a 16 bit e il ricampionatore.
fn quanti_a(campioni: &[f32], livello: f32) -> usize {
    campioni
        .iter()
        .filter(|c| (**c - livello).abs() < 0.01)
        .count()
}

/// Fa girare il contesto fino alla fine di tutto, raccogliendo quel che
/// sarebbe uscito dalle casse. `dopo_l_inizio` è quel che chi sta sopra fa
/// quando il motore comincia a sovrapporre due brani.
fn suona_tutto(banco: &mut Banco, mut dopo_l_inizio: impl FnMut(&mut Contesto)) -> Vec<f32> {
    let mut fuori = Vec::new();
    let mut avvisato = false;
    // Un tetto ai giri: una prova che non finisce è una prova che blocca
    // l'intera suite, e tre brani da un secondo non ne chiedono nemmeno un
    // decimo.
    for _ in 0..100_000 {
        let lavorato = banco.ctx.riempi();
        while let Ok(campione) = banco.consumatore.pop() {
            fuori.push(campione);
        }
        if !avvisato && banco.ctx.entrante.is_some() {
            dopo_l_inizio(&mut banco.ctx);
            avvisato = true;
        }
        if !lavorato && banco.ctx.corrente.is_none() {
            break;
        }
    }
    fuori
}

// ── il racconto, non i campioni ─────────────────────────────────────────
//
// Le prove qui sopra guardano la sequenza di campioni che *sarebbe* uscita.
// Quelle che seguono guardano l'altra metà del mestiere del motore: cosa
// il motore **dice** di star suonando, e a che punto. Per vederlo serve la
// callback, che qui non c'è — e allora la si fa: i campioni che escono
// dall'anello si contano in `Condiviso::fotogrammi`, esattamente come fa
// lei, e `aggiorna` traduce quel conteggio in una posizione.
//
// Senza quel conteggio la posizione resta a zero per sempre, ed è la ragione
// per cui nessuna prova di questo file guardava la posizione: il bug del
// segno di dissolvenza è vissuto per mesi dentro un banco che non poteva
// vederlo.

/// Quanti fotogrammi la callback finta consuma per giro.
///
/// Duecentocinquantasei: l'ordine di grandezza di quel che cpal chiede a
/// 48 kHz. Serve a rendere vera la tolleranza delle prove — «un blocco» è
/// cinque millisecondi e mezzo, non un margine scelto per far passare.
const BLOCCO_FINTO: u64 = 256;

/// Un giro di quel ciclo, visto da fuori.
struct Passo {
    /// Quel che il motore raccontava in quell'istante.
    posizione: Posizione,
    /// I fotogrammi contati dalla callback finta, grezzi.
    suonati: u64,
    /// Quanti eventi erano stati annunciati fino a lì.
    ///
    /// Un conteggio e non l'elenco: serve a sapere **a quale giro** un
    /// annuncio è arrivato, che è la domanda delle prove sulla latenza. Quale
    /// annuncio sia lo dice la sequenza, che per un brano solo è
    /// `iniziato`, `fermato`.
    eventi: usize,
}

/// Fa girare il contesto con la callback finta, e raccoglie il racconto.
///
/// I banchi di queste prove hanno un canale solo — vedi [`banco`] — quindi un
/// campione è un fotogramma e non serve nessuna conversione.
fn suona_raccontando(banco: &mut Banco, registro: &Registro) -> Vec<Passo> {
    let mut storia = Vec::new();
    // Lo stesso tetto di [`suona_tutto`], e per la stessa ragione: una prova
    // che non finisce blocca la suite intera.
    for _ in 0..200_000 {
        let lavorato = banco.ctx.riempi();
        let mut presi = 0u64;
        while presi < BLOCCO_FINTO {
            if banco.consumatore.pop().is_err() {
                break;
            }
            presi = presi.saturating_add(1);
        }
        banco
            .condiviso
            .fotogrammi
            .fetch_add(presi, Ordering::Relaxed);
        banco.ctx.aggiorna();
        let posizione = banco
            .ctx
            .posizione
            .lock()
            .map_or_else(|avvelenato| *avvelenato.into_inner(), |g| *g);
        storia.push(Passo {
            posizione,
            suonati: banco.condiviso.fotogrammi.load(Ordering::Relaxed),
            eventi: quanti_eventi(registro),
        });
        if !lavorato && presi == 0 && banco.ctx.corrente.is_none() {
            break;
        }
    }
    storia
}

/// Il primo istante in cui il motore dice di star suonando questo brano.
fn primo_annuncio(storia: &[Passo], track_id: i64) -> Option<&Passo> {
    storia
        .iter()
        .find(|p| p.posizione.track_id == Some(track_id))
}

#[test]
fn la_posizione_a_meta_dissolvenza_e_quella_del_brano_che_entra() {
    // Il bug che questa prova chiude, e che valeva tutto il brano: il segno
    // dell'entrante si annota a `spinti + meta`, cioè a un istante in cui
    // l'entrante ha già prodotto `meta` fotogrammi, e dichiarava
    // `offset_ms: 0`. Con quattrocento millisecondi di dissolvenza la
    // posizione riportata partiva da zero invece che da duecento e restava
    // indietro di duecento **fino alla fine del brano**. Con i sei secondi
    // che si possono scegliere dalle impostazioni sono tre secondi.
    let (registro, osservatore) = spia();
    let mut banco = banco_con(48_000, osservatore);
    banco.ctx.esegui(Comando::Dissolvenza { ms: 400 });
    let primo = aperto(&banco.ctx, brano(1, 48_000, 0.8));
    banco.ctx.esegui(suona(primo));
    let secondo = aperto(&banco.ctx, brano(2, 48_000, 0.4));
    banco.ctx.esegui(Comando::Prepara(Some(Box::new(secondo))));

    let storia = suona_raccontando(&mut banco, &registro);

    let arrivo = primo_annuncio(&storia, 2).expect("il secondo brano non è mai stato annunciato");
    // Metà di quattrocento millisecondi, più al più il blocco che la
    // callback finta consuma per giro.
    assert!(
        (190..=230).contains(&arrivo.posizione.ms),
        "il brano che entra si annuncia a {} ms invece di ~200: il segno \
         dice un istante e l'offset un altro",
        arrivo.posizione.ms
    );
}

#[test]
fn un_salto_verso_la_fine_non_porta_la_posizione_oltre_il_brano() {
    // Con la dissolvenza accesa, un salto a cento millisecondi dalla fine
    // fa cominciare una sovrapposizione che dura cento millisecondi e non
    // quattrocento. Il segno dell'entrante stava comunque a metà dei
    // quattrocento: per un decimo di secondo — con i dodici secondi che si
    // possono scegliere, cinque secondi — si sentiva già il brano dopo, e
    // il motore raccontava il primo a 1050, 1100 ms di un brano da mille.
    let (registro, osservatore) = spia();
    let mut banco = banco_con(48_000, osservatore);
    banco.ctx.esegui(Comando::Dissolvenza { ms: 400 });
    let primo = aperto(&banco.ctx, brano(1, 48_000, 0.8));
    banco.ctx.esegui(suona(primo));
    let secondo = aperto(&banco.ctx, brano(2, 48_000, 0.4));
    banco.ctx.esegui(Comando::Prepara(Some(Box::new(secondo))));
    banco.ctx.esegui(Comando::VaiA(900));

    let storia = suona_raccontando(&mut banco, &registro);

    let oltre = storia
        .iter()
        .filter(|p| p.posizione.track_id == Some(1))
        .map(|p| p.posizione.ms)
        .max()
        .unwrap_or(0);
    assert!(
        oltre <= 1_010,
        "il primo brano racconta {oltre} ms su mille: il secondo si sente già"
    );
    let arrivo = primo_annuncio(&storia, 2).expect("il secondo brano non è mai stato annunciato");
    // Metà della sovrapposizione vera — cento millisecondi — più al più un
    // blocco della callback finta e uno del decodificatore.
    assert!(
        arrivo.posizione.ms <= 90,
        "il brano che entra si annuncia a {} ms: metà di una dissolvenza \
         che non c'è stata",
        arrivo.posizione.ms
    );
}

#[test]
fn il_gapless_resta_a_zero_sul_primo_campione() {
    // Il contrappeso della prova qui sopra, e la ragione per cui i due
    // `offset_ms` di `forse_dissolvi` e `passa_al_prossimo` devono restare
    // diversi: senza dissolvenza il brano nuovo attacca dal suo primo
    // campione, e lì zero è la verità. Chi «uniformasse» le due per simmetria
    // lo scoprirebbe qui.
    let (registro, osservatore) = spia();
    let mut banco = banco_con(48_000, osservatore);
    let primo = aperto(&banco.ctx, brano(1, 48_000, 0.8));
    banco.ctx.esegui(suona(primo));
    let secondo = aperto(&banco.ctx, brano(2, 48_000, 0.4));
    banco.ctx.esegui(Comando::Prepara(Some(Box::new(secondo))));

    let storia = suona_raccontando(&mut banco, &registro);

    let arrivo = primo_annuncio(&storia, 2).expect("il secondo brano non è mai stato annunciato");
    assert!(
        arrivo.posizione.ms <= 20,
        "il gapless attacca a {} ms invece che da capo",
        arrivo.posizione.ms
    );
}

/// Due brani da un secondo a 44,1 kHz, allo stesso livello, uno dopo
/// l'altro su un'uscita a 48 kHz: il caso del collaudo dal vivo.
fn due_brani_a_44(osservatore: Box<dyn Fn(Evento) + Send>) -> Banco {
    let mut banco = banco_con(48_000, osservatore);
    let primo = aperto(&banco.ctx, brano_onesto(1, 44_100, 0.5, 1_000));
    banco.ctx.esegui(suona(primo));
    let secondo = aperto(&banco.ctx, brano_onesto(2, 44_100, 0.5, 1_000));
    banco.ctx.esegui(Comando::Prepara(Some(Box::new(secondo))));
    banco
}

#[test]
fn il_gapless_ricampionato_non_ha_buchi_alla_giuntura() {
    // Lo schiocco del collaudo dal vivo. Con un ricampionatore per brano la
    // coda del primo si chiudeva con degli zeri e la testa del secondo
    // partiva da zeri: in mezzo il livello scendeva per una manciata di
    // campioni, e un gradino così si sente. Con un ricampionatore solo che
    // attraversa la giuntura il livello non si muove.
    let mut banco = due_brani_a_44(Box::new(|_| {}));
    let fuori = suona_tutto(&mut banco, |_| {});

    // Due secondi a 44,1 kHz sono esattamente due secondi a 48.
    assert_eq!(
        fuori.len(),
        96_000,
        "la coda del primo si è persa o doppiata"
    );
    let giuntura = fuori
        .get(46_000..50_000)
        .expect("l'uscita arriva a due secondi");
    let fuori_posto = giuntura
        .iter()
        .filter(|c| (**c - 0.5).abs() >= 0.01)
        .count();
    assert_eq!(
        fuori_posto, 0,
        "{fuori_posto} campioni fuori livello alla giuntura"
    );
}

#[test]
fn il_gapless_ricampionato_annuncia_il_secondo_dove_comincia() {
    // Il contrappeso della prova qui sopra: i primi fotogrammi che il
    // secondo brano consegna sono la coda del primo, rimasta nel
    // ricampionatore che hanno in comune. Il segno del secondo va messo
    // dopo di loro, al secondo esatto, e non dove il primo ha smesso di
    // consegnare — altrimenti la posizione del secondo parte in anticipo e
    // la coda del primo viene contata come sua.
    let (registro, osservatore) = spia();
    let mut banco = due_brani_a_44(osservatore);
    let storia = suona_raccontando(&mut banco, &registro);

    let arrivo = primo_annuncio(&storia, 2).expect("il secondo brano non è mai stato annunciato");
    assert!(
        arrivo.suonati >= 48_000,
        "il secondo brano annunciato a {} fotogrammi, prima della fine del primo",
        arrivo.suonati
    );
    assert!(
        arrivo.posizione.ms <= 20,
        "il gapless attacca a {} ms invece che da capo",
        arrivo.posizione.ms
    );
}

/// Suona un brano con sé stesso preparato come successivo, e dice cosa è
/// stato annunciato. È quel che fa «ripeti uno», e una coda con lo stesso
/// brano due volte di fila.
fn stesso_brano_due_volte(dissolvenza_ms: u64) -> Vec<String> {
    let (registro, osservatore) = spia();
    let mut banco = banco_con(48_000, osservatore);
    banco
        .ctx
        .esegui(Comando::Dissolvenza { ms: dissolvenza_ms });
    let primo = aperto(&banco.ctx, brano(7, 48_000, 0.8));
    banco.ctx.esegui(suona(primo));
    let di_nuovo = aperto(&banco.ctx, brano(7, 48_000, 0.8));
    banco.ctx.esegui(Comando::Prepara(Some(Box::new(di_nuovo))));
    let _ = suona_raccontando(&mut banco, &registro);
    eventi(&registro)
}

#[test]
fn lo_stesso_brano_che_ricomincia_si_annuncia_di_nuovo() {
    // Il difetto che chiude: l'annuncio confrontava l'identificativo, il
    // secondo giro dello stesso brano passava muto, chi sta sopra non
    // preparava il terzo e «ripeti uno» si fermava dopo una ripetizione.
    for dissolvenza_ms in [0, 400] {
        let annunci = stesso_brano_due_volte(dissolvenza_ms);
        assert_eq!(
            annunci,
            ["iniziato:7", "iniziato:7", "fermato"],
            "con {dissolvenza_ms} ms di dissolvenza"
        );
    }
}

#[test]
fn un_salto_non_annuncia_di_nuovo_il_brano() {
    // Il contrappeso: un salto dentro lo stesso ascolto non è un inizio, e
    // annunciarlo farebbe ripartire da capo il conteggio dell'ascolto. Vale
    // anche per il salto che arriva prima del primo annuncio: il brano deve
    // essere annunciato comunque, una volta.
    for prima_di_annunciare in [false, true] {
        let (registro, osservatore) = spia();
        let mut banco = banco_con(48_000, osservatore);
        let primo = aperto(&banco.ctx, brano(3, 48_000, 0.8));
        banco.ctx.esegui(suona(primo));
        if !prima_di_annunciare {
            banco.ctx.aggiorna();
        }
        banco.ctx.esegui(Comando::VaiA(500));
        let _ = suona_raccontando(&mut banco, &registro);
        assert_eq!(
            eventi(&registro),
            ["iniziato:3", "fermato"],
            "salto {} del primo annuncio",
            if prima_di_annunciare { "prima" } else { "dopo" }
        );
    }
}

#[test]
fn un_salto_negli_ultimi_secondi_non_finisce_nel_brano_dopo() {
    // L'anello tiene da parte molto più di quel che la riserva dichiara —
    // su un'uscita a 48 kHz stereo più di tre secondi, vedi
    // `uscita::RISERVA_MS` — quindi negli ultimi secondi di ogni brano il
    // decodificatore sta già macinando quello dopo, e `corrente` non è più
    // il brano che esce dalle casse.
    //
    // Finché `vai_a` guardava `corrente`, in quella finestra il salto
    // cadeva sul brano **successivo**: si sentiva quello dal punto chiesto,
    // l'interfaccia mostrava ancora il titolo di prima col cursore che
    // correva oltre la fine della barra, e quando quel brano finiva in
    // canna non era rimasto niente — nessun `Iniziato`, nessun `Fermato`,
    // la coda ferma finché qualcuno non premeva un tasto.
    let (registro, osservatore) = spia();
    let mut banco = banco_con(48_000, osservatore);
    let primo = aperto(&banco.ctx, brano(1, 48_000, 0.8));
    banco.ctx.esegui(suona(primo));
    let secondo = aperto(&banco.ctx, brano(2, 48_000, 0.4));
    banco.ctx.esegui(Comando::Prepara(Some(Box::new(secondo))));

    // Si gira finché la decodifica è passata al secondo mentre dalle casse
    // esce ancora il primo: è esattamente la finestra del difetto.
    let mut finestra = false;
    for _ in 0..100_000 {
        banco.ctx.riempi();
        let mut presi = 0u64;
        while presi < BLOCCO_FINTO {
            if banco.consumatore.pop().is_err() {
                break;
            }
            presi = presi.saturating_add(1);
        }
        banco
            .condiviso
            .fotogrammi
            .fetch_add(presi, Ordering::Relaxed);
        banco.ctx.aggiorna();
        let si_decodifica = banco.ctx.corrente.as_ref().map(|d| d.track_id());
        let si_sente = banco.ctx.segni.front().map(|s| s.track_id);
        if si_decodifica == Some(2) && si_sente == Some(1) {
            finestra = true;
            break;
        }
    }
    assert!(
        finestra,
        "il banco non arriva mai a decodificare il secondo mentre si sente il primo"
    );

    banco.ctx.esegui(Comando::VaiA(200));
    let storia = suona_raccontando(&mut banco, &registro);

    assert_eq!(
        eventi(&registro),
        ["iniziato:1", "iniziato:2", "fermato"],
        "la coda non è andata avanti da sola dopo il salto"
    );
    let oltre = storia
        .iter()
        .filter(|p| p.posizione.track_id == Some(1))
        .map(|p| p.posizione.ms)
        .max()
        .unwrap_or(0);
    assert!(
        oltre <= 1_010,
        "il primo brano racconta {oltre} ms su mille: il salto è finito nel brano dopo"
    );
    let tornato = storia
        .iter()
        .any(|p| p.posizione.track_id == Some(1) && (150..=350).contains(&p.posizione.ms));
    assert!(
        tornato,
        "dopo il salto il primo brano non riparte da dove era stato chiesto"
    );
}

#[test]
fn con_l_anello_lungo_il_salto_cade_sul_brano_che_si_sente() {
    // Il caso che l'anello corto di [`banco_con`] non sa mostrare: quando
    // l'anello tiene da parte più di un brano, la decodifica finisce il
    // **secondo** prima che il primo smetta di sentirsi. Da quel momento
    // `uscente` porta il secondo mentre in testa c'è ancora il primo, e chi
    // lo lasciasse cadere al primo `pop_front` — cioè al cambio di brano —
    // butterebbe via proprio il decodificatore che il salto dentro il
    // secondo brano sta per chiedere.
    //
    // Le durate sono scelte per cadere in quella finestra: un anello da un
    // secondo, un primo brano più lungo (così la sua decodifica finisce
    // mentre lo si sente ancora) e un secondo più corto (così anche la sua
    // finisce prima che il primo sia stato udito per intero).
    const ANELLO_MS: u64 = 1_000;
    let (registro, osservatore) = spia();
    let mut banco = banco_con_anello(
        48_000,
        usize::try_from(fotogrammi_da_ms(ANELLO_MS, 48_000)).unwrap_or(48_000),
        osservatore,
    );
    let primo = aperto(&banco.ctx, brano_onesto(1, 48_000, 0.8, 2_000));
    banco.ctx.esegui(suona(primo));
    // La coda che resta da consegnare, come farebbe chi sta sopra: uno alla
    // volta, appena la canna si libera.
    let mut resto = vec![
        brano_onesto(3, 48_000, 0.2, 800),
        brano_onesto(2, 48_000, 0.4, 800),
    ];

    let mut saltato = false;
    let mut storia = Vec::new();
    for _ in 0..200_000 {
        if banco.ctx.prossimo.is_none()
            && banco.ctx.corrente.is_some()
            && let Some(sorgente) = resto.pop()
        {
            let aperto = aperto(&banco.ctx, sorgente);
            banco.ctx.esegui(Comando::Prepara(Some(Box::new(aperto))));
        }
        let lavorato = banco.ctx.riempi();
        let mut presi = 0u64;
        while presi < BLOCCO_FINTO {
            if banco.consumatore.pop().is_err() {
                break;
            }
            presi = presi.saturating_add(1);
        }
        banco
            .condiviso
            .fotogrammi
            .fetch_add(presi, Ordering::Relaxed);
        banco.ctx.aggiorna();
        let posizione = banco
            .ctx
            .posizione
            .lock()
            .map_or_else(|avvelenato| *avvelenato.into_inner(), |g| *g);
        // Il salto arriva quando si sente il secondo brano ed è già passata
        // alla decodifica del terzo: la finestra descritta qui sopra.
        if !saltato
            && posizione.track_id == Some(2)
            && posizione.ms > 400
            && banco
                .ctx
                .corrente
                .as_ref()
                .is_some_and(|d| d.track_id() == 3)
        {
            banco.ctx.esegui(Comando::VaiA(100));
            saltato = true;
        }
        storia.push(posizione);
        if !lavorato && presi == 0 && banco.ctx.corrente.is_none() {
            break;
        }
    }

    assert!(saltato, "la finestra del difetto non si è mai aperta");
    assert_eq!(
        eventi(&registro),
        ["iniziato:1", "iniziato:2", "iniziato:3", "fermato"],
        "la coda non è andata avanti da sola dopo il salto"
    );
    // Che sia **tornato** indietro, non che sia passato di lì salendo: il
    // secondo brano attraversa i cento millisecondi anche senza nessun
    // salto, e un'asserzione che non guarda l'ordine passerebbe anche col
    // salto lasciato cadere.
    let dopo_il_salto = storia
        .iter()
        .position(|p| p.track_id == Some(2) && p.ms > 400)
        .map_or(storia.len(), |i| i.saturating_add(1));
    let tornato = storia
        .get(dopo_il_salto..)
        .unwrap_or_default()
        .iter()
        .any(|p| p.track_id == Some(2) && (50..=300).contains(&p.ms));
    assert!(
        tornato,
        "il secondo brano non è mai tornato a ~100 ms: il salto è stato \
         lasciato cadere, o è finito sul brano dopo"
    );
    let oltre = storia
        .iter()
        .filter(|p| p.track_id == Some(2))
        .map(|p| p.ms)
        .max()
        .unwrap_or(0);
    assert!(
        oltre <= 810,
        "il secondo brano racconta {oltre} ms su ottocento: il salto è \
         finito nel brano dopo"
    );
}

#[test]
fn un_preparato_in_ritardo_non_si_porta_via_il_brano_che_si_sente() {
    // L'incrocio fra le due cose che l'anello lungo rende normali.
    //
    // Un brano più corto di quel che l'anello tiene da parte finisce di
    // decodificarsi prima che chi sta sopra abbia consegnato il successivo:
    // `passa_al_prossimo` scatta a mani vuote, lascia `corrente` a niente, e
    // mette da parte il brano che si sta ancora sentendo. Quando il
    // preparato arriva, [`Contesto::prepara`] richiama `passa_al_prossimo`
    // per attaccarlo — ed è lì che il brano messo da parte rischia di
    // sparire, cancellato da un `corrente` che ormai è vuoto.
    //
    // Se sparisce, il salto che arriva dopo non trova più il brano che si
    // sente e si lascia cadere: il cursore torna indietro da solo e chi
    // trascina non capisce perché.
    const ANELLO_MS: u64 = 1_000;
    let (registro, osservatore) = spia();
    let mut banco = banco_con_anello(
        48_000,
        usize::try_from(fotogrammi_da_ms(ANELLO_MS, 48_000)).unwrap_or(48_000),
        osservatore,
    );
    let primo = aperto(&banco.ctx, brano_onesto(1, 48_000, 0.8, 2_000));
    banco.ctx.esegui(suona(primo));

    // Nessun preparato: si gira finché la decodifica finisce il primo brano
    // e non trova niente da attaccare.
    let mut storia = Vec::new();
    let giro = |banco: &mut Banco, storia: &mut Vec<Posizione>| {
        banco.ctx.riempi();
        let mut presi = 0u64;
        while presi < BLOCCO_FINTO {
            if banco.consumatore.pop().is_err() {
                break;
            }
            presi = presi.saturating_add(1);
        }
        banco
            .condiviso
            .fotogrammi
            .fetch_add(presi, Ordering::Relaxed);
        banco.ctx.aggiorna();
        let posizione = banco
            .ctx
            .posizione
            .lock()
            .map_or_else(|avvelenato| *avvelenato.into_inner(), |g| *g);
        storia.push(posizione);
    };
    let mut a_mani_vuote = false;
    for _ in 0..100_000 {
        giro(&mut banco, &mut storia);
        if banco.ctx.corrente.is_none() && banco.ctx.prossimo.is_none() {
            a_mani_vuote = true;
            break;
        }
    }
    assert!(
        a_mani_vuote,
        "la decodifica non è mai arrivata in fondo al primo brano senza un \
         successivo da attaccare"
    );

    // Il preparato in ritardo, e subito dopo il salto.
    let secondo = aperto(&banco.ctx, brano_onesto(2, 48_000, 0.4, 800));
    banco.ctx.esegui(Comando::Prepara(Some(Box::new(secondo))));
    banco.ctx.esegui(Comando::VaiA(300));

    for _ in 0..200_000 {
        giro(&mut banco, &mut storia);
        if banco.ctx.corrente.is_none() && banco.ctx.prossimo.is_none() {
            break;
        }
    }

    assert_eq!(
        eventi(&registro),
        ["iniziato:1", "iniziato:2", "fermato"],
        "la coda non è andata avanti da sola dopo il salto"
    );
    let dopo_il_salto = storia
        .iter()
        .position(|p| p.track_id == Some(1) && p.ms > 600)
        .map_or(storia.len(), |i| i.saturating_add(1));
    let tornato = storia
        .get(dopo_il_salto..)
        .unwrap_or_default()
        .iter()
        .any(|p| p.track_id == Some(1) && (250..=500).contains(&p.ms));
    assert!(
        tornato,
        "il primo brano non è mai tornato a ~300 ms: il preparato in \
         ritardo si è portato via il brano che si sentiva"
    );
}

/// Un brano solo suonato fino in fondo, con le due latenze che si vogliono.
///
/// `misurata` è quel che la callback avrebbe scritto in
/// `Condiviso::latenza_fotogrammi`; `manuale_ms` è la preferenza
/// `audio.latenza_ms`. Il brano e il numero di giri sono gli stessi in ogni
/// chiamata — la decodifica non guarda la latenza — quindi due storie si
/// possono confrontare **giro per giro**.
fn un_brano_con_latenza(manuale_ms: i64, misurata: u64) -> Vec<Passo> {
    let (registro, osservatore) = spia();
    let mut banco = banco_con(48_000, osservatore);
    banco
        .condiviso
        .latenza_fotogrammi
        .store(misurata, Ordering::Relaxed);
    banco.ctx.esegui(Comando::Latenza { ms: manuale_ms });
    let solo = aperto(&banco.ctx, brano(1, 48_000, 0.5));
    banco.ctx.esegui(suona(solo));
    suona_raccontando(&mut banco, &registro)
}

/// Il giro di mezzo, dove il brano sta suonando e nessun estremo disturba.
#[expect(
    clippy::integer_division,
    reason = "metà di un elenco di giri: mezzo giro non esiste, e quale dei \
              due vicini si prenda non cambia niente"
)]
fn a_meta(storia: &[Passo]) -> &Passo {
    storia
        .get(storia.len() / 2)
        .expect("una storia vuota vuol dire che il banco non ha suonato")
}

#[test]
fn la_latenza_dichiarata_sposta_indietro_la_posizione() {
    let senza = un_brano_con_latenza(0, 0);
    let con = un_brano_con_latenza(100, 0);
    assert_eq!(
        senza.len(),
        con.len(),
        "la latenza ha cambiato la decodifica: non deve toccare i campioni"
    );

    let qui = a_meta(&senza);
    let la = a_meta(&con);
    assert_eq!(
        qui.suonati, la.suonati,
        "i due giri non sono lo stesso giro"
    );
    let scarto =
        i64::try_from(qui.posizione.ms).unwrap_or(0) - i64::try_from(la.posizione.ms).unwrap_or(0);
    assert!(
        (90..=110).contains(&scarto),
        "cento millisecondi di latenza spostano la posizione di {scarto}"
    );
}

#[test]
fn la_latenza_non_ritarda_la_fine() {
    // La fine si dichiara quando l'ultimo campione ha lasciato **l'anello
    // nostro**, non le casse: ritardarla della latenza vorrebbe dire
    // ritardare di altrettanto il brano dopo, cioè un buco fra due tracce di
    // un album gapless lungo esattamente la compensazione.
    //
    // Per un brano solo gli annunci sono due, in quest'ordine: `iniziato`,
    // `fermato`. Il giro in cui il conteggio arriva a due è il giro del
    // `Fermato`, e i fotogrammi contati lì devono essere gli stessi con e
    // senza latenza.
    let senza = un_brano_con_latenza(0, 0);
    let con = un_brano_con_latenza(500, 0);

    let fine = |storia: &[Passo]| {
        storia
            .iter()
            .find(|p| p.eventi >= 2)
            .map(|p| p.suonati)
            .expect("il brano non si è mai dichiarato finito")
    };
    assert_eq!(
        fine(&senza),
        fine(&con),
        "mezzo secondo di latenza ha spostato la fine del brano: il gapless \
         guadagnerebbe un buco lungo altrettanto"
    );
}

#[test]
fn una_latenza_assurda_si_ignora() {
    // Quel che `cpal` riporta è la durata del buffer del dispositivo: decine
    // di millisecondi. Un secondo lì dentro non è un'uscita lenta, è un
    // orologio che ha risposto una sciocchezza — e compensarlo sposterebbe
    // cursore e testi di un secondo senza che nessuno possa capire perché.
    let senza = un_brano_con_latenza(0, 0);
    let assurda = un_brano_con_latenza(0, fotogrammi_da_ms(1_000, 48_000));
    assert_eq!(
        a_meta(&senza).posizione.ms,
        a_meta(&assurda).posizione.ms,
        "una latenza oltre il tetto è stata compensata lo stesso"
    );

    // E l'altra metà della stessa decisione: sotto il tetto la misura si usa,
    // altrimenti questa prova passerebbe anche con una misura ignorata sempre.
    let buona = un_brano_con_latenza(0, fotogrammi_da_ms(100, 48_000));
    let scarto = i64::try_from(a_meta(&senza).posizione.ms).unwrap_or(0)
        - i64::try_from(a_meta(&buona).posizione.ms).unwrap_or(0);
    assert!(
        (90..=110).contains(&scarto),
        "cento millisecondi misurati spostano la posizione di {scarto}"
    );
}

#[test]
fn la_dissolvenza_manda_avanti_il_brano_dopo_non_quello_dopo_ancora() {
    // Il guasto che questa prova tiene chiuso: a metà sovrapposizione il
    // motore annuncia il brano che entra, chi sta sopra risponde preparando
    // il brano **ancora dopo**, e finché «preparato» ed «entrante» erano la
    // stessa casella quella preparazione scippava il brano a metà curva.
    // Chi ascoltava sentiva gracchiare e si ritrovava un brano più avanti.
    let mut banco = banco(48_000);
    banco.ctx.esegui(Comando::Dissolvenza { ms: 400 });
    let primo = aperto(&banco.ctx, brano(1, 48_000, 0.8));
    banco.ctx.esegui(suona(primo));
    let secondo = aperto(&banco.ctx, brano(2, 24_000, 0.4));
    banco.ctx.esegui(Comando::Prepara(Some(Box::new(secondo))));

    let fuori = suona_tutto(&mut banco, |ctx| {
        let terzo = aperto(ctx, brano(3, 48_000, 0.1));
        ctx.esegui(Comando::Prepara(Some(Box::new(terzo))));
    });

    // Il secondo brano deve sentirsi **da solo**, cioè al suo livello: fra
    // la fine della prima dissolvenza e l'inizio della seconda c'è un
    // quinto di secondo in cui non c'è nient'altro. Se al suo posto fosse
    // entrato il terzo, di campioni a 0,4 non ce ne sarebbe nessuno.
    let secondo = quanti_a(&fuori, 0.4);
    assert!(
        secondo > 5_000,
        "il brano che entra non si sente mai da solo: {secondo} campioni a 0,4"
    );
    // E il terzo deve sentirsi dopo, non al posto suo.
    assert!(quanti_a(&fuori, 0.1) > 5_000, "il terzo brano non arriva");
}

#[test]
fn il_brano_che_entra_non_perde_campioni_per_strada() {
    // L'altro guasto, quello che si sentiva come un raspare: i due
    // decodificatori consegnano blocchi di lunghezza diversa — qui 1152
    // fotogrammi contro 2048, perché il secondo brano va ricampionato — e
    // mescolarne uno contro l'altro buttava via tutto quel che avanzava.
    // Il brano entrante ci arrivava in fondo quasi al doppio della
    // velocità, e la coda finiva prima del dovuto: la durata totale è il
    // modo più semplice di accorgersene.
    let mut banco = banco(48_000);
    banco.ctx.esegui(Comando::Dissolvenza { ms: 400 });
    let primo = aperto(&banco.ctx, brano(1, 48_000, 0.8));
    banco.ctx.esegui(suona(primo));
    let secondo = aperto(&banco.ctx, brano(2, 24_000, 0.4));
    banco.ctx.esegui(Comando::Prepara(Some(Box::new(secondo))));

    let fuori = suona_tutto(&mut banco, |_| {});

    // Due brani da un secondo che si sovrappongono per quattro decimi: un
    // secondo e sei decimi, non uno e due.
    let atteso = 48_000 + 48_000 - 19_200;
    let scarto = i64::try_from(fuori.len()).unwrap_or(0) - atteso;
    assert!(
        scarto.abs() < 2_400,
        "durata sbagliata di {scarto} fotogrammi: {} invece di {atteso}",
        fuori.len()
    );
}

/// Un brano di un secondo con un guadagno ReplayGain dal tag e nessun picco
/// misurato: il caso in cui [`correzione`] alza senza tetto.
fn brano_con_tag(track_id: i64, livello: f32, replaygain_db: Option<f32>) -> Sorgente {
    Sorgente {
        replaygain_db,
        picco: None,
        ..brano(track_id, 48_000, livello)
    }
}

#[test]
fn un_brano_alzato_chiede_la_piega_per_tutti_i_suoi_fotogrammi() {
    // +6 dB su un brano a 0,9: i campioni arrivano a 1,8. Il limitatore
    // sta nella callback, che li vede **dopo** il volume; a dirle che sono
    // alzati è `piega_fino`, che deve coprire il brano fino all'ultimo
    // fotogramma spinto.
    let mut banco = banco(48_000);
    let primo = aperto(&banco.ctx, brano_con_tag(1, 0.9, Some(6.0)));
    banco.ctx.esegui(suona(primo));
    let fuori = suona_tutto(&mut banco, |_| {});
    assert!(
        fuori.iter().any(|c| *c > 1.0),
        "il brano non è stato alzato"
    );
    assert_eq!(
        banco.condiviso.piega_fino.load(Ordering::Relaxed),
        banco.ctx.spinti,
        "la piega non copre tutti i fotogrammi alzati"
    );
}

#[test]
fn un_brano_non_alzato_non_chiede_la_piega() {
    // Senza tag, o con un tag che abbassa, niente sopra l'unità: un disco
    // masterizzato a 0 dBFS deve uscire com'è.
    for tag in [None, Some(-3.0)] {
        let mut banco = banco(48_000);
        let primo = aperto(&banco.ctx, brano_con_tag(1, 0.9, tag));
        banco.ctx.esegui(suona(primo));
        let _ = suona_tutto(&mut banco, |_| {});
        assert!(banco.ctx.spinti > 0);
        assert_eq!(
            banco.condiviso.piega_fino.load(Ordering::Relaxed),
            0,
            "con il tag {tag:?} la callback piegherebbe un brano che nessuno ha alzato"
        );
    }
}

#[test]
fn la_somma_della_dissolvenza_chiede_la_piega() {
    // Normalizzazione spenta, due brani forti: a metà curva la potenza
    // costante somma 0,707 + 0,707 sui picchi che coincidono. La piega
    // deve coprire la sovrapposizione, e fermarsi lì.
    let mut banco = banco(48_000);
    banco.ctx.esegui(Comando::ReplayGain {
        attivo: false,
        bersaglio_db: -18.0,
    });
    banco.ctx.esegui(Comando::Dissolvenza { ms: 400 });
    let primo = aperto(&banco.ctx, brano(1, 48_000, 0.95));
    banco.ctx.esegui(suona(primo));
    let secondo = aperto(&banco.ctx, brano(2, 48_000, 0.95));
    banco.ctx.esegui(Comando::Prepara(Some(Box::new(secondo))));
    let fuori = suona_tutto(&mut banco, |_| {});
    assert!(
        fuori.iter().any(|c| *c > 1.0),
        "la somma non supera l'unità"
    );
    let piega = banco.condiviso.piega_fino.load(Ordering::Relaxed);
    // La sovrapposizione finisce dove finisce il primo brano, al
    // fotogramma 48 000: lì la piega deve arrivare, e non oltre il resto
    // del secondo, che esce da solo e non alzato.
    assert!(
        piega >= 47_000 && piega < banco.ctx.spinti,
        "piega fino a {piega}, spinti {}",
        banco.ctx.spinti
    );
}

#[test]
fn un_brano_che_finisce_prima_della_curva_non_fa_saltare_quello_dopo() {
    // La sovrapposizione comincia quando alla fine manca quanto dura la
    // dissolvenza, e «quanto manca» si sa dalla durata dichiarata. Qui il
    // primo brano ne dichiara mille millisecondi e ne contiene ottocento:
    // il suo decodificatore finisce a metà curva, e quello entrante resta
    // solo a mezza ampiezza. Senza la ripresa salterebbe di colpo a tutta.
    let mut banco = banco(48_000);
    banco.ctx.esegui(Comando::Dissolvenza { ms: 400 });
    let primo = aperto(&banco.ctx, brano_da(1, 48_000, 0.8, 800));
    banco.ctx.esegui(suona(primo));
    let secondo = aperto(&banco.ctx, brano(2, 48_000, 0.4));
    banco.ctx.esegui(Comando::Prepara(Some(Box::new(secondo))));

    let fuori = suona_tutto(&mut banco, |_| {});

    // Finché i due si sovrappongono la somma sta sopra 0,8; appena il primo
    // finisce resta solo il secondo, che a piena ampiezza fa 0,4.
    let taglio = fuori
        .iter()
        .position(|c| *c < 0.5)
        .expect("il primo brano non finisce mai");
    let ripresa = fuori.get(taglio).copied().unwrap_or(0.0);
    assert!(
        ripresa < 0.36,
        "il brano entrante salta a piena ampiezza: {ripresa}"
    );
    // E a piena ampiezza ci arriva, in una quarantina di millisecondi.
    let dopo = fuori.get(taglio + 2_400).copied().unwrap_or(0.0);
    assert!(
        (dopo - 0.4).abs() < 0.01,
        "la ripresa non arriva a piena ampiezza: {dopo}"
    );
    // Senza gradini per strada, che è tutto il punto.
    let salto = fuori.get(taglio..taglio + 2_400).map_or(1.0, |pezzo| {
        pezzo
            .windows(2)
            .map(|due| (due[1] - due[0]).abs())
            .fold(0.0_f32, f32::max)
    });
    assert!(salto < 0.005, "gradino di {salto} dentro la ripresa");
}

#[test]
fn un_brano_piu_corto_dell_anello_non_pianta_la_riproduzione() {
    // Il bug che questa prova chiude, ed era una riproduzione che moriva:
    // un intermezzo di due secondi viene decodificato **per intero** nei
    // primi millisecondi, perché davanti alla callback c'è posto per più di
    // tre secondi di musica. Quando `passa_al_prossimo` scatta, il
    // preparatore — che parte dall'annuncio di `Iniziato`, aspetta
    // l'antirimbalzo della raffica e poi apre un file — non ha ancora
    // consegnato niente: il corrente si azzera, e il successivo arriva un
    // istante dopo, quando nessuno lo guarda più.
    //
    // Da lì non si usciva: `decodifica_un_blocco` esce subito senza un
    // corrente, quindi `passa_al_prossimo` non veniva più chiamato;
    // `forse_fine` non dichiara la fine con qualcosa in canna, quindi non
    // arrivava nemmeno un `Fermato`. Nessun evento, coda ferma, cursore
    // piantato sull'ultimo millisecondo.
    let (registro, osservatore) = spia();
    let mut banco = banco_con(48_000, osservatore);
    // Duecento millisecondi a 48 kHz su un canale sono novemilaseicento
    // campioni, e l'anello del banco ne tiene sedicimilatrecentottantaquattro:
    // il brano ci sta tutto, che è la condizione da riprodurre.
    let primo = aperto(&banco.ctx, brano_da(1, 48_000, 0.8, 200));
    banco.ctx.esegui(suona(primo));
    // Nessuno svuota l'anello: la decodifica corre fino in fondo al file
    // mentre dalle casse non è ancora uscito un campione.
    for _ in 0..100 {
        banco.ctx.riempi();
        banco.ctx.aggiorna();
    }
    assert!(
        banco.ctx.corrente.is_none(),
        "il banco non riproduce la corsa: il decodificatore non è arrivato in fondo"
    );

    // E adesso il preparatore, in ritardo di un soffio.
    let secondo = aperto(&banco.ctx, brano(2, 48_000, 0.4));
    banco.ctx.esegui(Comando::Prepara(Some(Box::new(secondo))));
    assert!(
        banco.ctx.corrente.is_some(),
        "il successivo resta in canna e il motore non suona più niente"
    );

    // Da qui il racconto torna quello di sempre: i due brani in ordine, e la
    // fine dichiarata una volta sola quando finiscono davvero.
    let storia = suona_raccontando(&mut banco, &registro);
    assert_eq!(eventi(&registro), ["iniziato:1", "iniziato:2", "fermato"]);
    // Il secondo si sente, e si sente al livello suo: attaccarlo senza buco
    // è metà del punto, e l'altra metà è che ci arrivi tutto.
    let suo = storia
        .iter()
        .filter(|p| p.posizione.track_id == Some(2))
        .count();
    assert!(suo > 0, "il secondo brano non si annuncia mai");
}

/// «Ripeti uno» su un brano senza campioni non gira in tondo.
///
/// Il preparatore dell'applicazione risponde a ogni `Iniziato` con lo
/// stesso brano — con «ripeti uno» il successivo è il corrente — qualche
/// giro dopo. Un preparato che arrivava a fine dichiarata e rianimava il
/// motore, per non fermare la coda su un brano vuoto, qui la faceva girare
/// per sempre: cinquanta inizi e fini al secondo, e la pausa non la fermava,
/// perché ogni ripresa toglieva la pausa da sé. Un preparato a fine
/// dichiarata resta rifiutato, e la coda si ferma sul brano vuoto: è un caso
/// raro, e molto meno grave.
#[test]
fn ripeti_uno_su_un_brano_vuoto_non_gira_in_tondo() {
    let (registro, osservatore) = spia();
    let mut banco = banco_con(48_000, osservatore);
    let vuoto = aperto(&banco.ctx, brano_onesto(7, 48_000, 0.5, 0));
    banco.ctx.esegui(suona(vuoto));
    let mut visti = 0usize;
    let mut fra_quanti: Option<u32> = None;
    for _ in 0..2_000 {
        let iniziati = eventi(&registro)
            .iter()
            .filter(|e| e.as_str() == "iniziato:7")
            .count();
        if iniziati > visti && fra_quanti.is_none() {
            visti = iniziati;
            fra_quanti = Some(3);
        }
        if let Some(n) = fra_quanti.as_mut() {
            if *n == 0 {
                let ancora = aperto(&banco.ctx, brano_onesto(7, 48_000, 0.5, 0));
                banco.ctx.esegui(Comando::Prepara(Some(Box::new(ancora))));
                fra_quanti = None;
            } else {
                *n -= 1;
            }
        }
        banco.ctx.riempi();
        let mut presi = 0u64;
        while presi < BLOCCO_FINTO {
            if banco.consumatore.pop().is_err() {
                break;
            }
            presi = presi.saturating_add(1);
        }
        banco
            .condiviso
            .fotogrammi
            .fetch_add(presi, Ordering::Relaxed);
        banco.ctx.aggiorna();
    }
    let storia = eventi(&registro);
    let iniziati = storia.iter().filter(|e| e.as_str() == "iniziato:7").count();
    let fermati = storia.iter().filter(|e| e.as_str() == "fermato").count();
    assert!(
        iniziati <= 2,
        "ripeti-uno su brano vuoto: {iniziati} iniziato, {fermati} fermato in 2000 giri"
    );
}

// ── la condivisione che muore a metà brano ──────────────────────────────

/// Byte che a un certo punto smettono di rispondere.
///
/// I primi `soglia` byte arrivano interi, poi ogni lettura restituisce il
/// numero di Windows che si è scelto. Con il 64 —
/// `ERROR_NETNAME_DELETED` — è una condivisione che è morta a metà brano:
/// il modo in cui una VPN che cade o un NAS spento si presentano, e
/// l'unico modo di riprodurre quel guasto senza un NAS vero. Con un numero
/// qualunque che non sia di rete è invece un settore illeggibile del disco
/// di casa. Le due prove che seguono si distinguono **solo** per quel
/// numero, che è esattamente la distinzione che il motore deve saper fare.
struct ByteCheFalliscono {
    dentro: std::io::Cursor<Vec<u8>>,
    soglia: u64,
    letti: u64,
    numero: i32,
}

impl std::io::Read for ByteCheFalliscono {
    fn read(&mut self, dove: &mut [u8]) -> std::io::Result<usize> {
        if self.letti >= self.soglia {
            return Err(std::io::Error::from_raw_os_error(self.numero));
        }
        let quanti = std::io::Read::read(&mut self.dentro, dove)?;
        self.letti = self
            .letti
            .saturating_add(u64::try_from(quanti).unwrap_or(0));
        Ok(quanti)
    }
}

impl std::io::Seek for ByteCheFalliscono {
    fn seek(&mut self, da: std::io::SeekFrom) -> std::io::Result<u64> {
        std::io::Seek::seek(&mut self.dentro, da)
    }
}

impl crate::decodifica::Flusso for ByteCheFalliscono {
    fn lunghezza(&self) -> Option<u64> {
        // Il file **dichiara** la sua lunghezza vera: la share è morta dopo
        // che il sistema l'aveva già detta, ed è proprio questo a rendere
        // il guasto invisibile finché non si legge.
        u64::try_from(self.dentro.get_ref().len()).ok()
    }
}

/// Un brano che si interrompe a metà con il numero d'errore dato.
///
/// La soglia sta a metà dei byte perché symphonia deve fare in tempo a
/// riconoscere il WAV e a leggerne qualche pacchetto: un guasto che
/// arrivasse subito verrebbe scambiato per un file di formato ignoto, che è
/// un'altra strada e un altro errore, e la prova non guarderebbe più il
/// punto che deve guardare.
fn brano_che_si_interrompe(track_id: i64, frequenza: u32, numero: i32) -> Sorgente {
    let quanti = fotogrammi_da_ms(1_000, frequenza);
    let campioni = vec![0.5f32; usize::try_from(quanti).unwrap_or(48_000)];
    let byte = wav(frequenza, &campioni);
    let soglia = u64::try_from(byte.len().div_ceil(2)).unwrap_or(0);
    Sorgente {
        track_id,
        media: Box::new(ByteCheFalliscono {
            dentro: std::io::Cursor::new(byte),
            soglia,
            letti: 0,
            numero,
        }),
        estensione: Some("wav".to_owned()),
        durata_ms: 1_000,
        replaygain_db: None,
        picco: None,
        segmento: None,
    }
}

/// La condivisione che sparisce: `ERROR_NETNAME_DELETED`.
fn brano_che_muore(track_id: i64, frequenza: u32) -> Sorgente {
    brano_che_si_interrompe(track_id, frequenza, 64)
}

/// Il file davvero rotto sul disco di casa: `ERROR_CRC`, un settore che non
/// si legge più. Aspettare non lo aggiusta, e infatti va saltato.
fn brano_rotto(track_id: i64, frequenza: u32) -> Sorgente {
    brano_che_si_interrompe(track_id, frequenza, 23)
}

/// Gli eventi annunciati, in ordine, riletti dalla prova che li ha chiesti.
type Registro = Arc<std::sync::Mutex<Vec<String>>>;

/// La stessa scatola che il motore vero riceve dall'applicazione.
type Osservatore = Box<dyn Fn(Evento) + Send>;

/// Raccoglie gli eventi che il motore annuncia, per poterli leggere dopo.
fn spia() -> (Registro, Osservatore) {
    let registro = Arc::new(std::sync::Mutex::new(Vec::new()));
    let dentro = Arc::clone(&registro);
    let osservatore: Osservatore = Box::new(move |evento| {
        let riga = match evento {
            Evento::Iniziato { track_id } => format!("iniziato:{track_id}"),
            Evento::Fermato => "fermato".to_owned(),
            Evento::Errore(err) => format!(
                "errore:{}:{}",
                err.code().kind().code(),
                if err.is_retryable() {
                    "riprovabile"
                } else {
                    "definitivo"
                }
            ),
        };
        if let Ok(mut elenco) = dentro.lock() {
            elenco.push(riga);
        }
    });
    (registro, osservatore)
}

fn eventi(registro: &Registro) -> Vec<String> {
    registro.lock().map(|e| e.clone()).unwrap_or_default()
}

/// Quanti annunci sono arrivati, senza copiarli.
///
/// [`eventi`] clona l'elenco, che va benissimo a fine prova e non va bene
/// dentro un ciclo che gira qualche migliaio di volte: qui serve solo il
/// numero, e copiare delle stringhe per contarle sarebbe il genere di spreco
/// che rende lente le suite.
fn quanti_eventi(registro: &Registro) -> usize {
    registro.lock().map(|e| e.len()).unwrap_or(0)
}

// ── il FLAC vero ──────────────────────────────────────────────────────

/// Mezzo secondo a 44100, mono, a livello costante.
///
/// # Perché un file e non dei byte costruiti qui
///
/// Perché fino a ieri nessuna prova di questo crate decodificava un FLAC, e
/// il FLAC è il formato su cui pesano le due cose che questo giro ha
/// toccato: il riconoscimento del contenitore, che si porta dietro tutti i
/// blocchi di metadati, e il salto, che passa per la seek table. Costruire
/// un FLAC in memoria vorrebbe dire scrivere un codificatore — cioè provare
/// il proprio codificatore contro il decodificatore di symphonia, che non
/// dice niente su nessuno dei due.
///
/// Sono centocinquantaquattro byte: un livello costante si comprime quasi a
/// niente, ed è quel che serve a un campione che deve stare in un
/// repository.
///
/// 44100 e non 48000 apposta: così passa anche dal ricampionatore, che è la
/// strada di tutti i file veri di questa libreria.
const FLAC_COSTANTE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/campioni/costante.flac"
));

fn flac(track_id: i64) -> Sorgente {
    Sorgente {
        track_id,
        media: Box::new(Byte(std::io::Cursor::new(FLAC_COSTANTE.to_vec()))),
        estensione: Some("flac".to_owned()),
        durata_ms: 500,
        replaygain_db: None,
        picco: None,
        segmento: None,
    }
}

/// Tutti i campioni che un decodificatore consegna, fino alla fine.
fn tutto(decodificatore: &mut crate::decodifica::Decodificatore) -> Vec<f32> {
    let mut fuori = Vec::new();
    let mut blocco = Vec::new();
    while decodificatore.prossimo(&mut blocco).expect("decodifica") {
        fuori.extend(blocco.iter().copied());
    }
    fuori
}

#[test]
fn un_flac_vero_si_decodifica_e_si_ricampiona() {
    let mut decodificatore =
        crate::decodifica::Decodificatore::apri(flac(1), 48_000, 1).expect("il FLAC si apre");

    let campioni = tutto(&mut decodificatore);

    // Mezzo secondo a 44100 riportato a 48000 fa ventiquattromila
    // fotogrammi. Il ricampionatore lavora a blocchi e può consegnarne
    // qualcuno in meno o in più agli estremi: la tolleranza copre quello,
    // non un errore di frequenza — che sarebbe di migliaia di campioni.
    let attesi = 24_000_i64;
    let scarto = (i64::try_from(campioni.len()).unwrap_or(0) - attesi).abs();
    assert!(
        scarto < 500,
        "attesi ~{attesi} campioni a 48 kHz, ne sono arrivati {}",
        campioni.len()
    );

    // E il suono è quello che c'era dentro: un livello costante resta
    // costante, salvo le code del ricampionatore ai due estremi.
    let dentro = &campioni[1_000..campioni.len().saturating_sub(1_000)];
    assert!(
        quanti_a(dentro, 0.5) * 10 > dentro.len() * 9,
        "il livello non è quello inciso nel file"
    );
}

#[test]
fn un_flac_vero_si_puo_saltare() {
    // Il salto su FLAC è la strada che [`Decodificatore::cerca`] percorre
    // davvero, ed è quella che nessun'altra prova di questo crate tocca: sul
    // WAV il punto si calcola, qui si cerca fra i blocchi.
    let intero = {
        let mut d =
            crate::decodifica::Decodificatore::apri(flac(1), 48_000, 1).expect("il FLAC si apre");
        tutto(&mut d).len()
    };

    let mut decodificatore =
        crate::decodifica::Decodificatore::apri(flac(1), 48_000, 1).expect("il FLAC si apre");
    decodificatore.cerca(250).expect("il salto riesce");
    let dopo = tutto(&mut decodificatore).len();

    // Si confronta con il decodificato intero, non con un numero scritto a
    // mano, e la tolleranza è larga per una ragione precisa: **non si
    // atterra a metà esatta**. Questo campione non porta una `SEEKTABLE` —
    // è troppo corto perché il codificatore ne scriva una — quindi
    // symphonia si ferma al confine del blocco FLAC più vicino, che sono
    // 4096 fotogrammi alla volta. Su mezzo secondo la granularità si vede,
    // ed è il comportamento giusto: quel che conta è che il salto abbia
    // spostato la puntina e non l'abbia buttata fuori dal brano.
    assert!(
        dopo < intero,
        "dopo un salto in avanti deve uscire meno musica: {dopo} contro {intero}"
    );
    assert!(
        dopo * 4 > intero,
        "il salto ha portato quasi alla fine del brano: {dopo} contro {intero}"
    );
}

#[test]
fn un_salto_oltre_la_fine_non_dice_che_il_file_e_danneggiato() {
    // Trascinare il cursore fino in fondo. Prima diceva che il file era
    // danneggiato: symphonia rifiuta con `SeekError(OutOfRange)` appena
    // `ts > num_frames`, il ripiego lo traduceva in `playback.decodeFailed`,
    // e il catalogo dichiara quel codice `Warning, Never` ritentabile —
    // cioè all'utente arrivava «questo file è danneggiato, sostituiscilo».
    // Su un FLAC succedeva **sempre**, perché il cursore arriva in fondo
    // ogni volta che qualcuno ce lo trascina.
    let mut decodificatore =
        crate::decodifica::Decodificatore::apri(flac(1), 48_000, 1).expect("il FLAC si apre");

    // Cinque secondi oltre la fine dichiarata: nessun dubbio che sia fuori.
    decodificatore
        .cerca(500 + 5_000)
        .expect("saltare oltre la fine non è un guasto");

    // Il tetto di `cerca` ferma il salto poco prima dell'ultimo fotogramma,
    // quindi quel che resta è la manciata di millisecondi del margine: si
    // consuma, e deve finire da sé.
    let resto = tutto(&mut decodificatore);
    assert!(
        // Ventiquattromila campioni a 48 kHz sono il mezzo secondo che il
        // file contiene per intero: restarne così tanti vorrebbe dire che
        // il salto non è avvenuto.
        resto.len() < 24_000,
        "dopo un salto in fondo non deve restare mezzo secondo di musica: {}",
        resto.len()
    );

    // E il giro dopo il brano è finito, senza errore: è così che la coda
    // avanza al brano successivo invece di fermarsi con un avviso.
    let mut blocco = Vec::new();
    assert!(
        !decodificatore
            .prossimo(&mut blocco)
            .expect("dopo la fine non c'è niente da leggere, e non è un guasto"),
        "il brano doveva dichiararsi finito"
    );
}

#[test]
fn un_flac_su_una_share_che_muore_non_diventa_un_formato_ignoto() {
    // Il caso vero, e il più caro proprio su FLAC: il riconoscimento del
    // contenitore legge **tutti** i blocchi di metadati prima di poter dire
    // di che formato si tratta — su un disco con la copertina incorporata
    // sono centinaia di kilobyte, tutti dalla rete. Se la share cade lì in
    // mezzo, symphonia dice soltanto «nessun lettore adatto» e butta via il
    // numero del sistema: senza il testimone in `decodifica.rs` l'utente si
    // sentirebbe dire che il suo FLAC è di un formato che Rymembra non sa
    // leggere.
    let sorgente = Sorgente {
        track_id: 1,
        media: Box::new(ByteCheFalliscono {
            // Solo i primi quaranta byte esistono: l'intestazione `fLaC` e
            // l'inizio dello `STREAMINFO`. Il resto è la share che se n'è
            // andata mentre si leggevano i blocchi di metadati — che sul
            // FLAC sono la parte lunga della lettura.
            dentro: std::io::Cursor::new(FLAC_COSTANTE[..40].to_vec()),
            soglia: 40,
            letti: 0,
            numero: 64,
        }),
        estensione: Some("flac".to_owned()),
        durata_ms: 500,
        replaygain_db: None,
        picco: None,
        segmento: None,
    };

    let Err(err) = crate::decodifica::Decodificatore::apri(sorgente, 48_000, 1) else {
        panic!("con la rete giù a metà intestazione non si apre niente");
    };

    assert_eq!(
        err.code().kind(),
        ErrorCodeKind::FsNetworkUnavailable,
        "un FLAC su share morta deve dire «rete» (causa: {:?})",
        err.cause()
    );
    assert!(err.code().is_retryable(), "la rete torna: si ritenta");
}

// ── l'Opus vero ────────────────────────────────────────────────────────

/// Tre secondi di seno a 1 kHz, mono, ad ampiezza 0,5, a 16 kbit/s.
///
/// # Perché un tono e non un livello costante come il FLAC
///
/// Perché Opus la continua non la trasporta: SILK e CELT tolgono il DC
/// tutti e due, e un livello fisso si ricostruirebbe come quasi-silenzio.
/// Una prova che ci misurasse sopra un'ampiezza sarebbe verde per il motivo
/// sbagliato — verde su un decodificatore rotto quanto su uno giusto. Un
/// seno invece sopravvive, e il suo valore efficace è un numero che si può
/// confrontare con quello che ne ricava l'implementazione di riferimento.
///
/// # Perché un file inciso da libopus
///
/// Perché è l'unico modo di provare qualcosa sul **nostro** decodificatore.
/// `opus-decoder` è Rust puro e dichiara di passare i dodici vettori di
/// RFC 8251, ma quei vettori non viaggiano col crate: qui il campione lo ha
/// scritto l'implementazione di riferimento, quindi se il porto in Rust
/// sbagliasse in modo grossolano questa prova diventerebbe rossa.
/// Costruire i pacchetti a mano vorrebbe dire provare il proprio
/// codificatore contro il proprio decodificatore, che non dice niente su
/// nessuno dei due.
///
/// # Perché tre secondi, che per un campione sono tanti
///
/// Perché sotto non ci sono abbastanza **pagine Ogg**, e senza pagine il
/// gapless non si esercita. Mezzo secondo di tono entra tutto in una pagina
/// sola, e con una pagina sola il demultiplatore deve indovinare quale
/// silenzio è di testa e quale di coda — symphonia 0.5 attribuiva
/// l'imbottitura finale al pre-skip e non tagliava niente, la 0.6 suppone
/// che il flusso cominci da zero — e la prova finirebbe per misurare un
/// caso che a un file vero non capita. A tre secondi le pagine audio sono
/// quattro, il `trim_end` dell'ultima arriva davvero a `decodifica::opus`,
/// e quel che si prova è la strada che percorre la musica di qualcuno.
///
/// Sedici kilobit al secondo per tenerlo sotto i diecimila byte: un tono
/// puro si comprime bene, ed è quel che serve a un campione che deve stare
/// in un repository.
///
/// 48000 e non 44100 come il FLAC, e non è una scelta: Opus decodifica
/// sempre a 48 kHz. Il ricampionatore, che il FLAC esercita, qui resta
/// fuori dai piedi apposta — quel che si prova è il codec.
const OPUS_TONO: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/campioni/tono.opus"
));

/// Quanti fotogrammi dura [`OPUS_TONO`] una volta tolti pre-skip e coda.
const OPUS_FOTOGRAMMI: i64 = 144_000;

/// Il valore efficace che libopus ricava da [`OPUS_TONO`].
///
/// Misurato decodificando quel file con l'implementazione di riferimento e
/// scartando cinquanta millesimi ai due estremi, dove il tono attacca e
/// smette. È il numero contro cui si confronta il nostro decodificatore.
const OPUS_EFFICACE: f32 = 0.3519;

fn opus(track_id: i64, estensione: &str) -> Sorgente {
    Sorgente {
        track_id,
        media: Box::new(Byte(std::io::Cursor::new(OPUS_TONO.to_vec()))),
        estensione: Some(estensione.to_owned()),
        durata_ms: 3_000,
        replaygain_db: None,
        picco: None,
        segmento: None,
    }
}

/// Il valore efficace di un blocco di campioni.
fn efficace(campioni: &[f32]) -> f32 {
    if campioni.is_empty() {
        return 0.0;
    }
    let somma: f32 = campioni.iter().map(|c| c * c).sum();
    (somma / campioni.len() as f32).sqrt()
}

/// Il tono, misurato al netto degli estremi.
///
/// Cinquemila campioni via da una parte e dall'altra: all'inizio c'è
/// l'attacco, in fondo la coda, e dopo un salto c'è anche il tratto in cui
/// un codec con stato si riallinea. Nessuno dei tre dice niente sul livello
/// del tono, che è quel che queste prove misurano.
fn tono_di(campioni: &[f32]) -> f32 {
    if campioni.len() <= 10_000 {
        return 0.0;
    }
    efficace(&campioni[5_000..campioni.len() - 5_000])
}

#[test]
fn un_opus_vero_si_decodifica_e_suona_il_tono_inciso() {
    let mut decodificatore = crate::decodifica::Decodificatore::apri(opus(1, "opus"), 48_000, 1)
        .expect("l'Opus si apre");

    let campioni = tutto(&mut decodificatore);

    // Tre secondi a 48000 fanno centoquarantaquattromila fotogrammi, ed è
    // un'uguaglianza **esatta**, non una tolleranza: non c'è ricampionatore
    // di mezzo, e i due tagli sono l'uno il complemento dell'altro.
    //
    // È anche l'unica asserzione che li prova. Senza il taglio di coda ne
    // arriverebbero 144648; senza quello di testa, 144312 — che è quel che
    // arrivava davvero prima che `decodifica::opus` si contasse il pre-skip
    // da sé, e che una tolleranza larga avrebbe lasciato passare. Un numero
    // diverso da questo dice quale delle due mani ha smesso di lavorare.
    assert_eq!(
        i64::try_from(campioni.len()).unwrap_or(0),
        OPUS_FOTOGRAMMI,
        "tre secondi a 48 kHz sono {OPUS_FOTOGRAMMI} fotogrammi tondi"
    );

    // E il tono è quello inciso. La tolleranza è larga perché un codec con
    // perdita non restituisce i campioni di partenza; è stretta abbastanza
    // da accorgersi di un guadagno applicato male, di un canale scambiato o
    // di rumore al posto della musica.
    let tono = tono_di(&campioni);
    assert!(
        (tono - OPUS_EFFICACE).abs() < 0.03,
        "il valore efficace dice {tono}, libopus su questo file dice {OPUS_EFFICACE}"
    );
    // E non è un livello continuo travestito da tono: un seno ha un picco
    // che sta una spanna sopra il suo valore efficace, e sotto l'unità.
    let picco = campioni.iter().fold(0.0_f32, |max, c| max.max(c.abs()));
    assert!(
        picco > tono && picco < 1.0,
        "picco {picco} contro valore efficace {tono}: non è la forma di un seno"
    );
}

#[test]
fn un_opus_vero_si_puo_saltare() {
    // Il salto su Opus merita una prova sua: è un codec con stato — ogni
    // pacchetto continua il precedente — quindi dopo aver spostato la
    // puntina il decodificatore va azzerato, o quel che esce sono i resti
    // della finestra di prima. È `Decoder::reset`, che `cerca` chiama e che
    // qui si verifica che serva a qualcosa.
    let intero = {
        let mut d = crate::decodifica::Decodificatore::apri(opus(1, "opus"), 48_000, 1)
            .expect("l'Opus si apre");
        tutto(&mut d).len()
    };

    let mut decodificatore = crate::decodifica::Decodificatore::apri(opus(1, "opus"), 48_000, 1)
        .expect("l'Opus si apre");
    decodificatore.cerca(1_500).expect("il salto riesce");
    let dopo = tutto(&mut decodificatore);

    assert!(
        dopo.len() < intero,
        "dopo un salto in avanti deve uscire meno musica: {} contro {intero}",
        dopo.len()
    );
    // E quel che esce dopo il salto è ancora il tono, non il silenzio né il
    // rumore che darebbe uno stato non azzerato.
    let tono = tono_di(&dopo);
    assert!(
        (tono - OPUS_EFFICACE).abs() < 0.03,
        "dopo il salto il tono dovrebbe continuare: valore efficace {tono}"
    );
}

#[test]
fn un_opus_dentro_un_ogg_si_suona_lo_stesso() {
    // Il caso che prima del decodificatore cadeva nel punto peggiore:
    // `.ogg` era già fra le estensioni suonabili, quindi il file passava il
    // cancello delle estensioni, il demultiplatore lo apriva, riconosceva
    // il flusso Opus — e poi `make()` non trovava un decodificatore, e
    // all'utente arrivava «formato non supportato» per un contenitore che
    // l'app apre benissimo.
    //
    // Sono gli stessi byte: quel che cambia è solo l'estensione dichiarata,
    // cioè il suggerimento che arriva al riconoscitore. Dentro un Ogg il
    // codec lo dice la pagina, non il nome del file.
    let mut decodificatore = crate::decodifica::Decodificatore::apri(opus(1, "ogg"), 48_000, 1)
        .expect("un Ogg che dentro è Opus si apre");

    let campioni = tutto(&mut decodificatore);
    let tono = tono_di(&campioni);
    assert!(
        (tono - OPUS_EFFICACE).abs() < 0.03,
        "il tono dentro l'Ogg dovrebbe essere lo stesso: valore efficace {tono}"
    );
}

/// Byte che smettono di rispondere **quando lo si decide**.
///
/// A differenza di [`ByteCheFalliscono`], che cade dopo un tot di byte, qui
/// l'interruttore è in mano alla prova: serve a far morire la share in un
/// punto preciso della vita del decodificatore — fra l'apertura e il salto,
/// per esempio, che non è un momento riproducibile contando i byte letti.
///
/// Fallisce anche il riposizionamento, e non è un dettaglio: un salto su una
/// condivisione morta è proprio una `SetFilePointer` che non torna.
struct ByteCheMuoiono {
    dentro: std::io::Cursor<Vec<u8>>,
    morta: Arc<std::sync::atomic::AtomicBool>,
    numero: i32,
}

impl ByteCheMuoiono {
    fn e_morta(&self) -> bool {
        self.morta.load(Ordering::Relaxed)
    }
}

impl std::io::Read for ByteCheMuoiono {
    fn read(&mut self, dove: &mut [u8]) -> std::io::Result<usize> {
        if self.e_morta() {
            return Err(std::io::Error::from_raw_os_error(self.numero));
        }
        std::io::Read::read(&mut self.dentro, dove)
    }
}

impl std::io::Seek for ByteCheMuoiono {
    fn seek(&mut self, da: std::io::SeekFrom) -> std::io::Result<u64> {
        if self.e_morta() {
            return Err(std::io::Error::from_raw_os_error(self.numero));
        }
        std::io::Seek::seek(&mut self.dentro, da)
    }
}

impl crate::decodifica::Flusso for ByteCheMuoiono {
    fn lunghezza(&self) -> Option<u64> {
        // Dichiarata sempre, anche da morta: il sistema l'aveva già detta
        // quando la share rispondeva ancora, ed è proprio questo a rendere
        // il flusso «saltabile» agli occhi di symphonia.
        u64::try_from(self.dentro.get_ref().len()).ok()
    }
}

/// Un brano con l'interruttore della rete, e l'interruttore.
fn brano_con_interruttore(
    track_id: i64,
    frequenza: u32,
    numero: i32,
) -> (Sorgente, Arc<std::sync::atomic::AtomicBool>) {
    let quanti = fotogrammi_da_ms(2_000, frequenza);
    let campioni = vec![0.5f32; usize::try_from(quanti).unwrap_or(96_000)];
    let morta = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let sorgente = Sorgente {
        track_id,
        media: Box::new(ByteCheMuoiono {
            dentro: std::io::Cursor::new(wav(frequenza, &campioni)),
            morta: Arc::clone(&morta),
            numero,
        }),
        estensione: Some("wav".to_owned()),
        durata_ms: 2_000,
        replaygain_db: None,
        picco: None,
        segmento: None,
    };
    (sorgente, morta)
}

#[test]
fn una_share_morta_all_apertura_non_dice_che_il_formato_e_ignoto() {
    // La rete cade fra la `File::open` e il riconoscimento del contenitore.
    // symphonia non riesce a leggere nemmeno l'intestazione, e il guasto
    // usciva come `playback.formatUnsupported` — che il catalogo dichiara
    // mai ritentabile, cioè niente «Riprova» per un file che non ha niente
    // che non va. Il percorso più caro è proprio quello del FLAC, dove il
    // riconoscimento legge anche la copertina incorporata prima di dire di
    // che formato si tratta.
    let (sorgente, morta) = brano_con_interruttore(1, 48_000, 64);
    morta.store(true, Ordering::Relaxed);

    let Err(err) = crate::decodifica::Decodificatore::apri(sorgente, 48_000, 1) else {
        panic!("con la rete giù non si apre niente");
    };

    assert_eq!(
        err.code().kind(),
        ErrorCodeKind::FsNetworkUnavailable,
        "la share morta all'apertura deve dire «rete», non «formato» (causa: {:?})",
        err.cause()
    );
    assert!(err.code().is_retryable(), "la rete torna: si ritenta");
}

#[test]
fn un_contenitore_davvero_ignoto_resta_un_formato_non_supportato() {
    // Il contrappeso della prova qui sopra: il ramo di rete non deve
    // inghiottire il caso normale. Dei byte che non sono un formato audio
    // restano un formato che non si sa suonare, e riprovare non serve.
    let sorgente = Sorgente {
        track_id: 1,
        media: Box::new(Byte(std::io::Cursor::new(vec![0u8; 4_096]))),
        estensione: Some("wav".to_owned()),
        durata_ms: 0,
        replaygain_db: None,
        picco: None,
        segmento: None,
    };

    let Err(err) = crate::decodifica::Decodificatore::apri(sorgente, 48_000, 1) else {
        panic!("dei byte a zero non sono un file audio");
    };

    assert_eq!(
        err.code().kind(),
        ErrorCodeKind::PlaybackFormatUnsupported,
        "senza rete di mezzo il codice resta quello del formato"
    );
}

#[test]
fn una_share_che_muore_durante_un_salto_non_dice_che_il_file_e_rotto() {
    // Il buco più stretto e il più insidioso: il brano si è aperto quando la
    // rete c'era, si sente, e la share muore mentre qualcuno trascina il
    // cursore. Il salto è il momento peggiore in cui trovarsela morta —
    // `SeekMode::Accurate` va al punto e poi ridecodifica fino al fotogramma
    // esatto, e su FLAC ci mette in mezzo pure la seek table — e finché il
    // guasto usciva come `playback.decodeFailed`, l'utente si sentiva dire
    // che il file era danneggiato mentre gli mancava solo il cavo.
    let (sorgente, morta) = brano_con_interruttore(1, 48_000, 64);
    let mut decodificatore = crate::decodifica::Decodificatore::apri(sorgente, 48_000, 1)
        .expect("con la rete viva si apre");

    morta.store(true, Ordering::Relaxed);
    let err = decodificatore
        .cerca(1_000)
        .expect_err("su una share morta il salto non riesce");

    assert_eq!(
        err.code().kind(),
        ErrorCodeKind::FsNetworkUnavailable,
        "un salto a rete giù deve dire «rete», non «file danneggiato»"
    );
    assert!(
        err.code().is_retryable(),
        "senza questo la finestra non offre «Riprova» e non si annota dove tornare"
    );
}

#[test]
fn un_salto_fallito_su_un_disco_rotto_resta_un_guasto_di_decodifica() {
    // Stessa prova, un numero diverso: `ERROR_CRC`, cioè un settore che non
    // si legge più sul disco di casa. Aspettare non lo aggiusta, e il codice
    // deve restare quello che non offre «Riprova».
    let (sorgente, morta) = brano_con_interruttore(1, 48_000, 23);
    let mut decodificatore = crate::decodifica::Decodificatore::apri(sorgente, 48_000, 1)
        .expect("con il disco sano si apre");

    morta.store(true, Ordering::Relaxed);
    let err = decodificatore
        .cerca(1_000)
        .expect_err("il salto non riesce");

    assert_eq!(err.code().kind(), ErrorCodeKind::PlaybackDecodeFailed);
    assert!(
        !err.code().is_retryable(),
        "rileggere un settore rotto dà lo stesso esito"
    );
}

#[test]
fn la_share_che_muore_a_meta_brano_non_dice_che_il_file_e_rotto() {
    // Il danno che questa prova tiene chiuso: `playback.decodeFailed` in
    // italiano dice «questo file è danneggiato, sostituiscilo e rifai la
    // scansione». Detto per un cavo staccato è il consiglio peggiore
    // possibile — manda l'utente a buttare un file sano. Deve arrivare
    // invece l'errore di rete, che è ritentabile e dice la verità.
    let (registro, osservatore) = spia();
    let mut banco = banco_con(48_000, osservatore);
    let primo = aperto(&banco.ctx, brano_che_muore(1, 48_000));
    banco.ctx.esegui(suona(primo));

    let _ = suona_tutto(&mut banco, |_| {});

    let visti = eventi(&registro);
    assert!(
        visti
            .iter()
            .any(|e| e == "errore:fs.networkUnavailable:riprovabile"),
        "la share morta non si annuncia come guasto di rete ritentabile: {visti:?}"
    );
    assert!(
        !visti
            .iter()
            .any(|e| e.starts_with("errore:playback.decode")),
        "l'utente si sente dire che il file è danneggiato: {visti:?}"
    );
}

#[test]
fn la_share_che_muore_non_brucia_la_coda() {
    // Il brano dopo sta sulla stessa condivisione morta: passare a lui vuol
    // dire un'altra attesa e un altro avviso, e così via fino in fondo alla
    // coda. Il motore deve fermarsi sul brano dove si era, non scorrere.
    let (registro, osservatore) = spia();
    let mut banco = banco_con(48_000, osservatore);
    let primo = aperto(&banco.ctx, brano_che_muore(1, 48_000));
    banco.ctx.esegui(suona(primo));
    let secondo = aperto(&banco.ctx, brano(2, 48_000, 0.2));
    banco.ctx.esegui(Comando::Prepara(Some(Box::new(secondo))));

    let fuori = suona_tutto(&mut banco, |_| {});

    let visti = eventi(&registro);
    // Il brano dopo sta a un livello suo: se se ne sente anche solo un
    // pezzo, il motore è passato a lui invece di fermarsi.
    assert!(
        quanti_a(&fuori, 0.2) == 0,
        "la coda è scorsa lo stesso sulla share morta: si sente il brano dopo"
    );
    assert!(
        visti.iter().any(|e| e == "fermato"),
        "il motore non ha dichiarato di essersi fermato: {visti:?}"
    );
    assert!(
        banco.ctx.corrente.is_none() && banco.ctx.prossimo.is_none(),
        "il motore ha lasciato roba in canna invece di fermarsi"
    );
}

#[test]
fn un_file_davvero_rotto_continua_a_far_saltare_il_brano() {
    // Il rovescio della medaglia: un album con dentro un file corrotto deve
    // arrivare in fondo lo stesso. La distinzione la fa il codice
    // dell'errore, non il fatto che ci sia un errore.
    let (registro, osservatore) = spia();
    let mut banco = banco_con(48_000, osservatore);
    let primo = aperto(&banco.ctx, brano_rotto(1, 48_000));
    banco.ctx.esegui(suona(primo));
    let secondo = aperto(&banco.ctx, brano(2, 48_000, 0.2));
    banco.ctx.esegui(Comando::Prepara(Some(Box::new(secondo))));

    let fuori = suona_tutto(&mut banco, |_| {});

    let visti = eventi(&registro);
    assert!(
        visti
            .iter()
            .any(|e| e == "errore:playback.decodeFailed:definitivo"),
        "un settore illeggibile si annuncia come guasto di rete: {visti:?}"
    );
    assert!(
        !visti.iter().any(|e| e == "fermato"),
        "un file rotto ferma l'album invece di costare un fruscio: {visti:?}"
    );
    assert!(
        quanti_a(&fuori, 0.2) > 5_000,
        "il brano dopo quello rotto non si sente: la coda si è bruciata su di lui"
    );
}

// ── il filo che cade, e il motore che è stato sostituito ────────────────

/// Byte che a metà lettura non restituiscono un errore: **panicano**.
///
/// Il gemello di [`ByteCheFalliscono`], e la differenza è tutta lì. Quello
/// riproduce una share che muore, cioè un guasto che sale come `Result` e
/// che il motore sa raccontare. Questo riproduce l'altro modo in cui un
/// file troncato può finire: un `assert` dentro il codice che lo legge.
/// Nel nostro albero `unwrap`, `expect`, `panic!` e l'indicizzazione sono
/// vietati, ma symphonia e rubato non sono il nostro albero — e byte
/// arrivati a metà da una Wi-Fi che respira male sono esattamente
/// l'ingresso che nessuno dei due ha mai visto.
struct BytePanicanti {
    dentro: std::io::Cursor<Vec<u8>>,
    soglia: u64,
    letti: u64,
}

impl std::io::Read for BytePanicanti {
    fn read(&mut self, dove: &mut [u8]) -> std::io::Result<usize> {
        assert!(
            self.letti < self.soglia,
            "i byte finiscono qui, e non con garbo"
        );
        let quanti = std::io::Read::read(&mut self.dentro, dove)?;
        self.letti = self
            .letti
            .saturating_add(u64::try_from(quanti).unwrap_or(0));
        Ok(quanti)
    }
}

impl std::io::Seek for BytePanicanti {
    fn seek(&mut self, da: std::io::SeekFrom) -> std::io::Result<u64> {
        std::io::Seek::seek(&mut self.dentro, da)
    }
}

impl crate::decodifica::Flusso for BytePanicanti {
    fn lunghezza(&self) -> Option<u64> {
        u64::try_from(self.dentro.get_ref().len()).ok()
    }
}

/// Un brano che fa cadere chi lo legge, a metà.
///
/// La soglia sta a metà per la stessa ragione di
/// [`brano_che_si_interrompe`]: symphonia deve fare in tempo ad aprire il
/// file, altrimenti la caduta arriverebbe dentro [`BranoAperto::apri`] —
/// che è un altro filo e un altro discorso.
fn brano_che_panica(track_id: i64, frequenza: u32) -> Sorgente {
    let quanti = fotogrammi_da_ms(1_000, frequenza);
    let campioni = vec![0.5f32; usize::try_from(quanti).unwrap_or(48_000)];
    let byte = wav(frequenza, &campioni);
    let soglia = u64::try_from(byte.len().div_ceil(2)).unwrap_or(0);
    Sorgente {
        track_id,
        media: Box::new(BytePanicanti {
            dentro: std::io::Cursor::new(byte),
            soglia,
            letti: 0,
        }),
        estensione: Some("wav".to_owned()),
        durata_ms: 1_000,
        replaygain_db: None,
        picco: None,
        segmento: None,
    }
}

#[test]
fn un_filo_che_cade_non_lascia_il_motore_muto() {
    // Il guasto che questa prova tiene chiuso, ed è il peggiore da
    // diagnosticare perché somiglia a niente: il filo della decodifica si
    // smontava in silenzio — il suo `JoinHandle` lo butta via nessuno — e
    // da fuori non cambiava una virgola. La callback continuava a servire
    // zeri, `perso` restava basso, il sorvegliante non vedeva niente da
    // riaprire, i comandi finivano in un canale senza ricevitore. L'utente
    // premeva pausa e non succedeva nulla.
    //
    // La traccia del panico compare su stderr durante questa prova: è il
    // gancio del diario che fa il suo lavoro, non un guasto della prova.
    let (registro, osservatore) = spia();
    let mut banco = banco_con(48_000, osservatore);
    let condiviso = Arc::clone(&banco.condiviso);
    let primo = aperto(&banco.ctx, brano_che_panica(1, 48_000));
    banco.ctx.esegui(suona(primo));

    let Banco {
        ctx,
        mut consumatore,
        condiviso: _,
        _manda,
        _curve,
        stretta: _stretta,
    } = banco;

    // Qualcuno deve svuotare l'anello, altrimenti il filo si addormenta su
    // un anello pieno molto prima di arrivare ai byte che lo fanno cadere:
    // sono le casse, che qui non ci sono.
    let basta = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let ferma = Arc::clone(&basta);
    let casse = std::thread::spawn(move || {
        while !ferma.load(Ordering::Relaxed) {
            while consumatore.pop().is_ok() {}
            std::thread::sleep(Duration::from_millis(1));
        }
    });

    let (fatto, aspetta) = std::sync::mpsc::channel();
    let decodifica = std::thread::spawn(move || {
        filo_sorvegliato(ctx);
        let _ = fatto.send(());
    });

    // Dieci secondi sono un'eternità per un brano da un secondo: se
    // scadono, il filo si è appeso invece di cadere, ed è comunque un
    // guasto da vedere.
    let tornato = aspetta.recv_timeout(Duration::from_secs(10));
    basta.store(true, Ordering::Relaxed);
    let _ = casse.join();
    let _ = decodifica.join();
    assert!(
        tornato.is_ok(),
        "il filo sorvegliato non è mai tornato: la rete non l'ha raccolto"
    );

    assert!(
        condiviso.perso.load(Ordering::Acquire),
        "il filo è caduto e nessuno ha alzato la bandiera: il sorvegliante \
         non avrà niente da riaprire"
    );
    assert_eq!(
        condiviso.causa_perdita.load(Ordering::Acquire),
        CAUSA_FILO_CADUTO,
        "la causa non distingue un decodificatore caduto da un cavo staccato"
    );

    let visti = eventi(&registro);
    assert!(
        visti
            .iter()
            .any(|e| e == "errore:playback.stalled:riprovabile"),
        "la caduta non si annuncia come riproduzione impantanata: {visti:?}"
    );
    assert!(
        !visti
            .iter()
            .any(|e| e.starts_with("errore:playback.decode")),
        "l'utente si sente dire che il file è danneggiato: {visti:?}"
    );
}

#[test]
fn un_motore_abbandonato_non_fa_avanzare_la_coda_di_chi_lo_ha_sostituito() {
    let (registro, osservatore) = spia();
    let mut banco = banco_con(48_000, osservatore);

    // Il contrappeso: da vivo il motore parla. Senza questa metà la prova
    // passerebbe anche con un osservatore mai collegato.
    let primo = aperto(&banco.ctx, brano(1, 48_000, 0.5));
    banco.ctx.esegui(suona(primo));
    banco.ctx.esegui(Comando::Ferma);
    assert_eq!(
        eventi(&registro),
        vec!["fermato".to_owned()],
        "un motore vivo deve dire quando si ferma"
    );

    // Il motore è stato sostituito: `Motore::drop` alza questa bandiera
    // **prima** di chiedere la chiusura, perché fra il comando e la morte
    // del filo passa un giro di ciclo — e in quel giro un `Fermato` in
    // ritardo chiuderebbe l'ascolto e farebbe avanzare la coda del motore
    // nuovo, che sta suonando.
    banco.condiviso.abbandonato.store(true, Ordering::Release);
    let secondo = aperto(&banco.ctx, brano(2, 48_000, 0.5));
    banco.ctx.esegui(suona(secondo));
    banco.ctx.esegui(Comando::Ferma);

    assert_eq!(
        eventi(&registro),
        vec!["fermato".to_owned()],
        "un motore abbandonato parla ancora allo stato di quello che lo ha \
         sostituito"
    );
}

/// Byte che arrivano piano: ogni lettura aspetta due millisecondi, come
/// una condivisione lenta che consegna a pezzi senza mai fermarsi.
struct ByteLenti(std::io::Cursor<Vec<u8>>);

impl std::io::Read for ByteLenti {
    fn read(&mut self, dove: &mut [u8]) -> std::io::Result<usize> {
        std::thread::sleep(Duration::from_millis(2));
        std::io::Read::read(&mut self.0, dove)
    }
}

impl std::io::Seek for ByteLenti {
    fn seek(&mut self, da: std::io::SeekFrom) -> std::io::Result<u64> {
        std::io::Seek::seek(&mut self.0, da)
    }
}

impl crate::decodifica::Flusso for ByteLenti {
    fn lunghezza(&self) -> Option<u64> {
        u64::try_from(self.0.get_ref().len()).ok()
    }
}

#[test]
fn riempi_cede_il_passo_a_una_sorgente_lenta() {
    // Un anello più grande del brano intero: senza un bilancio, una sola
    // chiamata decodificava fino in fondo al file — e per tutto quel tempo
    // il filo non eseguiva comandi, non aggiornava la posizione, non
    // annunciava niente. Con una sorgente lenta sono secondi.
    let mut banco = banco_con_anello(48_000, 500_000, Box::new(|_| {}));
    let campioni = vec![0.5f32; 480_000];
    let sorgente = Sorgente {
        track_id: 1,
        media: Box::new(ByteLenti(std::io::Cursor::new(wav(48_000, &campioni)))),
        estensione: Some("wav".to_owned()),
        durata_ms: 10_000,
        replaygain_db: None,
        picco: None,
        segmento: None,
    };
    let primo = aperto(&banco.ctx, sorgente);
    banco.ctx.esegui(suona(primo));

    assert!(banco.ctx.riempi(), "il primo giro ha lavorato");
    let consegnati = banco.ctx.corrente.as_ref().map(Decodificatore::consegnati);
    assert!(
        consegnati.is_some_and(|c| c < 240_000),
        "una sola chiamata ha decodificato quasi tutto il brano invece di \
         tornare al filo: consegnati {consegnati:?}"
    );

    // Il bilancio non perde niente: chiamata dopo chiamata il brano arriva
    // in fondo, intero.
    let mut fuori = 0usize;
    for _ in 0..100_000 {
        let lavorato = banco.ctx.riempi();
        while banco.consumatore.pop().is_ok() {
            fuori += 1;
        }
        if !lavorato && banco.ctx.corrente.is_none() {
            break;
        }
    }
    assert_eq!(fuori, 480_000, "il brano non è uscito intero");
}

#[test]
fn tornare_a_zero_e_una_corsa_come_le_altre() {
    // Spegnere l'equalizzatore passa di qui, non da un salto a piatto.
    let mut correnti = [8.0f32; BANDE];
    let bersaglio = [0.0f32; BANDE];
    avvicina(&mut correnti, &bersaglio, PASSI_EQ - 1);
    let primo = correnti.first().copied().unwrap_or(0.0);
    assert!(primo > 0.0 && primo < 8.0, "salto invece di rampa: {primo}");
}

// ── gli annunci su un filo loro ─────────────────────────────────────────

/// Un giro del ciclo di decodifica con la callback finta: decodifica,
/// consuma un blocco come farebbero le casse, traduce in una posizione.
/// Restituisce la posizione raccontata e se qualcosa si è mosso.
fn un_giro(banco: &mut Banco) -> (Posizione, bool) {
    let lavorato = banco.ctx.riempi();
    let mut presi = 0u64;
    while presi < BLOCCO_FINTO {
        if banco.consumatore.pop().is_err() {
            break;
        }
        presi = presi.saturating_add(1);
    }
    banco
        .condiviso
        .fotogrammi
        .fetch_add(presi, Ordering::Relaxed);
    banco.ctx.avanza_sfumatura();
    banco.ctx.aggiorna();
    let posizione = istantanea(&banco.ctx.posizione, &banco.ctx.intese);
    (posizione, lavorato || presi > 0)
}

/// Gli annunci interi, tenuti da parte per guardarli dopo.
type Annunci = Arc<std::sync::Mutex<Vec<Annuncio>>>;

/// Una voce che conserva gli annunci invece di passarli a qualcuno.
fn spia_annunci() -> (Annunci, Voce) {
    let annunci: Annunci = Arc::new(std::sync::Mutex::new(Vec::new()));
    let dentro = Arc::clone(&annunci);
    let voce: Voce = Box::new(move |annuncio| {
        if let Ok(mut elenco) = dentro.lock() {
            elenco.push(annuncio);
        }
    });
    (annunci, voce)
}

/// Come [`spia`] trascrive gli eventi, per confrontarli con una lista.
fn riga_di(evento: &Evento) -> String {
    match evento {
        Evento::Iniziato { track_id } => format!("iniziato:{track_id}"),
        Evento::Fermato => "fermato".to_owned(),
        Evento::Errore(err) => format!("errore:{}", err.code().kind().code()),
    }
}

#[test]
fn un_osservatore_lento_non_ferma_la_decodifica() {
    // Il difetto che il filo «rymembra-eventi» chiude: l'osservatore girava
    // sul filo della decodifica, e al cambio di brano prendeva il
    // lucchetto del lettore, scriveva l'ascolto sul database e aggiornava
    // la scheda di Windows — tutto mentre l'anello si svuotava. Qui
    // l'osservatore resta fermo su un cancello chiuso per tutto il tempo in
    // cui i due brani si decodificano: se il filo della decodifica lo
    // aspettasse, il ciclo qui sotto non finirebbe mai.
    let cancello = Arc::new(std::sync::Mutex::new(()));
    let chiuso = cancello.lock().expect("il cancello si chiude");
    let registro: Registro = Arc::new(std::sync::Mutex::new(Vec::new()));
    let osservatore: Box<dyn Fn(Annuncio) + Send> = {
        let cancello = Arc::clone(&cancello);
        let registro = Arc::clone(&registro);
        Box::new(move |annuncio| {
            let _passa = cancello.lock();
            if let Ok(mut elenco) = registro.lock() {
                elenco.push(riga_di(&annuncio.evento));
            }
        })
    };
    let voce = voce_su_filo(osservatore).expect("il filo degli eventi parte");
    let mut banco = banco_con_voce(48_000, 16_384, voce);
    let primo = aperto(&banco.ctx, brano(1, 48_000, 0.8));
    banco.ctx.esegui(suona(primo));
    let secondo = aperto(&banco.ctx, brano(2, 48_000, 0.4));
    banco.ctx.esegui(Comando::Prepara(Some(Box::new(secondo))));

    let mut finito = false;
    for _ in 0..200_000 {
        let (_, mosso) = un_giro(&mut banco);
        if !mosso && banco.ctx.corrente.is_none() && banco.ctx.fine_dichiarata {
            finito = true;
            break;
        }
    }
    assert!(
        finito,
        "la decodifica non arriva in fondo mentre l'osservatore è fermo"
    );
    assert!(
        eventi(&registro).is_empty(),
        "il cancello era chiuso: nessun annuncio poteva essere già arrivato"
    );

    // Si apre il cancello: gli annunci arrivano tutti, e nell'ordine in cui
    // sono successi.
    drop(chiuso);
    let scadenza = Instant::now() + Duration::from_secs(10);
    while quanti_eventi(&registro) < 3 && Instant::now() < scadenza {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        eventi(&registro),
        ["iniziato:1", "iniziato:2", "fermato"],
        "gli annunci sono arrivati in un altro ordine"
    );
}

#[test]
fn un_annuncio_partito_prima_di_una_svolta_e_superato() {
    // L'`Iniziato` di un brano, annunciato un istante prima che qualcuno ne
    // chiedesse un altro, arriva a chi sta sopra **dopo** la richiesta. Letto
    // com'è farebbe avanzare la coda oltre il brano appena scelto:
    // `superato` è il modo di riconoscerlo.
    let (annunci, voce) = spia_annunci();
    let mut banco = banco_con_voce(48_000, 16_384, voce);
    let primo = aperto(&banco.ctx, brano(1, 48_000, 0.8));
    // Come fa `Motore::suona`: la svolta si conta prima di partire.
    banco.ctx.intese.chieste.fetch_add(1, Ordering::SeqCst);
    banco.ctx.esegui(suona(primo));
    let _ = un_giro(&mut banco);
    let vecchio = annunci
        .lock()
        .ok()
        .and_then(|mut elenco| elenco.pop())
        .expect("il primo brano si annuncia");
    assert!(
        !vecchio.superato(),
        "appena partito non è superato da niente"
    );

    // Qualcuno chiede un altro brano: la maniglia conta la svolta, il filo
    // non l'ha ancora eseguita.
    banco.ctx.intese.chieste.fetch_add(1, Ordering::SeqCst);
    assert!(
        vecchio.superato(),
        "un annuncio partito prima della richiesta deve risultare superato"
    );

    // Il filo esegue: quel che annuncia da qui in poi è la risposta.
    let secondo = aperto(&banco.ctx, brano(2, 48_000, 0.4));
    banco.ctx.esegui(suona(secondo));
    let _ = un_giro(&mut banco);
    let nuovo = annunci
        .lock()
        .ok()
        .and_then(|mut elenco| elenco.pop())
        .expect("il secondo brano si annuncia");
    assert!(matches!(nuovo.evento, Evento::Iniziato { track_id: 2 }));
    assert!(
        !nuovo.superato(),
        "la risposta alla richiesta non è superata"
    );

    // E la voce del motore sostituito si spegne anche per gli annunci già
    // partiti: la bandiera si legge quando si riceve, non quando si manda.
    assert!(nuovo.ha_voce());
    banco.condiviso.abbandonato.store(true, Ordering::Release);
    assert!(
        !nuovo.ha_voce(),
        "un annuncio già in viaggio parla ancora per un motore sostituito"
    );
}

#[test]
fn l_annuncio_di_un_guasto_di_rete_porta_la_posizione_di_prima() {
    // «Riprova» deve sapere dove si era quando la rete è caduta, e il motore
    // subito dopo si ferma e azzera la posizione. Finché l'osservatore
    // girava sul filo della decodifica la leggeva in tempo; adesso che
    // arriva dopo, la posizione viaggia dentro l'annuncio.
    let (annunci, voce) = spia_annunci();
    let mut banco = banco_con_voce(48_000, 16_384, voce);
    let primo = aperto(&banco.ctx, brano_che_muore(1, 48_000));
    chiedi_un_brano(&banco);
    banco.ctx.esegui(suona(primo));
    for _ in 0..100_000 {
        let (_, mosso) = un_giro(&mut banco);
        if !mosso && banco.ctx.corrente.is_none() {
            break;
        }
    }
    let elenco = annunci.lock().expect("il registro si legge");
    let guasto = elenco
        .iter()
        .find(|a| matches!(a.evento, Evento::Errore(_)))
        .expect("la share morta si annuncia");
    assert_eq!(
        guasto.posizione.track_id,
        Some(1),
        "il guasto non dice più di quale brano era"
    );
    assert!(
        !guasto.posizione.in_pausa,
        "la musica stava andando: la ripresa deve ripartire da sola"
    );
    let fermato = elenco
        .iter()
        .find(|a| matches!(a.evento, Evento::Fermato))
        .expect("il motore si ferma");
    assert_eq!(fermato.posizione.track_id, None);
    assert!(
        fermato.posizione.in_pausa,
        "un motore fermo deve dirsi in pausa"
    );
    drop(elenco);
    assert!(
        banco.condiviso.in_pausa.load(Ordering::SeqCst),
        "fermo per la rete, la callback deve restare in silenzio"
    );
}

/// Il primo annuncio d'errore di una lista di annunci.
fn primo_errore(annunci: &Annunci) -> Option<(String, bool, Posizione)> {
    let elenco = annunci.lock().ok()?;
    elenco.iter().find_map(|a| match &a.evento {
        Evento::Errore(err) => Some((
            err.code().kind().code().to_owned(),
            a.superato(),
            a.posizione,
        )),
        _ => None,
    })
}

#[test]
fn l_errore_del_salto_iniziale_parla_del_brano_nuovo() {
    // «Riprova» riapre il brano annotato a un secondo con `suona_da`, e la
    // condivisione ricade proprio durante il salto. L'annuncio partiva
    // prima che la svolta fosse contata — quindi «superato», e chi sta
    // sopra non annotava la ripresa — e con la posizione del brano di
    // prima: il punto si perdeva.
    let (annunci, voce) = spia_annunci();
    let mut banco = banco_con_voce(48_000, 16_384, voce);
    let (sorgente, morta) = brano_con_interruttore(5, 48_000, 64);
    let brano = aperto(&banco.ctx, sorgente);
    morta.store(true, Ordering::Relaxed);
    chiedi_un_brano(&banco);
    banco.ctx.esegui(Comando::Suona {
        brano: Box::new(brano),
        da_ms: 1_000,
    });

    let (codice, superato, posizione) =
        primo_errore(&annunci).expect("il salto fallito si annuncia");
    assert_eq!(codice, "fs.networkUnavailable");
    assert!(
        !superato,
        "l'errore è la risposta alla richiesta, non un annuncio di prima"
    );
    assert_eq!(
        posizione.track_id,
        Some(5),
        "l'errore non dice di quale brano è"
    );
    assert_eq!(posizione.ms, 1_000, "l'errore non porta il punto chiesto");
}

#[test]
fn l_errore_della_prima_lettura_dopo_suona_parla_del_brano_nuovo() {
    // Il salto riesce, e la rete cade alla prima lettura: fra il comando e
    // quella lettura nessun `aggiorna` aveva ancora scritto la posizione,
    // e l'annuncio portava quella vuota — niente da annotare per la ripresa.
    let (annunci, voce) = spia_annunci();
    let mut banco = banco_con_voce(48_000, 16_384, voce);
    let (sorgente, morta) = brano_con_interruttore(5, 48_000, 64);
    let brano = aperto(&banco.ctx, sorgente);
    chiedi_un_brano(&banco);
    banco.ctx.esegui(Comando::Suona {
        brano: Box::new(brano),
        da_ms: 1_000,
    });
    assert!(
        primo_errore(&annunci).is_none(),
        "con la rete viva il salto riesce"
    );
    morta.store(true, Ordering::Relaxed);
    let _ = banco.ctx.riempi();

    let (codice, superato, posizione) =
        primo_errore(&annunci).expect("la lettura fallita si annuncia");
    assert_eq!(codice, "fs.networkUnavailable");
    assert!(!superato);
    assert_eq!(
        posizione.track_id,
        Some(5),
        "l'errore non dice di quale brano è"
    );
    assert_eq!(posizione.ms, 1_000, "l'errore non porta il punto chiesto");
}

#[test]
fn un_salto_con_la_callback_in_ritardo_non_lascia_debiti() {
    // In pausa il flusso riposa — un secondo sul Bluetooth — e il salto
    // che arriva lo trova fermo: la stretta di mano dello svuotamento
    // scade dopo un decimo di secondo. Il filo azzerava i conti e
    // spingeva il punto nuovo; la rete di sicurezza del filo dell'uscita
    // risvegliava il flusso poco dopo, e la callback svuotava **tutto**,
    // punto nuovo compreso, senza contarlo. Da lì i fotogrammi spinti
    // restavano più di quelli suonati per sempre, e a fine coda il
    // `Fermato` non arrivava mai.
    let (annunci, voce) = spia_annunci();
    let mut banco = banco_con_voce(48_000, 16_384, voce);
    // La stretta di mano da qui la fa la callback finta qui sotto.
    banco.stretta.ferma();
    let primo = aperto(&banco.ctx, brano_onesto(1, 48_000, 0.5, 2_000));
    let Banco {
        mut ctx,
        mut consumatore,
        condiviso,
        _manda,
        _curve,
        stretta: _stretta,
    } = banco;

    // La callback finta: conta come la vera, tace in pausa, e a flusso
    // acceso risponde subito allo svuotamento. A riposo non gira; se
    // intanto qualcuno aspetta uno svuotamento, la rete di sicurezza la
    // risveglia dopo trecento millisecondi.
    let riposa = Arc::new(AtomicBool::new(false));
    let basta = Arc::new(AtomicBool::new(false));
    let casse = {
        let (riposa, basta, condiviso) = (
            Arc::clone(&riposa),
            Arc::clone(&basta),
            Arc::clone(&condiviso),
        );
        std::thread::spawn(move || {
            let mut aspetta_da: Option<Instant> = None;
            while !basta.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(1));
                if riposa.load(Ordering::Acquire) {
                    if !condiviso.svuota.load(Ordering::Acquire) {
                        continue;
                    }
                    let da = *aspetta_da.get_or_insert_with(Instant::now);
                    if da.elapsed() < Duration::from_millis(300) {
                        continue;
                    }
                    aspetta_da = None;
                    riposa.store(false, Ordering::Release);
                }
                if condiviso.svuota.load(Ordering::Acquire) {
                    while consumatore.pop().is_ok() {}
                    condiviso.svuota.store(false, Ordering::Release);
                    continue;
                }
                if condiviso.in_pausa.load(Ordering::Acquire) {
                    continue;
                }
                let mut presi = 0u64;
                while presi < BLOCCO_FINTO && consumatore.pop().is_ok() {
                    presi += 1;
                }
                condiviso.fotogrammi.fetch_add(presi, Ordering::Relaxed);
            }
        })
    };
    let fermato = |annunci: &Annunci| {
        annunci
            .lock()
            .is_ok_and(|elenco| elenco.iter().any(|a| matches!(a.evento, Evento::Fermato)))
    };
    let gira_per = |ctx: &mut Contesto, quanto: Duration| {
        let fine = Instant::now() + quanto;
        while Instant::now() < fine && !fermato(&annunci) {
            let _ = ctx.riempi();
            ctx.aggiorna();
            std::thread::sleep(Duration::from_millis(1));
        }
    };

    ctx.intese.chieste.fetch_add(1, Ordering::SeqCst);
    ctx.intese.pausa_voluta.store(false, Ordering::SeqCst);
    ctx.esegui(suona(primo));
    gira_per(&mut ctx, Duration::from_millis(150));

    // Pausa, e il flusso va a riposo.
    ctx.intese.pausa(&condiviso);
    std::thread::sleep(Duration::from_millis(20));
    riposa.store(true, Ordering::Release);
    ctx.esegui(Comando::VaiA(500));
    gira_per(&mut ctx, Duration::from_millis(500));
    ctx.intese.riprendi(&condiviso);
    gira_per(&mut ctx, Duration::from_secs(10));

    basta.store(true, Ordering::Release);
    let _ = casse.join();
    assert!(
        fermato(&annunci),
        "a brano finito il `Fermato` non è arrivato: spinti {}, suonati {}",
        ctx.spinti,
        condiviso.fotogrammi.load(Ordering::Relaxed)
    );
    assert_eq!(
        condiviso.fotogrammi.load(Ordering::Relaxed),
        ctx.spinti,
        "fra spinti e suonati è rimasto un debito"
    );
}

// ── la pausa e il brano nuovo ───────────────────────────────────────────

/// Quel che fa `Motore::suona` dalla parte della maniglia, senza maniglia.
fn chiedi_un_brano(banco: &Banco) {
    banco.ctx.intese.chieste.fetch_add(1, Ordering::SeqCst);
    banco.ctx.intese.pausa_voluta.store(false, Ordering::SeqCst);
}

#[test]
fn un_brano_scelto_in_pausa_non_fa_sentire_i_resti_di_quello_prima() {
    // Il difetto: `Motore::suona` abbassava subito la pausa in corso, mentre
    // il comando che butta l'anello era ancora in coda. Per qualche
    // millisecondo la callback suonava i resti del brano messo in pausa.
    let mut banco = banco(48_000);
    let primo = aperto(&banco.ctx, brano(1, 48_000, 0.8));
    chiedi_un_brano(&banco);
    banco.ctx.esegui(suona(primo));
    let _ = un_giro(&mut banco);
    banco.ctx.intese.pausa(&banco.condiviso);

    chiedi_un_brano(&banco);
    assert!(
        banco.condiviso.in_pausa.load(Ordering::SeqCst),
        "la pausa in corso si è abbassata prima che l'anello fosse buttato"
    );
    assert!(
        !istantanea(&banco.ctx.posizione, &banco.ctx.intese).in_pausa,
        "chi chiede dove siamo deve sentirsi dire quel che ha appena chiesto"
    );
    // Una ripresa premuta nel frattempo non cambia niente: la svolta è in
    // viaggio, e la pausa in corso la abbassa chi la esegue.
    banco.ctx.intese.riprendi(&banco.condiviso);
    assert!(banco.condiviso.in_pausa.load(Ordering::SeqCst));

    let secondo = aperto(&banco.ctx, brano(2, 48_000, 0.4));
    banco.ctx.esegui(suona(secondo));
    assert!(
        !banco.condiviso.in_pausa.load(Ordering::SeqCst),
        "svuotato l'anello, il brano nuovo deve suonare"
    );
}

#[test]
fn una_pausa_premuta_subito_dopo_un_brano_nuovo_vince() {
    // L'ordine che `Motore::pausa` promette: pausa, ripresa e avvio valgono
    // nell'ordine in cui si premono. Un avvio in coda non si mangia una
    // pausa premuta dopo.
    let mut banco = banco(48_000);
    let primo = aperto(&banco.ctx, brano(1, 48_000, 0.8));
    chiedi_un_brano(&banco);
    banco.ctx.intese.pausa(&banco.condiviso);
    banco.ctx.esegui(suona(primo));
    assert!(banco.condiviso.in_pausa.load(Ordering::SeqCst));
    assert!(istantanea(&banco.ctx.posizione, &banco.ctx.intese).in_pausa);
}

// ── il salto a metà di una dissolvenza ──────────────────────────────────

/// Tre brani con una dissolvenza da seicento millisecondi, su un anello
/// abbastanza corto da far cadere l'annuncio di metà curva **mentre** i
/// due brani si stanno ancora mescolando. `seconda_meta` sceglie l'istante
/// del salto: dopo l'annuncio del brano che entra, o prima.
///
/// Chi sta sopra fa quel che fa l'applicazione: all'annuncio del secondo
/// brano mette in canna il terzo.
fn salto_nella_dissolvenza(seconda_meta: bool) -> (Vec<String>, Vec<Posizione>) {
    let (registro, osservatore) = spia();
    // Un decimo di secondo d'anello: la decodifica sta avanti di così poco
    // che a metà curva l'entrante è ancora in mano alla dissolvenza.
    let mut banco = banco_con_anello(48_000, 4_800, osservatore);
    banco.ctx.esegui(Comando::Dissolvenza { ms: 600 });
    let primo = aperto(&banco.ctx, brano(1, 48_000, 0.8));
    banco.ctx.esegui(suona(primo));
    let secondo = aperto(&banco.ctx, brano(2, 48_000, 0.4));
    banco.ctx.esegui(Comando::Prepara(Some(Box::new(secondo))));

    let mut storia = Vec::new();
    let mut saltato = false;
    let mut terzo_consegnato = false;
    for _ in 0..200_000 {
        let (posizione, mosso) = un_giro(&mut banco);
        storia.push(posizione);
        let annunciato = eventi(&registro).iter().any(|e| e == "iniziato:2");
        if annunciato && !terzo_consegnato {
            let terzo = aperto(&banco.ctx, brano(3, 48_000, 0.1));
            banco.ctx.esegui(Comando::Prepara(Some(Box::new(terzo))));
            terzo_consegnato = true;
        }
        if !saltato
            && banco.ctx.entrante.is_some()
            && banco.ctx.dissolvenza_fatti > 0
            && annunciato == seconda_meta
        {
            banco.ctx.esegui(Comando::VaiA(100));
            saltato = true;
        }
        if !mosso && banco.ctx.corrente.is_none() && banco.ctx.fine_dichiarata {
            break;
        }
    }
    assert!(saltato, "il banco non arriva mai nella finestra del salto");
    (eventi(&registro), storia)
}

#[test]
fn un_salto_nella_seconda_meta_della_dissolvenza_non_perde_il_brano_dopo() {
    // Il difetto: a metà curva il brano che entra si annuncia, chi sta
    // sopra mette in canna il successivo, e un salto subito dopo riavvolgeva
    // il brano che entrava e lo riscriveva in canna **sopra** il successivo.
    // Il salto finiva sul brano che usciva, mentre la finestra mostrava
    // l'altro; il successivo era perso, e dopo il secondo brano la musica si
    // fermava.
    let (annunci, storia) = salto_nella_dissolvenza(true);
    assert_eq!(
        annunci,
        ["iniziato:1", "iniziato:2", "iniziato:3", "fermato"],
        "la coda non è arrivata al terzo brano"
    );
    // Il salto è caduto sul brano che si sentiva, cioè sul secondo: dalla
    // metà curva — trecento millisecondi — torna a cento.
    let tornato = storia.windows(2).any(|due| {
        due[0].track_id == Some(2)
            && due[1].track_id == Some(2)
            && due[0].ms > due[1].ms.saturating_add(100)
            && (80..=250).contains(&due[1].ms)
    });
    assert!(
        tornato,
        "dopo il salto il secondo brano non riparte da ~100 ms: è saltato il brano che usciva"
    );
    // E da quando il secondo si sente, il primo non torna più.
    let dal_secondo = storia
        .iter()
        .position(|p| p.track_id == Some(2))
        .unwrap_or(storia.len());
    let del_primo_dopo = storia
        .get(dal_secondo..)
        .unwrap_or_default()
        .iter()
        .any(|p| p.track_id == Some(1));
    assert!(
        !del_primo_dopo,
        "dopo il salto si racconta di nuovo il brano che usciva"
    );
}

#[test]
fn un_salto_nella_prima_meta_della_dissolvenza_torna_al_brano_che_esce() {
    // Il contrappeso: prima di metà curva si sente soprattutto il brano che
    // esce, la finestra mostra ancora lui, e il salto è per lui. Quello che
    // entrava torna in canna dal suo inizio, e arriva dopo come se niente
    // fosse.
    let (annunci, storia) = salto_nella_dissolvenza(false);
    assert_eq!(
        annunci,
        ["iniziato:1", "iniziato:2", "iniziato:3", "fermato"],
        "un salto nella prima metà ha disordinato la coda"
    );
    let del_primo: Vec<u64> = storia
        .iter()
        .filter(|p| p.track_id == Some(1))
        .map(|p| p.ms)
        .collect();
    let risalito = del_primo
        .windows(2)
        .any(|due| due[0] > due[1].saturating_add(100) && (80..=250).contains(&due[1]));
    assert!(
        risalito,
        "il brano che usciva non è tornato a ~100 ms: il salto è finito altrove"
    );
}

// ── ripeti uno ──────────────────────────────────────────────────────────

#[test]
fn un_salto_negli_ultimi_secondi_di_ripeti_uno_non_ferma_la_musica() {
    // Il difetto: negli ultimi secondi di ogni ripetizione il decodificatore
    // è già sulla ripetizione dopo, e il confronto per identificativo le
    // dava per lo stesso brano. Il salto cadeva sulla ripetizione successiva
    // col numero d'ascolto di quella in corso: la successiva non si
    // annunciava mai, chi sta sopra non preparava la terza, e la musica si
    // fermava.
    let (registro, osservatore) = spia();
    let mut banco = banco_con_anello(
        48_000,
        usize::try_from(fotogrammi_da_ms(1_000, 48_000)).unwrap_or(48_000),
        osservatore,
    );
    let primo = aperto(&banco.ctx, brano_onesto(7, 48_000, 0.8, 800));
    banco.ctx.esegui(suona(primo));
    // Come chi sta sopra con «ripeti uno»: il successivo è lo stesso brano,
    // consegnato alla partenza e poi a ogni annuncio.
    let di_nuovo = aperto(&banco.ctx, brano_onesto(7, 48_000, 0.8, 800));
    banco.ctx.esegui(Comando::Prepara(Some(Box::new(di_nuovo))));
    let mut consegnati = 2usize;

    let mut saltato = false;
    let mut storia = Vec::new();
    for _ in 0..400_000 {
        let annunci = quanti_eventi(&registro);
        if annunci >= consegnati && consegnati < 3 && banco.ctx.prossimo.is_none() {
            let ancora = aperto(&banco.ctx, brano_onesto(7, 48_000, 0.8, 800));
            banco.ctx.esegui(Comando::Prepara(Some(Box::new(ancora))));
            consegnati += 1;
        }
        let (posizione, mosso) = un_giro(&mut banco);
        // La finestra: si sente la prima ripetizione, la decodifica è già
        // sulla seconda.
        if !saltato
            && annunci == 1
            && posizione.ms > 400
            && banco.ctx.uscente.is_some()
            && banco.ctx.corrente.is_some()
        {
            banco.ctx.esegui(Comando::VaiA(100));
            saltato = true;
        }
        storia.push((posizione, quanti_eventi(&registro)));
        if !mosso && banco.ctx.corrente.is_none() && banco.ctx.fine_dichiarata {
            break;
        }
    }

    assert!(saltato, "la finestra del difetto non si è mai aperta");
    assert_eq!(
        eventi(&registro),
        ["iniziato:7", "iniziato:7", "iniziato:7", "fermato"],
        "dopo il salto le ripetizioni non vanno avanti"
    );
    let risalito = storia
        .windows(2)
        .any(|due| due[0].0.ms > 400 && (50..=300).contains(&due[1].0.ms));
    assert!(risalito, "il salto non ha riportato la puntina a ~100 ms");
    // E il salto è caduto sulla ripetizione che si **sentiva**: la seconda
    // comincia dopo, dal suo inizio. Se il salto fosse caduto sulla seconda
    // — il decodificatore che correva avanti — lei si annuncerebbe subito,
    // a cento millisecondi, e la prima sarebbe sparita a metà.
    let seconda = storia
        .iter()
        .find(|(_, annunci)| *annunci >= 2)
        .map(|(posizione, _)| posizione.ms)
        .expect("la seconda ripetizione non si annuncia mai");
    assert!(
        seconda < 50,
        "la seconda ripetizione si annuncia a {seconda} ms: il salto l'ha portata via"
    );
}

// ── la sfumatura del timer di spegnimento ───────────────────────────────

#[test]
fn la_sfumatura_scende_in_decibel_e_si_ferma_in_fondo() {
    let durata = Duration::from_secs(4);
    assert!((fattore_di_sfumatura(Duration::ZERO, durata) - 1.0).abs() < 1e-6);
    // A metà strada, metà dei decibel: −30 dB.
    let meta = fattore_di_sfumatura(Duration::from_secs(2), durata);
    assert!(
        (meta - 10.0f32.powf(-1.5)).abs() < 1e-4,
        "a metà vale {meta}"
    );
    let fondo = fattore_di_sfumatura(durata, durata);
    assert!((fondo - 0.001).abs() < 1e-6, "in fondo vale {fondo}");
    // Oltre la fine non risale.
    let oltre = fattore_di_sfumatura(Duration::from_secs(9), durata);
    assert!((oltre - fondo).abs() < f32::EPSILON);
}

#[test]
fn la_sfumatura_finisce_in_pausa_e_rimette_il_volume_alla_ripresa() {
    // La fine del timer: il volume scende, la musica si ferma **al fondo**
    // — la porta della callback si chiude sul quasi-silenzio, non su un
    // volume che risale — e il volume torna dov'era alla ripresa, così chi
    // riprende domani non trova il cursore a zero.
    let mut banco = banco(48_000);
    banco.ctx.esegui(Comando::Volume {
        volume: 0.5,
        muto: false,
    });
    let primo = aperto(&banco.ctx, brano(1, 48_000, 0.8));
    chiedi_un_brano(&banco);
    banco.ctx.esegui(suona(primo));
    banco.ctx.esegui(Comando::Sfuma { ms: 80 });

    std::thread::sleep(Duration::from_millis(40));
    banco.ctx.avanza_sfumatura();
    let a_meta = f32::from_bits(banco.condiviso.guadagno.load(Ordering::Relaxed));
    assert!(
        a_meta > 0.0 && a_meta < 0.25,
        "a metà sfumatura il guadagno è {a_meta}"
    );
    assert!(!banco.condiviso.in_pausa.load(Ordering::SeqCst));

    std::thread::sleep(Duration::from_millis(60));
    banco.ctx.avanza_sfumatura();
    assert!(
        banco.condiviso.in_pausa.load(Ordering::SeqCst),
        "finita la sfumatura la musica deve essere in pausa"
    );
    assert!(istantanea(&banco.ctx.posizione, &banco.ctx.intese).in_pausa);
    // In pausa il guadagno resta al fondo: rimetterlo subito era il «blip»
    // mentre la porta si chiudeva.
    let in_pausa = f32::from_bits(banco.condiviso.guadagno.load(Ordering::Relaxed));
    assert!(
        in_pausa < volume_uscita(0.5, false) * 0.01,
        "a pausa appena messa il guadagno è già risalito: {in_pausa}"
    );
    // E resta lì ai giri dopo, senza rimettere la pausa a ogni passo.
    std::thread::sleep(Duration::from_millis(5));
    banco.ctx.avanza_sfumatura();
    let ancora = f32::from_bits(banco.condiviso.guadagno.load(Ordering::Relaxed));
    assert!((ancora - in_pausa).abs() < 1e-6);

    // La ripresa la toglie: dov'era vuol dire il cursore a metà, che con
    // la curva percepita è un ottavo dell'ampiezza e non la metà.
    banco.ctx.esegui(Comando::AnnullaSfuma);
    let rimesso = f32::from_bits(banco.condiviso.guadagno.load(Ordering::Relaxed));
    assert!(
        (rimesso - volume_uscita(0.5, false)).abs() < 1e-6,
        "il volume non è tornato dov'era: {rimesso}"
    );
}

#[test]
fn un_brano_scelto_durante_la_sfumatura_la_annulla() {
    let mut banco = banco(48_000);
    let primo = aperto(&banco.ctx, brano(1, 48_000, 0.8));
    chiedi_un_brano(&banco);
    banco.ctx.esegui(suona(primo));
    banco.ctx.esegui(Comando::Sfuma { ms: 60_000 });
    std::thread::sleep(Duration::from_millis(5));
    banco.ctx.avanza_sfumatura();

    let secondo = aperto(&banco.ctx, brano(2, 48_000, 0.4));
    chiedi_un_brano(&banco);
    banco.ctx.esegui(suona(secondo));
    banco.ctx.avanza_sfumatura();
    assert!(
        banco.ctx.sfumatura.is_none(),
        "la sfumatura è sopravvissuta"
    );
    let guadagno = f32::from_bits(banco.condiviso.guadagno.load(Ordering::Relaxed));
    assert!(
        (guadagno - volume_uscita(0.8, false)).abs() < 1e-6,
        "il brano nuovo parte col volume sfumato: {guadagno}"
    );
}

// ── il tempo condiviso: epoca, mappa, etichette ─────────────────────────

/// La mappa com'è pubblicata adesso.
fn mappa_di(banco: &Banco) -> Mappa {
    banco
        .ctx
        .mappa
        .lock()
        .map_or_else(|avvelenata| avvelenata.into_inner().clone(), |m| m.clone())
}

/// Un'ancora grezza del fotogramma `contenuto`, in un'epoca.
fn grezza(contenuto: u64, epoca: u32) -> crate::ancora::AncoraGrezza {
    crate::ancora::AncoraGrezza {
        contenuto,
        udito_ns: 1_000_000_000,
        epoca,
        in_corso: true,
        ..crate::ancora::AncoraGrezza::default()
    }
}

/// Un giro con la callback finta: vedi [`un_giro`].
fn gira(banco: &mut Banco) -> bool {
    un_giro(banco).1
}

#[test]
fn l_epoca_cresce_a_ogni_svuotamento() {
    // Ogni volta che il conteggio dei fotogrammi ricomincia da zero l'epoca
    // cresce: è quel che impedisce a un'ancora contata prima di un salto di
    // essere letta coi segni di dopo.
    let mut banco = banco(48_000);
    assert_eq!(banco.condiviso.epoca.load(Ordering::Acquire), 0);
    let primo = aperto(&banco.ctx, brano(1, 48_000, 0.5));
    banco.ctx.esegui(suona(primo));
    assert_eq!(banco.condiviso.epoca.load(Ordering::Acquire), 1);
    let _ = gira(&mut banco);
    assert_eq!(mappa_di(&banco).epoca, 1, "la mappa porta la sua epoca");
    banco.ctx.esegui(Comando::VaiA(500));
    assert_eq!(banco.condiviso.epoca.load(Ordering::Acquire), 2);
    banco.ctx.esegui(Comando::Ferma);
    assert_eq!(banco.condiviso.epoca.load(Ordering::Acquire), 3);
    let _ = gira(&mut banco);
    assert_eq!(mappa_di(&banco).epoca, 3);
    assert!(mappa_di(&banco).segni.is_empty(), "fermo, nessun segno");
}

#[test]
fn dopo_un_salto_il_segno_e_al_fotogramma_esatto() {
    let mut banco = banco(48_000);
    let primo = aperto(&banco.ctx, brano(1, 48_000, 0.5));
    banco.ctx.esegui(suona(primo));
    let _ = gira(&mut banco);
    banco.ctx.esegui(Comando::VaiA(250));
    let segno = banco.ctx.segni.back().map(Segno::pubblico);
    assert_eq!(
        segno.map(|s| (s.da, s.offset_fotogrammi)),
        Some((0, 12_000))
    );
    // Il primo blocco conferma dove si è atterrati, al fotogramma.
    assert!(banco.ctx.riempi());
    let segno = banco.ctx.segni.back().map(Segno::pubblico);
    assert_eq!(segno.map(|s| s.offset_fotogrammi), Some(12_000));
    banco.ctx.aggiorna();
    let mappa = mappa_di(&banco);
    let ancora = mappa
        .traduci(&grezza(0, banco.condiviso.epoca.load(Ordering::Acquire)))
        .expect("il fotogramma zero dell'epoca è il punto del salto");
    assert_eq!(ancora.posizione_us, 250_000);
}

#[test]
fn nel_gapless_con_rinvio_il_segno_e_al_fotogramma_esatto() {
    // Due brani a 44,1 kHz su un'uscita a 48: la coda del primo passa col
    // ricampionatore e uscirà davanti al secondo. Il segno del secondo sta
    // dopo di lei, al fotogramma esatto in cui il primo finisce — un
    // secondo a 48 kHz — e dal suo inizio.
    let mut banco = due_brani_a_44(Box::new(|_| {}));
    let mut visto = None;
    for _ in 0..10_000 {
        let mosso = gira(&mut banco);
        if let Some(secondo) = banco.ctx.segni.iter().find(|s| s.track_id == 2) {
            visto = Some(secondo.pubblico());
            break;
        }
        if !mosso {
            break;
        }
    }
    let secondo = visto.expect("il secondo brano non ha mai avuto un segno");
    assert_eq!(secondo.offset_fotogrammi, 0);
    assert_eq!(secondo.da, 48_000, "il segno non è dove il primo finisce");
}

#[test]
fn la_posizione_somma_i_fotogrammi_prima_di_troncare() {
    // Il cambiamento voluto della Fase 0. Quarantasette fotogrammi di salto
    // e quarantasette di musica a 48 kHz sono quasi due millisecondi: prima
    // ognuno dei due addendi si troncava per conto suo e la posizione
    // diceva zero, adesso la somma si fa in fotogrammi e dice uno.
    let mut banco = banco(48_000);
    banco.ctx.segni.push_back(Segno {
        da: 0,
        track_id: 9,
        durata_ms: 1_000,
        offset_fotogrammi: 47,
        etichetta: None,
        replaygain_db: None,
        ascolto: 1,
        formato: None,
    });
    banco.condiviso.fotogrammi.store(47, Ordering::Relaxed);
    banco.ctx.aggiorna();
    let posizione = istantanea(&banco.ctx.posizione, &banco.ctx.intese);
    assert_eq!(
        ms_da_fotogrammi(47, 48_000) + ms_da_fotogrammi(47, 48_000),
        0,
        "il conto di prima"
    );
    assert_eq!(posizione.ms, 1, "il conto di adesso");
}

/// Le coppie (brano, etichetta) che la mappa ha pubblicato suonando due
/// brani etichettati, con la dissolvenza data.
fn etichette_viste(dissolvenza_ms: u64) -> Vec<(i64, Option<u64>)> {
    let mut banco = banco(48_000);
    banco
        .ctx
        .esegui(Comando::Dissolvenza { ms: dissolvenza_ms });
    let primo = aperto(&banco.ctx, brano(1, 48_000, 0.8)).con_etichetta(11);
    banco.ctx.esegui(suona(primo));
    let secondo = aperto(&banco.ctx, brano(2, 48_000, 0.4)).con_etichetta(22);
    banco.ctx.esegui(Comando::Prepara(Some(Box::new(secondo))));
    let mut viste = Vec::new();
    for _ in 0..200_000 {
        let mosso = gira(&mut banco);
        for segno in &mappa_di(&banco).segni {
            if !viste.contains(&(segno.track_id, segno.etichetta)) {
                viste.push((segno.track_id, segno.etichetta));
            }
        }
        if !mosso && banco.ctx.corrente.is_none() {
            break;
        }
    }
    viste
}

#[test]
fn l_etichetta_segue_il_cambio_di_brano() {
    // Senza buco e con la dissolvenza: due strade diverse per annotare il
    // segno del brano dopo, e tutte e due devono portare la sua etichetta.
    for dissolvenza_ms in [0, 400] {
        assert_eq!(
            etichette_viste(dissolvenza_ms),
            [(1, Some(11)), (2, Some(22))],
            "con {dissolvenza_ms} ms di dissolvenza"
        );
    }
    // E un salto resta col brano che si sente.
    let mut banco = banco(48_000);
    let primo = aperto(&banco.ctx, brano(1, 48_000, 0.8)).con_etichetta(11);
    assert_eq!(primo.etichetta(), Some(11));
    banco.ctx.esegui(suona(primo));
    banco.ctx.esegui(Comando::VaiA(300));
    let _ = gira(&mut banco);
    let etichette: Vec<_> = mappa_di(&banco).segni.iter().map(|s| s.etichetta).collect();
    assert_eq!(etichette, [Some(11)]);
}

/// Un secondo di rampa a 48 kHz: ogni campione dice a che punto del brano
/// sta, in secondi.
fn brano_a_rampa(track_id: i64) -> Sorgente {
    let campioni: Vec<f32> = (0..48_000u16).map(|n| f32::from(n) / 48_000.0).collect();
    Sorgente {
        media: Box::new(Byte(std::io::Cursor::new(wav(48_000, &campioni)))),
        ..brano(track_id, 48_000, 0.0)
    }
}

#[test]
fn apri_fermo_non_fa_uscire_niente_dal_punto_zero() {
    // Il brano si apre in pausa e già a metà: il primo campione che entra
    // nell'anello è quello del mezzo secondo, e nessuno di prima.
    let mut banco = banco(48_000);
    let rampa = aperto(&banco.ctx, brano_a_rampa(5)).con_etichetta(3);
    banco.ctx.intese.chieste.fetch_add(1, Ordering::SeqCst);
    banco.ctx.intese.pausa(&banco.condiviso);
    banco.ctx.esegui(Comando::Suona {
        brano: Box::new(rampa),
        da_ms: 500,
    });
    assert!(banco.ctx.riempi());
    let primo = banco.consumatore.pop().expect("l'anello si è riempito");
    assert!(
        (primo - 0.5).abs() < 0.001,
        "il primo campione è a {primo} s"
    );
    let mut minimo = primo;
    while let Ok(campione) = banco.consumatore.pop() {
        minimo = minimo.min(campione);
    }
    assert!(
        minimo > 0.499,
        "un campione del punto zero è entrato: {minimo}"
    );
    assert!(
        banco.condiviso.in_pausa.load(Ordering::SeqCst),
        "aperto fermo, resta fermo"
    );
    // E chi aspetta sa da dove si ripartirà.
    banco.ctx.aggiorna();
    let epoca = banco.condiviso.epoca.load(Ordering::Acquire);
    assert_eq!(mappa_di(&banco).attesa(epoca, 0), Some((Some(3), 500_000)));
    assert_eq!(istantanea(&banco.ctx.posizione, &banco.ctx.intese).ms, 500);
}

#[test]
fn suona_da_parte_dal_punto_senza_passare_dal_punto_zero() {
    // La ripresa a metà di chi suona: `Motore::suona_da` manda lo stesso
    // comando con la pausa **non** voluta. Il primo campione è quello del
    // mezzo secondo, e dopo lo svuotamento la pausa in corso si abbassa.
    let mut banco = banco(48_000);
    let rampa = aperto(&banco.ctx, brano_a_rampa(5));
    banco.ctx.intese.chieste.fetch_add(1, Ordering::SeqCst);
    banco.ctx.intese.pausa_voluta.store(false, Ordering::SeqCst);
    banco.ctx.esegui(Comando::Suona {
        brano: Box::new(rampa),
        da_ms: 500,
    });
    assert!(banco.ctx.riempi());
    let primo = banco.consumatore.pop().expect("l'anello si è riempito");
    assert!(
        (primo - 0.5).abs() < 0.001,
        "il primo campione è a {primo} s"
    );
    let mut minimo = primo;
    while let Ok(campione) = banco.consumatore.pop() {
        minimo = minimo.min(campione);
    }
    assert!(
        minimo > 0.499,
        "un campione del punto zero è entrato: {minimo}"
    );
    assert!(
        !banco.condiviso.in_pausa.load(Ordering::SeqCst),
        "suonando, la pausa in corso si abbassa a svolta eseguita"
    );
}

#[test]
fn una_latenza_negativa_non_perde_il_brano_dell_ancora() {
    // Trecento millisecondi di latenza negativa: gli uditi corrono davanti
    // alla callback, e il segno del primo brano esce dalla fila mentre la
    // callback ne sta ancora consegnando gli ultimi fotogrammi. Un'ancora
    // di quei fotogrammi deve trovare ancora il primo brano.
    let mut banco = banco(48_000);
    banco.ctx.esegui(Comando::Latenza { ms: -300 });
    let primo = aperto(&banco.ctx, brano(1, 48_000, 0.5));
    banco.ctx.esegui(suona(primo));
    let secondo = aperto(&banco.ctx, brano(2, 48_000, 0.5));
    banco.ctx.esegui(Comando::Prepara(Some(Box::new(secondo))));
    let mut provata = false;
    for _ in 0..100_000 {
        let _ = gira(&mut banco);
        let contati = banco.condiviso.fotogrammi.load(Ordering::Relaxed);
        let tolto_il_primo = banco.ctx.tolto.is_some_and(|t| t.track_id == 1);
        if tolto_il_primo && contati < 48_000 {
            let mappa = mappa_di(&banco);
            assert!(mappa.segni.iter().all(|s| s.track_id == 2));
            let epoca = banco.condiviso.epoca.load(Ordering::Acquire);
            let ancora = mappa
                .traduci(&grezza(contati, epoca))
                .expect("l'ancora ha perso il suo brano");
            assert_eq!(ancora.track_id, 1);
            assert_eq!(
                ancora.posizione_us,
                crate::ancora::us_da_fotogrammi(contati, 48_000)
            );
            assert_eq!(ancora.udito_ns, 700_000_000, "latenza sommata col segno");
            provata = true;
            break;
        }
    }
    assert!(
        provata,
        "il primo segno non è mai uscito prima della callback"
    );
}

#[test]
fn un_epoca_diversa_non_si_traduce_e_dopo_il_salto_si_rimette_in_pari() {
    let mut banco = banco(48_000);
    let primo = aperto(&banco.ctx, brano(1, 48_000, 0.5));
    banco.ctx.esegui(suona(primo));
    let _ = gira(&mut banco);
    assert!(mappa_di(&banco).traduci(&grezza(10, 1)).is_some());
    assert_eq!(mappa_di(&banco).traduci(&grezza(10, 0)), None);

    // Il salto alza l'epoca subito; la mappa la segue al giro dopo. In
    // mezzo un'ancora nuova non trova niente, invece di trovare i segni
    // vecchi.
    banco.ctx.esegui(Comando::VaiA(500));
    assert_eq!(mappa_di(&banco).traduci(&grezza(10, 2)), None);
    let _ = gira(&mut banco);
    let ancora = mappa_di(&banco).traduci(&grezza(0, 2)).expect("in pari");
    assert_eq!(ancora.posizione_us, 500_000);
    assert_eq!(mappa_di(&banco).traduci(&grezza(10, 1)), None);
}

// ── le tracce di un'immagine CD ─────────────────────────────────────────

/// Un'immagine CD finta: un WAV mono con un livello per traccia, un
/// secondo — settantacinque fotogrammi CD — l'una. Il livello dice di
/// quale traccia è ogni campione che esce.
fn immagine(frequenza: u32, livelli: &[f32]) -> Vec<u8> {
    let per_traccia = usize::try_from(fotogrammi_da_ms(1_000, frequenza)).unwrap_or(48_000);
    let campioni: Vec<f32> = livelli
        .iter()
        .flat_map(|livello| std::iter::repeat_n(*livello, per_traccia))
        .collect();
    wav(frequenza, &campioni)
}

/// Un pezzo di quell'immagine, come lo apre l'applicazione.
fn pezzo(
    track_id: i64,
    disco: &[u8],
    inizio_cd: u32,
    fine_cd: Option<u32>,
    durata_ms: u64,
) -> Sorgente {
    Sorgente {
        track_id,
        media: Box::new(Byte(std::io::Cursor::new(disco.to_vec()))),
        estensione: Some("wav".to_owned()),
        durata_ms,
        replaygain_db: None,
        picco: None,
        segmento: Some(crate::Segmento {
            immagine: "disco.wav".to_owned(),
            inizio_cd,
            fine_cd,
        }),
    }
}

/// La traccia `numero` (da uno) di un'immagine di `tracce` tracce da un
/// secondo: l'ultima va fino alla fine del file, come in un `.cue`.
fn traccia(disco: &[u8], numero: u32, tracce: u32) -> Sorgente {
    let fine_cd = (numero < tracce).then_some(numero * 75);
    pezzo(i64::from(numero), disco, (numero - 1) * 75, fine_cd, 1_000)
}

/// Fa girare il contesto con la callback finta, come
/// [`suona_raccontando`], tenendo i campioni che escono: servono tutti e
/// due, il racconto e il suono.
fn suona_e_raccogli(banco: &mut Banco) -> Vec<f32> {
    let mut fuori = Vec::new();
    for _ in 0..200_000 {
        let lavorato = banco.ctx.riempi();
        let mut presi = 0u64;
        while presi < BLOCCO_FINTO {
            let Ok(campione) = banco.consumatore.pop() else {
                break;
            };
            fuori.push(campione);
            presi = presi.saturating_add(1);
        }
        banco
            .condiviso
            .fotogrammi
            .fetch_add(presi, Ordering::Relaxed);
        banco.ctx.aggiorna();
        if !lavorato && presi == 0 && banco.ctx.corrente.is_none() {
            break;
        }
    }
    fuori
}

/// Suona `prima` e poi `dopo`, con la dissolvenza data: quel che si è
/// sentito e quel che il motore ha annunciato.
fn due_tracce(dissolvenza_ms: u64, prima: Sorgente, dopo: Sorgente) -> (Vec<f32>, Vec<String>) {
    let (registro, osservatore) = spia();
    let mut banco = banco_con(48_000, osservatore);
    banco
        .ctx
        .esegui(Comando::Dissolvenza { ms: dissolvenza_ms });
    let prima = aperto(&banco.ctx, prima);
    banco.ctx.esegui(suona(prima));
    let dopo = aperto(&banco.ctx, dopo);
    banco.ctx.esegui(Comando::Prepara(Some(Box::new(dopo))));
    let fuori = suona_e_raccogli(&mut banco);
    (fuori, eventi(&registro))
}

#[test]
fn due_tracce_contigue_non_si_dissolvono() {
    // La dissolvenza è accesa, e fra la prima e la seconda traccia del
    // disco non deve succedere: il secondo esce attaccato al primo, al suo
    // livello, e la durata dichiarata è quella vera — così la dissolvenza,
    // se ci fosse, avrebbe tutto il tempo di cominciare.
    let disco = immagine(48_000, &[0.2, 0.4, 0.6]);
    let (fuori, annunci) = due_tracce(400, traccia(&disco, 1, 3), traccia(&disco, 2, 3));
    assert_eq!(annunci, ["iniziato:1", "iniziato:2", "fermato"]);
    assert_eq!(
        fuori.len(),
        96_000,
        "una dissolvenza di 400 ms avrebbe accorciato di 19 200 fotogrammi"
    );
    assert_eq!(quanti_a(&fuori[..48_000], 0.2), 48_000);
    assert_eq!(quanti_a(&fuori[48_000..], 0.4), 48_000);

    // Il contrappeso: la prima e la terza, che non sono contigue, nella
    // stessa immagine e con la stessa dissolvenza, si sovrappongono.
    let (fuori, annunci) = due_tracce(400, traccia(&disco, 1, 3), traccia(&disco, 3, 3));
    assert_eq!(annunci, ["iniziato:1", "iniziato:3", "fermato"]);
    assert!(
        fuori.len() < 96_000,
        "la prova non vede la dissolvenza: {} fotogrammi",
        fuori.len()
    );
    let mescolati = fuori.len() - quanti_a(&fuori, 0.2) - quanti_a(&fuori, 0.6);
    assert!(mescolati > 0, "nessun campione mescolato");
}

#[test]
fn due_tracce_contigue_ricampionate_non_hanno_giuntura() {
    // Il caso vero: un'immagine a 44,1 kHz su un'uscita a 48, con la
    // dissolvenza accesa. Il livello non si muove attraversando il confine,
    // e la durata è quella di due secondi esatti.
    let disco = immagine(44_100, &[0.5, 0.5]);
    let (fuori, annunci) = due_tracce(400, traccia(&disco, 1, 2), traccia(&disco, 2, 2));
    assert_eq!(annunci, ["iniziato:1", "iniziato:2", "fermato"]);
    assert_eq!(
        fuori.len(),
        96_000,
        "la coda del primo si è persa o doppiata"
    );
    let giuntura = fuori
        .get(46_000..50_000)
        .expect("l'uscita arriva a due secondi");
    let fuori_posto = giuntura
        .iter()
        .filter(|c| (**c - 0.5).abs() >= 0.01)
        .count();
    assert_eq!(
        fuori_posto, 0,
        "{fuori_posto} campioni fuori livello alla giuntura"
    );
}

#[test]
fn ripeti_uno_su_una_traccia_d_immagine() {
    // La traccia due, e poi di nuovo lei: non è contigua a sé stessa, la
    // seconda volta riparte dal suo inizio e non dalla traccia tre, e
    // finita la seconda volta la musica si ferma. È qui che la 3.0.2 ha
    // dovuto ritirare una correzione del gapless, che in ripeti-uno girava
    // all'infinito.
    let disco = immagine(48_000, &[0.2, 0.4, 0.6]);
    for dissolvenza_ms in [0, 400] {
        let (fuori, annunci) =
            due_tracce(dissolvenza_ms, traccia(&disco, 2, 3), traccia(&disco, 2, 3));
        assert_eq!(
            annunci,
            ["iniziato:2", "iniziato:2", "fermato"],
            "con {dissolvenza_ms} ms di dissolvenza"
        );
        assert_eq!(quanti_a(&fuori, 0.6), 0, "è uscita la traccia dopo");
        assert_eq!(quanti_a(&fuori, 0.2), 0, "è uscita la traccia prima");
        if dissolvenza_ms == 0 {
            assert_eq!(fuori.len(), 96_000);
            assert_eq!(quanti_a(&fuori, 0.4), 96_000);
        } else {
            assert!(fuori.len() < 96_000, "la ripetizione non si è dissolta");
        }
    }
}

#[test]
fn dall_ultima_traccia_alla_prima() {
    // L'ultima traccia va fino in fondo al file e la prima comincia da
    // capo: non sono contigue, e il passaggio è quello di due brani
    // qualunque — attaccati senza dissolvenza, sovrapposti con.
    let disco = immagine(48_000, &[0.2, 0.4, 0.6]);
    let (fuori, annunci) = due_tracce(0, traccia(&disco, 3, 3), traccia(&disco, 1, 3));
    assert_eq!(annunci, ["iniziato:3", "iniziato:1", "fermato"]);
    assert_eq!(fuori.len(), 96_000);
    assert_eq!(quanti_a(&fuori[..48_000], 0.6), 48_000);
    assert_eq!(quanti_a(&fuori[48_000..], 0.2), 48_000);

    let (fuori, annunci) = due_tracce(400, traccia(&disco, 3, 3), traccia(&disco, 1, 3));
    assert_eq!(annunci, ["iniziato:3", "iniziato:1", "fermato"]);
    assert!(fuori.len() < 96_000, "dall'ultima alla prima si dissolve");
}

#[test]
fn ripeti_uno_su_una_traccia_vuota_d_immagine_non_gira_in_tondo() {
    // Come `ripeti_uno_su_un_brano_vuoto_non_gira_in_tondo`, con un foglio
    // che descrive una traccia oltre la fine del file: si apre, non
    // contiene niente, e chi sta sopra la ripropone a ogni annuncio. La
    // guardia è la stessa, e deve valere anche qui.
    let disco = immagine(48_000, &[0.2, 0.4, 0.6]);
    let (registro, osservatore) = spia();
    let mut banco = banco_con(48_000, osservatore);
    let vuota = aperto(&banco.ctx, pezzo(7, &disco, 300, None, 0));
    banco.ctx.esegui(suona(vuota));
    let mut visti = 0usize;
    let mut fra_quanti: Option<u32> = None;
    for _ in 0..2_000 {
        let iniziati = eventi(&registro)
            .iter()
            .filter(|e| e.as_str() == "iniziato:7")
            .count();
        if iniziati > visti && fra_quanti.is_none() {
            visti = iniziati;
            fra_quanti = Some(3);
        }
        if let Some(n) = fra_quanti.as_mut() {
            if *n == 0 {
                let ancora = aperto(&banco.ctx, pezzo(7, &disco, 300, None, 0));
                banco.ctx.esegui(Comando::Prepara(Some(Box::new(ancora))));
                fra_quanti = None;
            } else {
                *n -= 1;
            }
        }
        banco.ctx.riempi();
        let mut presi = 0u64;
        while presi < BLOCCO_FINTO {
            if banco.consumatore.pop().is_err() {
                break;
            }
            presi = presi.saturating_add(1);
        }
        banco
            .condiviso
            .fotogrammi
            .fetch_add(presi, Ordering::Relaxed);
        banco.ctx.aggiorna();
    }
    let iniziati = eventi(&registro)
        .iter()
        .filter(|e| e.as_str() == "iniziato:7")
        .count();
    assert!(
        iniziati <= 2,
        "{iniziati} inizi in 2000 giri: gira in tondo"
    );
}
