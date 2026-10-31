//! Le prove dell'uscita: la callback e le sue primitive di tempo, il riposo
//! del flusso, il campanello, gli errori del dispositivo e il ponte.

use super::regia::{Cade, dove_cade};
use super::*;
use crate::equalizzatore::{BANDE, LIMITE_DB};

/// Una frequenza a cui la porta si chiude in un fotogramma solo.
///
/// Le prove che parlano d'altro — il conto dei fotogrammi, i filtri, lo
/// svuotamento — non devono attraversare una dissolvenza per arrivare al
/// punto: a cento fotogrammi al secondo dodici millisecondi sono un
/// fotogramma, e la porta si comporta come l'interruttore di prima.
const SECCA: u32 = 100;

/// Una frequenza a cui la porta si chiude in dodici fotogrammi: abbastanza
/// per guardarla muoversi, poco abbastanza per stare in un blocco di prova.
const LENTA: u32 = 1_000;

/// Il battito delle prove che non parlano di tempo: nessuna frequenza,
/// quindi nessuna ancora e nessun avvio programmato. Il giro che ne esce è
/// quello di sempre.
const SENZA_OROLOGIO: Battito = Battito {
    playback_ns: 0,
    anticipo_ns: 0,
    frequenza: 0,
    ingresso_ns: 0,
};

/// Le caselle condivise di un motore che sta suonando, con tanti canali.
///
/// Da `Condiviso::nuovo` e non scritte a mano: le caselle crescono, e una
/// copia del valore di riposo qui dentro sarebbe la seconda verità da
/// tenere in pari con la prima.
fn condiviso(canali: u16) -> Arc<Condiviso> {
    let condiviso = Condiviso::nuovo();
    condiviso.in_pausa.store(false, Ordering::Release);
    condiviso.canali.store(u32::from(canali), Ordering::Relaxed);
    condiviso
}

/// La voce di una callback che sta già suonando: porta aperta e volume al
/// voluto, così le prove partono dal regime e non da una salita.
///
/// Restituisce anche la maniglia per mandarle delle curve. La spia non
/// guarda: le prove di questo file parlano della callback, non dello
/// spettro, e il suo anello nessuno lo legge.
fn voce(lettore: rtrb::Consumer<f32>, frequenza: u32) -> (rtrb::Producer<Coefficienti>, Voce) {
    let (manda, ricevi) = rtrb::RingBuffer::<Coefficienti>::new(4);
    let (spia, _tenuto) = rtrb::RingBuffer::<f32>::new(8);
    let mut andamento = Andamento::nuovo(frequenza);
    andamento.guadagno = 1.0;
    andamento.porta = andamento.corsa;
    (
        manda,
        Voce {
            lettore,
            andamento,
            filtro: Filtro {
                stato: StatoEq::nuovo(),
                ricevi,
            },
            spia: Spia {
                manda: spia,
                somma: 0.0,
                quanti: 0,
            },
            veglia: Arc::new(Veglia::nuova()),
        },
    )
}

/// Un anello con `quanti` campioni tutti uguali dentro.
fn anello(capacita: usize, quanti: usize, valore: f32) -> rtrb::Consumer<f32> {
    let (mut scrittore, lettore) = rtrb::RingBuffer::<f32>::new(capacita);
    for _ in 0..quanti {
        let _ = scrittore.push(valore);
    }
    lettore
}

/// L'aritmetica della latenza, che è l'unica metà provabile.
///
/// [`annota_latenza`] non ha una prova sua, e non è una dimenticanza:
/// `cpal::OutputCallbackInfo` lo costruisce il backend audio con gli
/// istanti del suo orologio. Provarla vorrebbe dire un dispositivo vero,
/// cioè una prova che sulla CI non gira. Quel che si poteva separare è
/// questa conversione; quel che resta nella funzione è una lettura di due
/// campi `Copy` e uno `store`.
#[test]
fn la_latenza_si_converte_in_fotogrammi() {
    // Il buffer tipico di WASAPI in modalità condivisa: dieci millisecondi,
    // che a 48 kHz sono 480 fotogrammi.
    assert_eq!(fotogrammi_di(Duration::from_millis(10), 48_000), 480);
    assert_eq!(fotogrammi_di(Duration::ZERO, 48_000), 0);
    // E il resto si perde verso il basso: mezzo fotogramma non è un
    // fotogramma.
    assert_eq!(fotogrammi_di(Duration::from_micros(10), 48_000), 0);
}

// ── la porta e la rampa ──

#[test]
fn la_porta_dura_una_dozzina_di_millisecondi() {
    // A 48 kHz dodici millisecondi sono 576 fotogrammi, a 44,1 kHz 529.
    assert_eq!(corsa_della_porta(48_000), 576);
    assert_eq!(corsa_della_porta(44_100), 529);
    // E almeno un fotogramma anche dove dodici millisecondi non ne fanno
    // nemmeno uno: una porta di zero fotogrammi sarebbe il gradino di prima.
    assert_eq!(corsa_della_porta(10), 1);
    assert_eq!(corsa_della_porta(0), 1);
}

#[test]
fn la_porta_si_chiude_e_si_riapre_in_una_corsa_esatta() {
    let mut andamento = Andamento::nuovo(48_000);
    andamento.porta = andamento.corsa;
    assert_eq!(andamento.apertura().to_bits(), 1.0f32.to_bits());
    let mut precedente = andamento.apertura();
    let mut passi = 0;
    while andamento.porta > 0 {
        andamento.muovi_porta(false);
        passi += 1;
        let adesso = andamento.apertura();
        assert!(adesso < precedente, "la porta non scende: {adesso}");
        precedente = adesso;
    }
    // Zero esatto, non un residuo: una porta «quasi chiusa» moltiplicata
    // per un campione darebbe un numero piccolissimo invece del silenzio.
    assert_eq!(passi, 576);
    assert_eq!(andamento.apertura().to_bits(), 0.0f32.to_bits());
    andamento.muovi_porta(false);
    assert_eq!(andamento.porta, 0, "chiusa resta chiusa");
    // E all'insù lo stesso: uno esatto, e lì si ferma.
    for _ in 0..1_000 {
        andamento.muovi_porta(true);
    }
    assert_eq!(andamento.porta, andamento.corsa);
    assert_eq!(andamento.apertura().to_bits(), 1.0f32.to_bits());
}

#[test]
fn la_rampa_verso_lo_zero_non_finisce_fra_i_subnormali() {
    // Il difetto che questa prova impedisce: `g += (0 − g) × 0,0005` non
    // arriva mai a zero, e dopo qualche decina di migliaia di campioni il
    // guadagno diventava subnormale — cioè decine di volte più caro da
    // moltiplicare, dentro la callback in tempo reale.
    let mut guadagno = 1.0f32;
    for _ in 0..1_000_000 {
        guadagno = avvicina(guadagno, 0.0);
        assert!(!guadagno.is_subnormal(), "subnormale: {guadagno:e}");
    }
    assert_eq!(guadagno.to_bits(), 0.0f32.to_bits(), "doveva agganciarsi");
}

#[test]
fn la_rampa_si_aggancia_anche_all_insu() {
    let mut guadagno = 0.0f32;
    for _ in 0..1_000_000 {
        guadagno = avvicina(guadagno, 0.7);
    }
    assert_eq!(guadagno.to_bits(), 0.7f32.to_bits());
    // E un passo solo, lontano dal bersaglio, resta un passo di rampa.
    let uno = avvicina(0.0, 1.0);
    assert!((uno - RAMPA).abs() < f32::EPSILON, "{uno}");
}

// ── pausa, ripresa e salto ──

#[test]
fn a_porta_chiusa_in_pausa_esce_silenzio_e_l_anello_non_si_consuma() {
    let stato = condiviso(2);
    stato.in_pausa.store(true, Ordering::Release);
    let (_manda, mut voce) = voce(anello(16, 8, 0.9), LENTA);
    voce.andamento.porta = 0;
    let mut dati = [1.0f32; 4];
    riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
    assert_eq!(dati, [0.0; 4]);
    // Niente è stato consumato: riprendere non deve aspettare.
    assert_eq!(voce.lettore.slots(), 8);
    assert_eq!(stato.fotogrammi.load(Ordering::Relaxed), 0);
}

#[test]
fn la_pausa_sfuma_prima_di_tacere() {
    // Il clic che questa prova impedisce: il ramo della pausa scriveva zeri
    // dal primo campione, e l'onda passava da dove si trovava a zero in un
    // colpo solo.
    let stato = condiviso(2);
    stato.in_pausa.store(true, Ordering::Release);
    let (_manda, mut voce) = voce(anello(128, 128, 0.5), LENTA);
    let mut dati = [9.0f32; 40];
    riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);

    // A 1 kHz la porta si chiude in dodici fotogrammi: i primi undici
    // escono, sempre più piano, e da lì solo silenzio.
    let mut precedente = f32::INFINITY;
    for (n, fotogramma) in dati.chunks(2).enumerate() {
        let [sinistro, destro] = fotogramma else {
            panic!("fotogramma spezzato");
        };
        assert!(
            (sinistro - destro).abs() < f32::EPSILON,
            "i due canali di uno stesso istante escono insieme"
        );
        assert!(*sinistro <= precedente, "fotogramma {n} risale");
        precedente = *sinistro;
        if n < 11 {
            assert!(*sinistro > 0.0, "fotogramma {n} già muto");
        } else {
            assert_eq!(*sinistro, 0.0, "fotogramma {n} suona ancora");
        }
    }
    // Quel che è sfumato è uscito dall'anello, ed è stato contato: la
    // ripresa riparte dal campione dopo.
    assert_eq!(voce.lettore.slots(), 128 - 22);
    assert_eq!(stato.fotogrammi.load(Ordering::Relaxed), 11);
    // E con la porta chiusa il volume è tornato a zero: la ripresa salirà.
    assert_eq!(voce.andamento.guadagno.to_bits(), 0.0f32.to_bits());
    // Una pausa non è un disco lento: il silenzio chiesto non si conta fra
    // i buchi.
    assert_eq!(stato.vuoti.load(Ordering::Relaxed), 0);
}

#[test]
fn la_ripresa_sale_da_zero_senza_gradino() {
    let stato = condiviso(2);
    let (_manda, mut voce) = voce(anello(512, 512, 0.8), LENTA);
    // Dove la pausa l'ha lasciata: porta chiusa, volume a zero.
    voce.andamento.porta = 0;
    voce.andamento.guadagno = 0.0;
    let mut dati = [0.0f32; 256];
    riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
    let primo = dati.first().copied().unwrap_or(1.0);
    assert!(primo < 0.01, "primo campione della ripresa: {primo}");
    // Sale e non torna indietro: un passo all'indietro sarebbe un tremolio.
    for coppia in dati.windows(2) {
        let [prima, dopo] = coppia else { continue };
        assert!(dopo >= prima, "{dopo} dopo {prima}");
    }
    assert_eq!(
        voce.andamento.porta, voce.andamento.corsa,
        "la porta doveva aprirsi del tutto"
    );
}

#[test]
fn una_ripresa_a_meta_dissolvenza_torna_su_da_dove_era() {
    // Pausa e ripresa più vicine della dissolvenza: la porta è a metà, e la
    // ripresa deve ripartire da lì — non da uno, che sarebbe il gradino, né
    // da zero, che sarebbe un buco.
    let stato = condiviso(1);
    let (_manda, mut voce) = voce(anello(64, 64, 1.0), LENTA);
    voce.andamento.porta = 6;
    let mut dati = [0.0f32; 1];
    riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
    let uscito = dati.first().copied().unwrap_or(0.0);
    // Sei fotogrammi su dodici, più il passo di questo: sette dodicesimi.
    assert!((uscito - 7.0 / 12.0).abs() < 1e-6, "{uscito}");
}

#[test]
fn a_porta_chiusa_lo_svuotamento_scarta_tutto_e_si_disarma() {
    let stato = condiviso(2);
    stato.svuota.store(true, Ordering::Release);
    let (_manda, mut voce) = voce(anello(16, 8, 0.9), LENTA);
    voce.andamento.porta = 0;
    let mut dati = [1.0f32; 4];
    riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
    assert_eq!(dati, [0.0; 4]);
    assert_eq!(voce.lettore.slots(), 0, "l'anello doveva restare vuoto");
    assert!(!stato.svuota.load(Ordering::Acquire), "doveva disarmarsi");
}

#[test]
fn un_salto_a_musica_in_corso_sfuma_il_punto_vecchio_prima_di_buttarlo() {
    // Il salto era il terzo clic: lo svuotamento scriveva zeri al primo
    // blocco, a metà onda. Adesso il punto vecchio sfuma, e solo a porta
    // chiusa l'anello si svuota e la bandiera si abbassa.
    let stato = condiviso(1);
    stato.svuota.store(true, Ordering::Release);
    let (_manda, mut voce) = voce(anello(64, 64, 1.0), LENTA);

    // Un blocco più corto della dissolvenza: la porta non è ancora chiusa,
    // e la bandiera deve restare alzata — chi aspetta dall'altra parte
    // aspetta ancora.
    let mut primo = [0.0f32; 6];
    riempi(&mut primo, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
    assert!(primo.iter().all(|c| *c > 0.0), "{primo:?}");
    assert!(stato.svuota.load(Ordering::Acquire), "troppo presto");

    let mut secondo = [9.0f32; 10];
    riempi(&mut secondo, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
    assert!(!stato.svuota.load(Ordering::Acquire), "doveva disarmarsi");
    assert_eq!(voce.lettore.slots(), 0, "il punto vecchio doveva sparire");
    assert_eq!(secondo.last().copied(), Some(0.0));
    // Il punto nuovo salirà da zero.
    assert_eq!(voce.andamento.guadagno.to_bits(), 0.0f32.to_bits());
}

#[test]
#[expect(clippy::integer_division, reason = "campioni stereo in fotogrammi")]
fn chi_aspetta_lo_svuotamento_non_perde_i_campioni_spinti_subito_dopo() {
    // Il protocollo è quello di `Contesto::scarta_in_volo`: il filo della
    // decodifica alza `svuota`, resta fermo finché non la vede abbassata —
    // perché solo allora l'anello è certamente vuoto — e **solo allora**
    // azzera i contatori e ricomincia a spingere.
    //
    // Finché la bandiera si abbassava all'inizio della callback, quella
    // attesa finiva prima dello svuotamento: i campioni spinti subito dopo
    // finivano dentro il `while` che stava ancora svuotando, e uscivano dal
    // conto dei fotogrammi per sempre. Il debito non si richiudeva più, e a
    // pagarlo era la fine della coda — vedi il commento in `svuota_anello`,
    // e `Contesto::forse_fine`.
    //
    // Alla frequenza vera: la porta impiega qualche blocco a chiudersi, ed
    // è proprio in quei blocchi che la bandiera deve restare alzata.
    const ANELLO: usize = 65_536;
    const DA_CONTARE: usize = 4_096;
    let (mut scrittore, lettore) = rtrb::RingBuffer::<f32>::new(ANELLO);
    let stato = condiviso(2);
    let fermati = Arc::new(AtomicBool::new(false));

    let callback = {
        let stato = Arc::clone(&stato);
        let fermati = Arc::clone(&fermati);
        std::thread::spawn(move || {
            let (_manda, mut voce) = voce(lettore, 48_000);
            let mut dati = [0.0f32; 512];
            while !fermati.load(Ordering::Acquire) {
                riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
                // Il passo di un dispositivo vero, e non un giro a vuoto il
                // più in fretta possibile: senza, questo filo svuoterebbe
                // l'anello in pochi microsecondi e la finestra da riprodurre
                // non si aprirebbe mai.
                std::thread::sleep(Duration::from_millis(1));
            }
        })
    };

    // Prima si aspetta che la callback stia davvero girando — con lei non
    // ancora partita la bandiera resterebbe alzata fino al suo primo giro, e
    // lo svuotamento sarebbe finito prima ancora di cominciare.
    let _ = scrittore.push(0.9);
    for _ in 0..1_000 {
        if stato.fotogrammi.load(Ordering::Relaxed) > 0 || scrittore.slots() == ANELLO {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    // Poi l'anello si riempie fino all'orlo, perché il `while` che lo svuota
    // ci metta un tempo misurabile: è la finestra in cui il difetto viveva.
    while scrittore.push(0.9).is_ok() {}

    stato.svuota.store(true, Ordering::Release);
    // L'attesa del filo, con un giro stretto invece dei millisecondi veri:
    // qui non si vuole dormire *attraverso* l'istante in cui la bandiera si
    // abbassa, perché è proprio quell'istante che la prova guarda.
    let scadenza = Instant::now() + Duration::from_secs(5);
    while stato.svuota.load(Ordering::Acquire) && Instant::now() < scadenza {
        std::hint::spin_loop();
    }
    assert!(
        !stato.svuota.load(Ordering::Acquire),
        "la callback non ha mai abbassato la bandiera"
    );
    // L'invariante, guardata dal lato che la usa: quando il filo vede la
    // bandiera abbassata, l'anello **è** vuoto. È la promessa su cui
    // `scarta_in_volo` azzera i contatori.
    assert_eq!(
        scrittore.slots(),
        ANELLO,
        "la bandiera si è abbassata con l'anello ancora pieno"
    );
    // Da qui in poi il filo azzera il conto e ricomincia a spingere: è
    // esattamente quel che fa `scarta_in_volo`.
    stato.fotogrammi.store(0, Ordering::Relaxed);
    let mut spinti = 0usize;
    while spinti < DA_CONTARE {
        if scrittore.push(0.5).is_ok() {
            spinti = spinti.saturating_add(1);
        }
    }

    // Il tempo che la callback consumi quel che è stato spinto.
    for _ in 0..1_000 {
        if stato.fotogrammi.load(Ordering::Relaxed) >= (DA_CONTARE / 2) as u64 {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    fermati.store(true, Ordering::Release);
    let _ = callback.join();

    assert_eq!(
        stato.fotogrammi.load(Ordering::Relaxed),
        (DA_CONTARE / 2) as u64,
        "la callback ha contato meno fotogrammi di quanti ne sono stati \
         spinti: lo svuotamento si è mangiato i campioni arrivati dopo"
    );
}

// ── il conto dei fotogrammi ──

#[test]
fn un_anello_vuoto_da_silenzio_e_lo_conta() {
    let stato = condiviso(2);
    let (_manda, mut voce) = voce(anello(16, 0, 0.0), SECCA);
    let mut dati = [1.0f32; 4];
    riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
    assert_eq!(dati, [0.0; 4]);
    assert_eq!(stato.vuoti.load(Ordering::Relaxed), 4);
}

#[test]
fn i_fotogrammi_si_contano_per_canale_non_per_campione() {
    let stato = condiviso(2);
    let (_manda, mut voce) = voce(anello(16, 8, 0.5), SECCA);
    let mut dati = [0.0f32; 8];
    riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
    // Otto campioni su due canali sono quattro fotogrammi.
    assert_eq!(stato.fotogrammi.load(Ordering::Relaxed), 4);
}

#[test]
fn un_fotogramma_a_cavallo_di_due_blocchi_si_conta_una_volta() {
    // Il difetto che questa prova impedisce: `presi / canali` buttava il
    // resto. Con l'anello che si svuota a metà fotogramma, i campioni presi
    // non sono un multiplo dei canali e il fotogramma spezzato non veniva
    // contato da nessuno dei due blocchi. L'errore non si compensa — va
    // sempre nella stessa direzione — quindi su un disco lento il cursore
    // resta indietro sempre di più, in silenzio.
    let (mut scrittore, lettore) = rtrb::RingBuffer::<f32>::new(64);
    let stato = condiviso(2);
    let (_manda, mut voce) = voce(lettore, SECCA);

    // Tre campioni per un'uscita da quattro: un fotogramma e mezzo su due
    // canali. L'anello si secca esattamente in mezzo al secondo.
    for _ in 0..3 {
        let _ = scrittore.push(0.5);
    }
    let mut dati = [0.0f32; 4];
    riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
    assert_eq!(
        stato.fotogrammi.load(Ordering::Relaxed),
        1,
        "un fotogramma intero è uscito, il secondo è a metà"
    );

    // Il compagno del campione spaiato arriva adesso: insieme fanno il
    // secondo fotogramma, e va contato.
    let _ = scrittore.push(0.5);
    let mut ancora = [0.0f32; 2];
    riempi(&mut ancora, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
    assert_eq!(
        stato.fotogrammi.load(Ordering::Relaxed),
        2,
        "quattro campioni su due canali sono due fotogrammi, comunque \
         siano stati consegnati"
    );
}

#[test]
fn lo_svuotamento_dimentica_anche_il_fotogramma_a_meta() {
    // Dopo un salto il decodificatore azzera il conteggio: un resto portato
    // oltre lo svuotamento conterebbe un fotogramma del punto vecchio dentro
    // quello nuovo.
    let stato = condiviso(2);
    let (_manda, mut voce) = voce(anello(64, 1, 0.5), SECCA);
    let mut dati = [0.0f32; 2];
    riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
    assert_eq!(
        voce.andamento.resto, 1,
        "un campione spaiato è rimasto in sospeso"
    );

    stato.svuota.store(true, Ordering::Release);
    let mut silenzio = [0.0f32; 2];
    riempi(&mut silenzio, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
    assert_eq!(
        voce.andamento.resto, 0,
        "il fotogramma a metà era del punto di prima"
    );
}

#[test]
fn il_guadagno_sale_per_rampa_e_non_di_colpo() {
    let stato = condiviso(2);
    let (_manda, mut voce) = voce(anello(64, 64, 1.0), SECCA);
    voce.andamento.guadagno = 0.0;
    let mut dati = [0.0f32; 64];
    riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
    // Il primo campione è quasi zero, non uno: nessun gradino.
    let primo = dati.first().copied().unwrap_or(1.0);
    assert!(primo < 0.01, "primo campione: {primo}");
    assert!(*dati.last().unwrap_or(&0.0) > primo);
}

#[test]
fn i_campioni_alzati_dal_filo_si_piegano_anche_a_volume_pieno() {
    // Un brano alzato dalla normalizzazione senza picco noto: il filo ha
    // messo nei campioni un 1,2, e il guadagno della callback — il volume —
    // vale uno. Guardando solo quello la callback non piegava, e la
    // conversione tagliava di netto.
    let stato = condiviso(1);
    let (_manda, mut voce) = voce(anello(64, 64, 1.2), SECCA);
    stato.piega_fino.store(32, Ordering::Relaxed);
    let mut dati = [0.0f32; 16];
    riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
    for campione in dati {
        assert_eq!(
            campione.to_bits(),
            limita(1.2).to_bits(),
            "un campione alzato è uscito senza piega: {campione}"
        );
    }

    // Passato l'ultimo fotogramma alzato, si torna a non toccare niente:
    // un disco a 0 dBFS che nessuno ha alzato esce com'è.
    stato.fotogrammi.store(32, Ordering::Relaxed);
    riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
    for campione in dati {
        assert_eq!(
            campione.to_bits(),
            1.2f32.to_bits(),
            "un campione non alzato è stato piegato: {campione}"
        );
    }
}

// ── le conversioni ──

#[test]
fn il_taglio_evita_che_un_picco_avvolga_il_segno() {
    let stato = condiviso(2);
    let (_manda, mut voce) = voce(anello(8, 4, 4.0), SECCA);
    let mut dati = [0i16; 4];
    riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, a_i16);
    assert!(dati.iter().all(|&v| v > 0), "un picco è diventato negativo");
}

#[test]
fn l_uscita_in_virgola_mobile_taglia_i_campioni_fuori_scala() {
    // Il caso che il bersaglio «alto» della normalizzazione rende comune:
    // una correzione ReplayGain che porta un brano già forte sopra l'uno.
    // Prima questa uscita passava il valore com'era, e cosa ne facesse il
    // driver non era scritto da nessuna parte.
    assert!(
        (a_f32(1.8) - 1.0).abs() < f32::EPSILON,
        "non ha tagliato sopra"
    );
    assert!(
        (a_f32(-1.8) + 1.0).abs() < f32::EPSILON,
        "non ha tagliato sotto"
    );
    // E per tutto il resto, che è quasi sempre, non deve toccare niente.
    for dentro in [0.0f32, 0.5, -0.5, 1.0, -1.0] {
        assert!(
            (a_f32(dentro) - dentro).abs() < f32::EPSILON,
            "ha cambiato un campione che stava già in scala: {dentro}"
        );
    }
    // Un campione che non è un numero esce silenzio, non `NaN`: `clamp`
    // da solo lo lasciava passare fino al driver.
    assert!(a_f32(f32::NAN).abs() < f32::EPSILON);
    assert!(a_f64(f32::NAN).abs() < f64::EPSILON);
    assert!((a_f32(f32::INFINITY) - 1.0).abs() < f32::EPSILON);
}

#[test]
fn le_uscite_larghe_tagliano_e_non_avvolgono() {
    // I formati che cpal 0.18 sa consegnare: 24 e 32 bit interi, 64 in
    // virgola mobile. Stessa regola dei 16 bit — il fondo scala è il tetto,
    // e un picco oltre il tetto resta un picco, non un segno ribaltato.
    assert_eq!(a_i32(4.0), 2_147_483_647);
    assert_eq!(a_i32(-4.0), -2_147_483_647);
    assert_eq!(a_i32(0.0), 0);
    assert_eq!(a_i24(4.0).inner(), 8_388_607);
    assert_eq!(a_i24(-4.0).inner(), -8_388_607);
    assert_eq!(a_i24(0.0).inner(), 0);
    assert!((a_f64(1.8) - 1.0).abs() < f64::EPSILON);
    assert!((a_f64(0.25) - 0.25).abs() < f64::EPSILON);
}

// ── l'equalizzatore ──

#[test]
fn un_filtro_piatto_non_cambia_l_uscita() {
    let (mut scrittore, lettore) = rtrb::RingBuffer::<f32>::new(8);
    for valore in [0.1f32, -0.2, 0.3, -0.4] {
        let _ = scrittore.push(valore);
    }
    let stato = condiviso(2);
    let (_manda, mut voce) = voce(lettore, SECCA);
    let mut dati = [0.0f32; 4];
    riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
    // Il guadagno parte già a uno, quindi la rampa non sposta niente di
    // percettibile: quel che è entrato è quel che esce.
    for (uscito, atteso) in dati.iter().zip([0.1f32, -0.2, 0.3, -0.4]) {
        assert!(
            (uscito - atteso).abs() < 1e-3,
            "{uscito} invece di {atteso}"
        );
    }
}

#[test]
fn una_curva_mandata_viene_adottata_subito() {
    let stato = condiviso(2);
    let (mut manda, mut voce) = voce(anello(64, 64, 0.5), SECCA);
    // Tutto abbassato: il preamp resta a uno e i filtri tagliano.
    let _ = manda.push(Coefficienti::calcola(&[-LIMITE_DB; BANDE], true, 48_000));
    let mut dati = [0.0f32; 64];
    riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
    assert!(
        !voce.filtro.stato.piatto(),
        "la curva doveva essere ritirata"
    );
    let ultimo = dati.last().copied().unwrap_or(0.5);
    assert!(
        ultimo.abs() < 0.5,
        "l'ultimo campione vale {ultimo}: i filtri non hanno toccato niente"
    );
}

#[test]
fn lo_svuotamento_azzera_anche_la_coda_dei_filtri() {
    // Il difetto che questa prova impedisce: dopo un salto, la coda dei
    // filtri continua a suonare il punto vecchio sopra quello nuovo.
    let (mut scrittore, lettore) = rtrb::RingBuffer::<f32>::new(128);
    for _ in 0..64 {
        let _ = scrittore.push(0.9);
    }
    let stato = condiviso(2);
    let (mut manda, mut voce) = voce(lettore, SECCA);
    let _ = manda.push(Coefficienti::calcola(&[LIMITE_DB; BANDE], true, 48_000));
    let mut dati = [0.0f32; 64];
    riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);

    stato.svuota.store(true, Ordering::Release);
    let mut silenzio = [0.0f32; 4];
    riempi(&mut silenzio, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
    assert!(!stato.svuota.load(Ordering::Acquire), "doveva disarmarsi");

    // Con la memoria azzerata, un silenzio in ingresso dà un silenzio in
    // uscita. Senza, i filtri avrebbero continuato a scaricare quel che si
    // ricordavano di prima del salto.
    for _ in 0..8 {
        let _ = scrittore.push(0.0);
    }
    let mut dopo = [1.0f32; 8];
    riempi(&mut dopo, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
    for (n, valore) in dopo.iter().enumerate() {
        assert!(valore.abs() < 1e-6, "campione {n} vale {valore}, non zero");
    }
}

// ── quel che la callback racconta al filo dell'uscita ──

#[test]
fn ogni_blocco_batte_un_giro() {
    let stato = condiviso(2);
    let (_manda, mut voce) = voce(anello(16, 0, 0.0), SECCA);
    let mut dati = [0.0f32; 4];
    for atteso in 1..=3u64 {
        riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
        assert_eq!(voce.veglia.giri.load(Ordering::Acquire), atteso);
    }
}

#[test]
fn a_riposo_solo_in_pausa_a_porta_chiusa_e_con_l_anello_vuoto() {
    let stato = condiviso(2);
    let mut dati = [0.0f32; 4];

    // Suonando: mai a riposo, nemmeno con l'anello secco.
    let (_manda, mut vuota) = voce(anello(16, 0, 0.0), SECCA);
    riempi(&mut dati, &mut vuota, &stato, &SENZA_OROLOGIO, |v| v);
    assert!(!vuota.veglia.a_riposo.load(Ordering::Acquire));

    stato.in_pausa.store(true, Ordering::Release);
    // In pausa con l'anello vuoto — la coda finita, il lettore fermato.
    riempi(&mut dati, &mut vuota, &stato, &SENZA_OROLOGIO, |v| v);
    assert!(vuota.veglia.a_riposo.load(Ordering::Acquire));

    // In pausa con l'anello pieno — la pausa di chi riprenderà: no.
    let (_manda, mut piena) = voce(anello(16, 16, 0.5), SECCA);
    piena.andamento.porta = 0;
    riempi(&mut dati, &mut piena, &stato, &SENZA_OROLOGIO, |v| v);
    assert!(!piena.veglia.a_riposo.load(Ordering::Acquire));
}

#[test]
fn gli_avvisi_non_sono_perdite() {
    // Un buco nel buffer o la priorità negata non fermano il flusso: alzare
    // `perso` per loro vorrebbe dire ricostruire il motore per un
    // singhiozzo.
    assert_eq!(codice_errore(cpal::ErrorKind::Xrun), None);
    assert_eq!(codice_errore(cpal::ErrorKind::RealtimeDenied), None);
    assert_eq!(codice_errore(cpal::ErrorKind::DeviceChanged), None);
    // Il dispositivo sparito e il flusso da ricostruire sono «non c'è più».
    assert_eq!(
        codice_errore(cpal::ErrorKind::DeviceNotAvailable),
        Some(CAUSA_SPARITO)
    );
    assert_eq!(
        codice_errore(cpal::ErrorKind::StreamInvalidated),
        Some(CAUSA_SPARITO)
    );
    // Tutto il resto è un guasto.
    assert_eq!(
        codice_errore(cpal::ErrorKind::BackendError),
        Some(CAUSA_GUASTO)
    );
    assert_eq!(
        codice_errore(cpal::ErrorKind::DeviceBusy),
        Some(CAUSA_GUASTO)
    );
}

#[test]
fn un_endpoint_in_transizione_si_riprova() {
    use rymembra_domain::errors::ErrorCodeKind;

    // I due guasti del diario vero, come li consegna `cpal`: l'endpoint
    // che non si crea all'avvio, e l'orologio che non si ottiene a metà
    // riapertura perché il dispositivo è stato invalidato. Tutti e due
    // arrivano come «dispositivo non disponibile», col numero nel testo.
    let creazione = cpal::Error::with_message(
        cpal::ErrorKind::DeviceNotAvailable,
        "Elemento non trovato. (0x8889000F)",
    );
    let orologio = cpal::Error::with_message(
        cpal::ErrorKind::DeviceNotAvailable,
        "Failed to get audio clock: (0x88890004)",
    );
    for guasto in [&creazione, &orologio] {
        let err = errore_di_apertura(guasto);
        assert_eq!(err.code().kind(), ErrorCodeKind::PlaybackDeviceLost);
        assert!(err.is_retryable(), "un endpoint in transizione si riprova");
        assert!(
            err.cause().is_some_and(|c| c.contains("0x8889")),
            "il numero deve restare nel diario"
        );
    }
    // Anche il flusso da ricostruire e l'uscita presa in esclusiva per un
    // attimo: sono attese, non guasti.
    for tipo in [
        cpal::ErrorKind::StreamInvalidated,
        cpal::ErrorKind::DeviceBusy,
    ] {
        assert!(errore_di_apertura(&cpal::Error::new(tipo)).is_retryable());
    }
    // Un formato che il dispositivo rifiuta resta quel che era: riprovare
    // fra un secondo non lo cambia.
    let formato = errore_di_apertura(&cpal::Error::new(cpal::ErrorKind::UnsupportedConfig));
    assert_eq!(
        formato.code().kind(),
        ErrorCodeKind::PlaybackEngineUnavailable
    );
    assert!(!formato.is_retryable());
}

#[test]
fn un_campanello_di_un_uscita_caduta_non_fa_niente() {
    // Il campanello è debole: caduto il mittente forte, suonarlo deve
    // tornare subito invece di aspettare una risposta che nessuno darà, e
    // soprattutto non deve aver tenuto vivo il canale.
    let (manda, ricevi) = mpsc::channel::<Ordine>();
    let forte = Arc::new(manda);
    let campanello = Campanello {
        ordini: Arc::downgrade(&forte),
    };
    drop(forte);
    let prima = Instant::now();
    campanello.suona();
    assert!(prima.elapsed() < ATTESA_RISVEGLIO, "ha aspettato il vuoto");
    assert!(
        matches!(
            ricevi.recv_timeout(Duration::from_millis(10)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ),
        "il campanello ha tenuto vivo il mittente"
    );
}

// ── il riposo del flusso ──

fn sguardo(in_pausa: bool, a_riposo: bool) -> Sguardo {
    Sguardo {
        in_pausa,
        svuota: false,
        a_riposo,
    }
}

/// Un `Sonno` a flusso acceso, come dopo il primo campanello.
fn sveglio() -> Sonno {
    sveglio_dopo(RIPOSO_DOPO)
}

/// Lo stesso, su un'uscita che riposa dopo `riposo_in_pausa` di pausa.
fn sveglio_dopo(riposo_in_pausa: Duration) -> Sonno {
    let mut sonno = Sonno::nuovo(riposo_in_pausa);
    assert!(sonno.sveglia(), "appena aperto, il flusso riposa");
    sonno
}

#[test]
fn appena_aperto_il_flusso_riposa_finche_nessuno_suona() {
    // Niente è ancora stato suonato: un flusso acceso a vuoto è proprio
    // quel che teneva sveglio il computer.
    let mut sonno = Sonno::nuovo(RIPOSO_DOPO);
    let t0 = Instant::now();
    assert_eq!(sonno.passo(sguardo(true, true), t0), Mossa::Niente);
    assert!(sonno.dorme);
    // Il campanello lo mette in moto una volta sola.
    assert!(sonno.sveglia());
    assert!(!sonno.sveglia(), "già sveglio: niente da rimettere in moto");
}

#[test]
fn trenta_secondi_di_pausa_mettono_il_flusso_a_riposo() {
    let mut sonno = sveglio();
    let t0 = Instant::now();
    assert_eq!(sonno.passo(sguardo(true, false), t0), Mossa::Niente);
    let quasi = t0 + RIPOSO_DOPO - Duration::from_secs(1);
    assert_eq!(sonno.passo(sguardo(true, false), quasi), Mossa::Niente);
    assert_eq!(
        sonno.passo(sguardo(true, false), t0 + RIPOSO_DOPO),
        Mossa::Riposa
    );
    assert!(sonno.dorme);
    // A riposo non si riposa due volte.
    let dopo = t0 + RIPOSO_DOPO + Duration::from_secs(5);
    assert_eq!(sonno.passo(sguardo(true, false), dopo), Mossa::Niente);
}

#[test]
fn suonare_anche_un_istante_azzera_il_conto() {
    let mut sonno = sveglio();
    let t0 = Instant::now();
    assert_eq!(sonno.passo(sguardo(true, false), t0), Mossa::Niente);
    let meta = t0 + Duration::from_secs(20);
    assert_eq!(sonno.passo(sguardo(false, false), meta), Mossa::Niente);
    // Di nuovo in pausa: il conto riparte da qui, non da `t0`.
    assert_eq!(
        sonno.passo(sguardo(true, false), meta + Duration::from_secs(1)),
        Mossa::Niente
    );
    assert_eq!(
        sonno.passo(sguardo(true, false), t0 + RIPOSO_DOPO),
        Mossa::Niente,
        "il conto doveva ripartire dalla seconda pausa"
    );
    assert_eq!(
        sonno.passo(
            sguardo(true, false),
            meta + Duration::from_secs(1) + RIPOSO_DOPO
        ),
        Mossa::Riposa
    );
}

#[test]
fn anche_il_campanello_azzera_il_conto() {
    // Chi salta in pausa sta ancora ascoltando.
    let mut sonno = sveglio();
    let t0 = Instant::now();
    assert_eq!(sonno.passo(sguardo(true, false), t0), Mossa::Niente);
    assert!(!sonno.sveglia());
    let dopo = t0 + Duration::from_secs(25);
    assert_eq!(sonno.passo(sguardo(true, false), dopo), Mossa::Niente);
    assert_eq!(
        sonno.passo(sguardo(true, false), t0 + RIPOSO_DOPO),
        Mossa::Niente
    );
}

#[test]
fn senza_niente_da_suonare_il_riposo_arriva_quasi_subito() {
    // La coda finita o il lettore fermato: l'anello è vuoto, e trenta
    // secondi di attesa non proteggono nessuna ripresa istantanea.
    let mut sonno = sveglio();
    let t0 = Instant::now();
    assert_eq!(sonno.passo(sguardo(true, true), t0), Mossa::Niente);
    assert_eq!(
        sonno.passo(sguardo(true, true), t0 + RIPOSO_A_VUOTO),
        Mossa::Riposa
    );
}

#[test]
fn un_anello_che_si_riempie_torna_ai_trenta_secondi() {
    let mut sonno = sveglio();
    let t0 = Instant::now();
    assert_eq!(sonno.passo(sguardo(true, true), t0), Mossa::Niente);
    // Il decodificatore ha riempito l'anello: era una pausa vera.
    assert_eq!(
        sonno.passo(sguardo(true, false), t0 + Duration::from_millis(500)),
        Mossa::Niente
    );
    assert_eq!(
        sonno.passo(sguardo(true, false), t0 + RIPOSO_A_VUOTO * 3),
        Mossa::Niente
    );
}

#[test]
fn con_uno_svuotamento_in_corso_non_si_riposa() {
    // Il filo della decodifica sta aspettando proprio la callback.
    let mut sonno = sveglio();
    let t0 = Instant::now();
    let aspetta = Sguardo {
        in_pausa: true,
        svuota: true,
        a_riposo: true,
    };
    assert_eq!(sonno.passo(aspetta, t0), Mossa::Niente);
    assert_eq!(sonno.passo(aspetta, t0 + RIPOSO_DOPO * 2), Mossa::Niente);
    assert!(!sonno.dorme);
}

#[test]
fn sul_bluetooth_la_pausa_libera_le_cuffie_in_un_secondo() {
    // Le cuffie multipoint passano al telefono solo se il computer smette
    // di mandare: trenta secondi di silenzio le tenevano occupate.
    let mut sonno = sveglio_dopo(RIPOSO_BLUETOOTH);
    let t0 = Instant::now();
    assert_eq!(sonno.passo(sguardo(true, false), t0), Mossa::Niente);
    assert_eq!(
        sonno.passo(sguardo(true, false), t0 + Duration::from_millis(500)),
        Mossa::Niente,
        "mezzo secondo: la porta e il buffer non hanno ancora finito"
    );
    assert_eq!(
        sonno.passo(sguardo(true, false), t0 + RIPOSO_BLUETOOTH),
        Mossa::Riposa
    );
    assert!(sonno.dorme);
}

#[test]
fn sul_bluetooth_suonare_azzera_il_conto_lo_stesso() {
    let mut sonno = sveglio_dopo(RIPOSO_BLUETOOTH);
    let t0 = Instant::now();
    assert_eq!(sonno.passo(sguardo(true, false), t0), Mossa::Niente);
    let ripreso = t0 + Duration::from_millis(800);
    assert_eq!(sonno.passo(sguardo(false, false), ripreso), Mossa::Niente);
    // Di nuovo in pausa: il secondo si conta da qui.
    let ancora = ripreso + Duration::from_millis(100);
    assert_eq!(sonno.passo(sguardo(true, false), ancora), Mossa::Niente);
    assert_eq!(
        sonno.passo(sguardo(true, false), t0 + RIPOSO_BLUETOOTH),
        Mossa::Niente,
        "il conto doveva ripartire dalla seconda pausa"
    );
    assert_eq!(
        sonno.passo(sguardo(true, false), ancora + RIPOSO_BLUETOOTH),
        Mossa::Riposa
    );
}

#[test]
fn neanche_sul_bluetooth_si_riposa_con_uno_svuotamento_in_corso() {
    let mut sonno = sveglio_dopo(RIPOSO_BLUETOOTH);
    let t0 = Instant::now();
    let aspetta = Sguardo {
        in_pausa: true,
        svuota: true,
        a_riposo: false,
    };
    assert_eq!(sonno.passo(aspetta, t0), Mossa::Niente);
    assert_eq!(
        sonno.passo(aspetta, t0 + RIPOSO_BLUETOOTH * 10),
        Mossa::Niente
    );
    assert!(!sonno.dorme);
}

#[test]
fn fuori_dal_bluetooth_la_pausa_aspetta_ancora_trenta_secondi() {
    // Un ricevitore HDMI o ottico si mangia l'inizio a ogni ripartenza:
    // lì la ripresa istantanea vale più delle cuffie libere.
    let mut sonno = sveglio();
    let t0 = Instant::now();
    assert_eq!(sonno.passo(sguardo(true, false), t0), Mossa::Niente);
    assert_eq!(
        sonno.passo(sguardo(true, false), t0 + RIPOSO_BLUETOOTH * 5),
        Mossa::Niente
    );
    assert_eq!(
        sonno.passo(sguardo(true, false), t0 + RIPOSO_DOPO),
        Mossa::Riposa
    );
}

#[test]
fn la_rete_di_sicurezza_sveglia_chi_suona_senza_campanello() {
    let mut sonno = Sonno::nuovo(RIPOSO_DOPO);
    let t0 = Instant::now();
    // Qualcuno ha tolto la pausa senza chiamare `Uscita::sveglia`.
    assert_eq!(sonno.passo(sguardo(false, false), t0), Mossa::Sveglia);
    assert!(!sonno.dorme);
    // E qualcuno aspetta uno svuotamento a flusso fermo.
    let mut sonno = Sonno::nuovo(RIPOSO_DOPO);
    let aspetta = Sguardo {
        in_pausa: true,
        svuota: true,
        a_riposo: false,
    };
    assert_eq!(sonno.passo(aspetta, t0), Mossa::Sveglia);
}

// ── il percorso inattivo, identico al bit ───────────────────────────────

/// Un lato del confronto: una callback con il suo anello e le sue caselle.
struct Lato {
    voce: Voce,
    stato: Arc<Condiviso>,
    scrittore: rtrb::Producer<f32>,
    curve: rtrb::Producer<Coefficienti>,
}

/// Una callback appena aperta: porta chiusa, volume a zero.
fn lato(canali: u16, frequenza: u32) -> Lato {
    let (scrittore, lettore) = rtrb::RingBuffer::<f32>::new(4_096);
    let stato = condiviso(canali);
    let (curve, mut voce) = voce(lettore, frequenza);
    voce.andamento.porta = 0;
    voce.andamento.guadagno = 0.0;
    Lato {
        voce,
        stato,
        scrittore,
        curve,
    }
}

/// Numeri pseudocasuali fra −1,3 e 1,3: oltre il fondo scala apposta, così
/// anche il limitatore e i tagli delle conversioni passano dal confronto.
#[expect(
    clippy::cast_possible_truncation,
    reason = "un numero fra −1,3 e 1,3 sta in un f32"
)]
fn rumore(seme: &mut u32, quanti: usize) -> Vec<f32> {
    (0..quanti)
        .map(|_| {
            *seme = seme.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (f64::from(*seme >> 8) / f64::from(1u32 << 24) * 2.6 - 1.3) as f32
        })
        .collect()
}

/// Un passo del copione: cosa cambia prima del blocco, quanti campioni
/// entrano nell'anello, quanto è lungo il blocco.
struct Passo {
    pausa: bool,
    svuota: bool,
    guadagno: f32,
    spettro: bool,
    curva: bool,
    entrano: usize,
    blocco: usize,
}

/// Lo stesso copione sulla callback di adesso e su quella di prima: ogni
/// campione, ogni contatore, ogni pezzo di stato devono coincidere.
///
/// La callback di adesso riceve un battito **vero** — una frequenza, degli
/// istanti — così il confronto copre anche il lavoro che fa sempre,
/// pubblicare l'ancora: deve costare tempo, non suono.
fn stesso_copione<T: Copy>(converti: fn(f32) -> T, in_bit: fn(T) -> u64) {
    let copione = [
        // La ripresa da porta chiusa, col limitatore acceso.
        Passo {
            pausa: false,
            svuota: false,
            guadagno: 1.3,
            spettro: false,
            curva: false,
            entrano: 400,
            blocco: 64,
        },
        // Un blocco che non è un multiplo dei canali.
        Passo {
            pausa: false,
            svuota: false,
            guadagno: 1.3,
            spettro: true,
            curva: false,
            entrano: 0,
            blocco: 7,
        },
        Passo {
            pausa: false,
            svuota: false,
            guadagno: 0.4,
            spettro: false,
            curva: false,
            entrano: 0,
            blocco: 64,
        },
        // La pausa che sfuma a metà blocco, poi il silenzio.
        Passo {
            pausa: true,
            svuota: false,
            guadagno: 0.4,
            spettro: false,
            curva: false,
            entrano: 0,
            blocco: 20,
        },
        Passo {
            pausa: true,
            svuota: false,
            guadagno: 0.4,
            spettro: false,
            curva: false,
            entrano: 0,
            blocco: 64,
        },
        // La ripresa, con l'anello che si secca a metà fotogramma.
        Passo {
            pausa: false,
            svuota: false,
            guadagno: 0.9,
            spettro: false,
            curva: false,
            entrano: 0,
            blocco: 512,
        },
        Passo {
            pausa: false,
            svuota: false,
            guadagno: 0.9,
            spettro: false,
            curva: false,
            entrano: 3,
            blocco: 8,
        },
        Passo {
            pausa: false,
            svuota: false,
            guadagno: 0.9,
            spettro: false,
            curva: true,
            entrano: 101,
            blocco: 64,
        },
        // Un salto: il punto vecchio sfuma, poi sparisce.
        Passo {
            pausa: false,
            svuota: true,
            guadagno: 0.9,
            spettro: false,
            curva: false,
            entrano: 50,
            blocco: 10,
        },
        Passo {
            pausa: false,
            svuota: true,
            guadagno: 0.9,
            spettro: false,
            curva: false,
            entrano: 0,
            blocco: 64,
        },
        Passo {
            pausa: false,
            svuota: false,
            guadagno: 1.0,
            spettro: false,
            curva: false,
            entrano: 300,
            blocco: 128,
        },
    ];
    let mut adesso = lato(2, LENTA);
    let mut prima = lato(2, LENTA);
    let mut seme = 7u32;
    let mut battito = Battito {
        playback_ns: 1_000_000_000,
        anticipo_ns: 9_000_000,
        frequenza: LENTA,
        ingresso_ns: 990_000_000,
    };
    for (numero, passo) in copione.iter().enumerate() {
        let entrano = rumore(&mut seme, passo.entrano);
        for lato in [&mut adesso, &mut prima] {
            lato.stato.in_pausa.store(passo.pausa, Ordering::Release);
            lato.stato.svuota.store(passo.svuota, Ordering::Release);
            lato.stato
                .guadagno
                .store(passo.guadagno.to_bits(), Ordering::Relaxed);
            lato.stato.spettro.store(passo.spettro, Ordering::Relaxed);
            if passo.curva {
                let mut curva = [0.0; BANDE];
                curva[2] = 6.0;
                let _ = lato.curve.push(Coefficienti::calcola(&curva, true, 48_000));
            }
            for campione in &entrano {
                let _ = lato.scrittore.push(*campione);
            }
        }
        let mut fuori_adesso = vec![converti(9.0); passo.blocco];
        let mut fuori_prima = vec![converti(9.0); passo.blocco];
        riempi(
            &mut fuori_adesso,
            &mut adesso.voce,
            &adesso.stato,
            &battito,
            converti,
        );
        riempi_di_prima(&mut fuori_prima, &mut prima.voce, &prima.stato, converti);
        battito.playback_ns += 500_000 * u64::try_from(passo.blocco).unwrap_or(0);

        for (n, (a, b)) in fuori_adesso.iter().zip(&fuori_prima).enumerate() {
            assert_eq!(
                in_bit(*a),
                in_bit(*b),
                "passo {numero}, campione {n}: il percorso inattivo è cambiato"
            );
        }
        let contatori = |l: &Lato| {
            (
                l.stato.fotogrammi.load(Ordering::Relaxed),
                l.stato.vuoti.load(Ordering::Relaxed),
                l.stato.svuota.load(Ordering::Acquire),
                l.voce.andamento.resto,
                l.voce.andamento.porta,
                l.voce.andamento.guadagno.to_bits(),
                l.voce.veglia.a_riposo.load(Ordering::Acquire),
                l.voce.lettore.slots(),
            )
        };
        assert_eq!(
            contatori(&adesso),
            contatori(&prima),
            "passo {numero}: lo stato della callback è cambiato"
        );
    }
    // E il lavoro in più c'è stato davvero: l'ancora è stata pubblicata,
    // senza che il suono se ne accorgesse.
    assert!(adesso.stato.ancora.leggi().is_some());
    assert_eq!(adesso.stato.gradini_fatti.load(Ordering::Relaxed), 0);
}

#[test]
fn il_percorso_inattivo_e_identico_al_bit() {
    // La regola della Fase 0: con l'avvio a zero, nessun gradino, nessuna
    // deriva e la sessione spenta, la callback deve suonare **esattamente**
    // come prima — non «quasi», non entro una tolleranza. Chi non usa
    // l'ascolto insieme non deve poter sentire che esiste.
    stesso_copione(|v| v, |v: f32| u64::from(v.to_bits()));
    stesso_copione(a_i16, |v: i16| {
        u64::from(u16::from_ne_bytes(v.to_ne_bytes()))
    });
}

// ── l'ancora ────────────────────────────────────────────────────────────

/// Un milione di nanosecondi: un fotogramma a [`LENTA`], dove i conti del
/// tempo si fanno a mente.
const MS: u64 = 1_000_000;

/// Il battito di un blocco che si sentirà a `playback_ns`, a [`LENTA`].
const fn battito(playback_ns: u64) -> Battito {
    Battito {
        playback_ns,
        anticipo_ns: 10 * MS,
        frequenza: LENTA,
        ingresso_ns: playback_ns.saturating_sub(10 * MS + 3_000),
    }
}

/// Un anello mono in cui ogni campione dice da dove viene: il numero del
/// fotogramma, in sessantaquattromillesimi. Esatto in un `f32`, e la media
/// di due vicini cade esattamente a metà.
fn anello_contato(capacita: usize, quanti: usize) -> (rtrb::Producer<f32>, rtrb::Consumer<f32>) {
    let (mut scrittore, lettore) = rtrb::RingBuffer::<f32>::new(capacita);
    for n in 0..quanti {
        let _ = scrittore.push(numero_in_campione(n + 1));
    }
    (scrittore, lettore)
}

fn numero_in_campione(n: usize) -> f32 {
    f32::from(u16::try_from(n).unwrap_or(u16::MAX)) / 65_536.0
}

/// Da campione a numero di fotogramma, in metà: un numero dispari è una
/// media fra due fotogrammi.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "i campioni di prova sono numeri di fotogramma piccoli e positivi"
)]
fn meta_di(campione: f32) -> u64 {
    (campione * 131_072.0).round() as u64
}

#[test]
fn l_indice_dell_ancora_e_il_contatore_prima_del_blocco() {
    let stato = condiviso(1);
    stato.fotogrammi.store(1_000, Ordering::Relaxed);
    stato.epoca.store(4, Ordering::Release);
    let (_scrittore, lettore) = anello_contato(256, 200);
    let (_manda, mut voce) = voce(lettore, LENTA);
    let mut dati = [0.0f32; 16];
    riempi(&mut dati, &mut voce, &stato, &battito(5_000 * MS), |v| v);
    let ancora = stato
        .ancora
        .leggi()
        .expect("un blocco di musica pubblica l'ancora");
    assert_eq!(ancora.contenuto, 1_000, "il primo fotogramma del blocco");
    assert_eq!(ancora.udito_ns, 5_000 * MS);
    assert_eq!(ancora.controprova_ns, 5_000 * MS - 3_000);
    assert_eq!(ancora.epoca, 4);
    assert!(ancora.in_corso && !ancora.latenza_scartata);

    // Il blocco dopo comincia dove questo ha finito.
    riempi(&mut dati, &mut voce, &stato, &battito(5_016 * MS), |v| v);
    let dopo = stato.ancora.leggi().expect("anche il secondo");
    assert_eq!(dopo.contenuto, 1_016);
    assert_eq!(dopo.udito_ns, 5_016 * MS);
}

#[test]
fn l_ancora_e_esatta_anche_con_zeri_in_testa() {
    // L'avvio programmato tace per i primi tre fotogrammi: l'ancora è il
    // quarto, che si sente tre millesimi di secondo dopo l'inizio del
    // blocco, ed è il fotogramma d'anello che il contatore aveva prima.
    let stato = condiviso(1);
    stato.fotogrammi.store(77, Ordering::Relaxed);
    let (_scrittore, lettore) = anello_contato(256, 200);
    let (_manda, mut voce) = voce(lettore, LENTA);
    voce.andamento.porta = 0;
    stato.avvio_ns.store(2_003 * MS, Ordering::Release);
    let mut dati = [9.0f32; 16];
    riempi(&mut dati, &mut voce, &stato, &battito(2_000 * MS), |v| v);
    assert_eq!(&dati[..3], &[0.0; 3], "prima dell'avvio, silenzio");
    assert!(dati[3] > 0.0, "all'avvio, musica");
    let ancora = stato.ancora.leggi().expect("c'è musica nel blocco");
    assert_eq!(ancora.contenuto, 77);
    assert_eq!(ancora.udito_ns, 2_003 * MS);
    assert_eq!(stato.avvio_ns.load(Ordering::Acquire), 0, "avvio consumato");
}

#[test]
fn nessuna_ancora_senza_musica_o_a_meta_fotogramma() {
    // Senza musica: in pausa a porta chiusa.
    let stato = condiviso(2);
    stato.in_pausa.store(true, Ordering::Release);
    let (_manda, mut ferma) = voce(anello(64, 64, 0.5), LENTA);
    ferma.andamento.porta = 0;
    let mut dati = [0.0f32; 16];
    riempi(&mut dati, &mut ferma, &stato, &battito(MS), |v| v);
    assert_eq!(
        stato.ancora.leggi(),
        None,
        "in pausa non c'è niente da ancorare"
    );

    // Senza musica: l'anello è secco.
    stato.in_pausa.store(false, Ordering::Release);
    let (_manda, mut secca) = voce(anello(64, 0, 0.0), LENTA);
    riempi(&mut dati, &mut secca, &stato, &battito(MS), |v| v);
    assert_eq!(stato.ancora.leggi(), None, "un anello secco non è musica");

    // A metà fotogramma: il primo campione preso è il compagno di uno già
    // contato, e il contatore non dice quale fotogramma sia.
    let (_manda, mut storta) = voce(anello(64, 64, 0.5), LENTA);
    storta.andamento.resto = 1;
    riempi(&mut dati, &mut storta, &stato, &battito(MS), |v| v);
    assert_eq!(stato.ancora.leggi(), None, "un resto aperto non si ancora");
}

#[test]
fn una_latenza_assurda_ancora_alla_callback_e_lo_dice() {
    let stato = condiviso(1);
    let (_scrittore, lettore) = anello_contato(64, 64);
    let (_manda, mut voce) = voce(lettore, LENTA);
    let mut dati = [0.0f32; 4];
    let assurdo = Battito {
        anticipo_ns: (LATENZA_MASSIMA_MS + 1) * MS,
        ..battito(9_000 * MS)
    };
    riempi(&mut dati, &mut voce, &stato, &assurdo, |v| v);
    let ancora = stato.ancora.leggi().expect("c'è musica");
    assert!(ancora.latenza_scartata);
    assert_eq!(ancora.udito_ns, (9_000 - LATENZA_MASSIMA_MS - 1) * MS);
}

// ── l'avvio programmato ─────────────────────────────────────────────────

/// Fa girare blocchi da `blocco` fotogrammi mono finché non esce musica, e
/// dice a che istante esce il primo campione non nullo e quanti fotogrammi
/// d'anello erano stati consumati prima.
fn primo_suono(avvio_ns: u64, inizio_ns: u64, blocco: usize) -> (u64, u64) {
    let stato = condiviso(1);
    let (_manda, mut voce) = voce(anello(4_096, 4_096, 1.0), LENTA);
    voce.andamento.porta = 0;
    voce.andamento.guadagno = 0.0;
    stato.avvio_ns.store(avvio_ns, Ordering::Release);
    let mut playback = inizio_ns;
    for _ in 0..100 {
        let prima = stato.fotogrammi.load(Ordering::Relaxed);
        let mut dati = vec![0.0f32; blocco];
        riempi(&mut dati, &mut voce, &stato, &battito(playback), |v| v);
        if let Some(n) = dati.iter().position(|c| *c != 0.0) {
            return (playback + MS * u64::try_from(n).unwrap_or(0), prima);
        }
        assert_eq!(
            stato.fotogrammi.load(Ordering::Relaxed),
            0,
            "si è consumata musica prima dell'avvio"
        );
        playback += MS * u64::try_from(blocco).unwrap_or(0);
    }
    panic!("la musica non è mai partita");
}

#[test]
fn l_avvio_programmato_e_esatto_a_cavallo_di_piu_blocchi() {
    // Cento millisecondi e tre decimi dopo l'inizio del primo blocco, con
    // blocchi da sessantaquattro: il primo tace tutto e non tocca l'anello,
    // il secondo tace fino al fotogramma che si sente al centounesimo
    // millisecondo — il primo che non suona prima dell'avvio.
    let avvio = 1_000 * MS + 100 * MS + 300_000;
    let (suona, consumati) = primo_suono(avvio, 1_000 * MS, 64);
    assert_eq!(suona, 1_101 * MS);
    assert_eq!(consumati, 0);
}

#[test]
fn l_avvio_non_suona_mai_prima_della_scadenza() {
    // Ogni sfasamento fra l'avvio e la griglia dei fotogrammi, e ogni
    // posizione rispetto ai blocchi: il primo suono non è mai prima
    // dell'avvio, e mai più di un fotogramma dopo.
    for dopo_ns in [
        0,
        1,
        999_999,
        MS,
        MS + 1,
        5 * MS + 500_000,
        63 * MS + 999_999,
        64 * MS,
        200 * MS + 7,
    ] {
        let avvio = 1_000 * MS + dopo_ns;
        let (suona, _) = primo_suono(avvio, 1_000 * MS, 64);
        assert!(
            suona >= avvio,
            "{dopo_ns} ns: suona a {suona}, prima di {avvio}"
        );
        assert!(
            suona < avvio + MS,
            "{dopo_ns} ns: suona a {suona}, troppo dopo {avvio}"
        );
    }
}

#[test]
fn un_avvio_in_ritardo_diventa_un_salto() {
    // Dieci millisecondi e mezzo di ritardo: si parte subito, e i dieci
    // fotogrammi interi che dovevano già essere usciti si saltano.
    let stato = condiviso(1);
    let (_scrittore, lettore) = anello_contato(1_024, 1_000);
    let (_manda, mut voce) = voce(lettore, LENTA);
    voce.andamento.porta = 0;
    voce.andamento.guadagno = 1.0;
    let playback = 3_000 * MS;
    stato
        .avvio_ns
        .store(playback - 10 * MS - 500_000, Ordering::Release);
    let mut dati = [0.0f32; 64];
    riempi(&mut dati, &mut voce, &stato, &battito(playback), |v| v);

    assert_eq!(stato.avvio_ns.load(Ordering::Acquire), 0);
    assert_eq!(stato.gradini_fatti.load(Ordering::Relaxed), 1);
    // Nessun fotogramma che non consuma: i dieci saltati più i
    // sessantaquattro del blocco.
    assert_eq!(stato.fotogrammi.load(Ordering::Relaxed), 74);
    // E l'ancora sa che il primo fotogramma uscito è il decimo.
    let ancora = stato.ancora.leggi().expect("c'è musica");
    assert_eq!(ancora.contenuto, 10);
    assert_eq!(ancora.udito_ns, playback);
    // Il secondo fotogramma del blocco è l'undicesimo dell'anello.
    assert_eq!(meta_di(dati[1] * 12.0), 2 * (11 + 1), "{}", dati[1]);
}

// ── il gradino ──────────────────────────────────────────────────────────

/// Quel che un gradino fa a una callback che sta suonando: fotogrammi
/// d'anello consumati, fotogrammi d'uscita che non ne hanno consumato
/// nessuno, e l'ultimo campione uscito.
fn con_un_gradino(gradino: i64, blocchi: usize) -> (u64, u64, f32, Arc<Condiviso>) {
    let stato = condiviso(1);
    let (_scrittore, lettore) = anello_contato(4_096, 4_000);
    let (_manda, mut voce) = voce(lettore, LENTA);
    if gradino != 0 {
        stato.gradino.store(gradino, Ordering::Release);
    }
    let mut usciti = 0u64;
    let mut ultimo = 0.0;
    for _ in 0..blocchi {
        let mut dati = [0.0f32; 64];
        riempi(&mut dati, &mut voce, &stato, &battito(MS), |v| v);
        usciti += 64;
        ultimo = dati[63];
    }
    let consumati = stato.fotogrammi.load(Ordering::Relaxed);
    (consumati, usciti.saturating_sub(consumati), ultimo, stato)
}

#[test]
fn un_gradino_in_avanti_fa_salire_il_contatore_di_n() {
    let (senza, fermi_senza, ultimo_senza, _) = con_un_gradino(0, 4);
    let (con, fermi_con, ultimo_con, stato) = con_un_gradino(-30, 4);
    assert_eq!(
        con - senza,
        30,
        "trenta fotogrammi saltati, né uno di più né uno di meno"
    );
    assert_eq!(fermi_senza, 0);
    assert_eq!(
        fermi_con, 0,
        "un salto non deve lasciare fotogrammi che non consumano"
    );
    assert_eq!(
        meta_di(ultimo_con) - meta_di(ultimo_senza),
        60,
        "la musica è avanti di trenta"
    );
    assert_eq!(stato.gradini_fatti.load(Ordering::Relaxed), 1);
    assert!(
        !stato.in_correzione.load(Ordering::Acquire),
        "la porta si è riaperta"
    );
}

#[test]
fn un_gradino_all_indietro_lascia_fermo_il_contatore_per_n_fotogrammi() {
    let (senza, _, ultimo_senza, _) = con_un_gradino(0, 4);
    let (con, fermi, ultimo_con, stato) = con_un_gradino(30, 4);
    assert_eq!(
        senza - con,
        30,
        "trenta fotogrammi di silenzio non consumano niente"
    );
    assert_eq!(
        fermi, 30,
        "esattamente trenta fotogrammi d'uscita senza musica"
    );
    assert_eq!(
        meta_di(ultimo_senza) - meta_di(ultimo_con),
        60,
        "la musica è indietro di trenta"
    );
    assert_eq!(stato.gradini_fatti.load(Ordering::Relaxed), 1);
    assert!(!stato.in_correzione.load(Ordering::Acquire));
}

#[test]
fn un_gradino_oltre_l_anello_resta_in_sospeso() {
    // Quaranta fotogrammi nell'anello, cinquanta da saltare. La porta si
    // chiude consumandone undici, il salto prende i ventinove rimasti, e
    // gli altri ventuno aspettano che il decodificatore ne porti.
    let stato = condiviso(1);
    let (mut scrittore, lettore) = anello_contato(4_096, 40);
    let (_manda, mut voce) = voce(lettore, LENTA);
    stato.gradino.store(-50, Ordering::Release);
    let mut dati = [0.0f32; 64];
    riempi(&mut dati, &mut voce, &stato, &battito(MS), |v| v);
    assert_eq!(stato.fotogrammi.load(Ordering::Relaxed), 40);
    assert_eq!(voce.andamento.fase, Fase::Esegue { resto: -21 });
    assert!(stato.in_correzione.load(Ordering::Acquire));
    assert_eq!(stato.gradini_fatti.load(Ordering::Relaxed), 0);
    assert_eq!(
        stato.vuoti.load(Ordering::Relaxed),
        0,
        "a porta chiusa non è un buco"
    );

    // Arriva altra musica: il salto finisce, e da lì si suona.
    for n in 40..140 {
        let _ = scrittore.push(numero_in_campione(n + 1));
    }
    riempi(&mut dati, &mut voce, &stato, &battito(MS), |v| v);
    assert_eq!(stato.gradini_fatti.load(Ordering::Relaxed), 1);
    assert_eq!(stato.fotogrammi.load(Ordering::Relaxed), 40 + 21 + 64);
}

#[test]
fn una_pausa_fa_cadere_il_gradino() {
    let stato = condiviso(1);
    let (_manda, mut voce) = voce(anello(4_096, 4_000, 0.5), LENTA);
    stato.gradino.store(25, Ordering::Release);
    let mut dati = [0.0f32; 4];
    riempi(&mut dati, &mut voce, &stato, &battito(MS), |v| v);
    assert!(matches!(voce.andamento.fase, Fase::Chiude { quanto: 25 }));
    stato.in_pausa.store(true, Ordering::Release);
    stato.gradino.store(-8, Ordering::Release);
    riempi(&mut dati, &mut voce, &stato, &battito(MS), |v| v);
    assert_eq!(voce.andamento.fase, Fase::Libera);
    assert_eq!(stato.gradino.load(Ordering::Acquire), 0);
    assert!(!stato.in_correzione.load(Ordering::Acquire));
}

// ── la deriva ───────────────────────────────────────────────────────────

/// Cento blocchi da centouno fotogrammi mono, con la deriva data: le medie
/// uscite, e i fotogrammi consumati meno quelli usciti.
fn con_la_deriva(ppm: i32) -> (usize, i64) {
    let stato = condiviso(1);
    let (_scrittore, lettore) = anello_contato(16_384, 12_000);
    let (_manda, mut voce) = voce(lettore, LENTA);
    stato.deriva_ppm.store(ppm, Ordering::Relaxed);
    let mut medie = 0usize;
    for _ in 0..100 {
        let mut dati = [0.0f32; 101];
        riempi(&mut dati, &mut voce, &stato, &battito(MS), |v| v);
        medie += dati.iter().filter(|c| meta_di(**c) % 2 == 1).count();
    }
    let consumati = i64::try_from(stato.fotogrammi.load(Ordering::Relaxed)).unwrap_or(0);
    (medie, consumati - 10_100)
}

#[test]
fn la_deriva_interviene_un_numero_esatto_di_volte() {
    // Diecimila fotogrammi che possono intervenire — il primo di ogni
    // blocco no — a cinquecento parti per milione: cinque interventi, né
    // quattro né sei.
    assert_eq!(con_la_deriva(500).0, 5);
    assert_eq!(con_la_deriva(-500).0, 5);
    assert_eq!(con_la_deriva(0).0, 0);
    // Oltre il tetto vale il tetto.
    assert_eq!(con_la_deriva(5_000).0, 5);
}

#[test]
fn i_fotogrammi_tolti_si_contano_e_quelli_aggiunti_no() {
    // Avanti: ogni intervento consuma un fotogramma in più, e si conta.
    assert_eq!(con_la_deriva(500).1, 5);
    // Indietro: ogni intervento esce senza consumare niente.
    assert_eq!(con_la_deriva(-500).1, -5);
}

#[test]
fn la_deriva_non_tocca_mai_il_primo_fotogramma_del_blocco() {
    // L'accumulo è già oltre il fotogramma intero: il primo fotogramma che
    // può intervenire interviene. Non è mai il primo del blocco — quello è
    // l'ancora — ma sempre il secondo.
    for ppm in [500, -500] {
        let stato = condiviso(1);
        let (_scrittore, lettore) = anello_contato(4_096, 4_000);
        let (_manda, mut voce) = voce(lettore, LENTA);
        stato.deriva_ppm.store(ppm, Ordering::Relaxed);
        for _ in 0..10 {
            voce.andamento.accumulo = i64::from(ppm.signum()) * 3 * MILIONE;
            let mut dati = [0.0f32; 4];
            riempi(&mut dati, &mut voce, &stato, &battito(MS), |v| v);
            assert_eq!(meta_di(dati[0]) % 2, 0, "{ppm} ppm: il primo è una media");
            assert_eq!(meta_di(dati[1]) % 2, 1, "{ppm} ppm: il secondo non lo è");
        }
    }
}

// ── il riposo e il ponte ────────────────────────────────────────────────

#[test]
fn il_ponte_porta_un_istante_del_flusso_sull_orologio_del_processo() {
    // Il punto medio delle due letture locali.
    let ponte = Ponte::misurato(10_000, 5_000, 10_400);
    assert_eq!(ponte.locale_ns, 10_200);
    assert_eq!(ponte.locale_da_flusso(5_000), 10_200);
    assert_eq!(ponte.locale_da_flusso(7_500), 12_700);
    assert_eq!(ponte.locale_da_flusso(4_000), 9_200);
    // Prima dell'inizio dell'orologio locale si resta a zero.
    assert_eq!(ponte.locale_da_flusso(0), 5_200);
    assert_eq!(Ponte::misurato(0, 9_000, 0).locale_da_flusso(0), 0);
    assert_eq!(ponte.locale_da_flusso(u64::MAX), u64::MAX);
}

#[test]
fn dove_cade_l_avvio_rispetto_al_blocco() {
    assert_eq!(dove_cade(1_000, 1_000, 48_000), Cade::Ritardo(0));
    assert_eq!(dove_cade(1_001, 1_000, 1_000), Cade::Fra(1));
    assert_eq!(dove_cade(1_000 + 3 * MS, 1_000, 1_000), Cade::Fra(3));
    assert_eq!(
        dove_cade(1_000, 1_000 + 3 * MS + 1, 1_000),
        Cade::Ritardo(3)
    );
}

// ── il percorso di prima, congelato ─────────────────────────────────────
//
// `riempi` e `svuota_anello` com'erano prima delle primitive di tempo di
// «Ascolta insieme», copiati qui senza i commenti e mai più toccati. Sono la
// misura del percorso inattivo: con tutte le caselle nuove a zero la callback
// di adesso deve dare, campione per campione, quel che dà questa.

#[expect(clippy::integer_division, reason = "come l'originale")]
fn riempi_di_prima<T>(
    dati: &mut [T],
    voce: &mut Voce,
    condiviso: &Condiviso,
    converti: impl Fn(f32) -> T,
) {
    voce.veglia.giri.fetch_add(1, Ordering::Release);

    voce.filtro.ritira();

    let svuota = condiviso.svuota.load(Ordering::Acquire);
    let in_pausa = condiviso.in_pausa.load(Ordering::Acquire);
    let tace = svuota || in_pausa;

    let voluto = f32::from_bits(condiviso.guadagno.load(Ordering::Relaxed));
    let canali = condiviso.canali.load(Ordering::Relaxed).max(1);
    let per_fotogramma = usize::try_from(canali).unwrap_or(2);
    let filtra = !voce.filtro.stato.piatto();
    let guarda = condiviso.spettro.load(Ordering::Relaxed);
    let piega = voluto > 1.0 || filtra;
    let mut presi = 0u64;
    let mut mancati = 0u64;
    let mut apertura = voce.andamento.apertura();

    for (indice, posto) in dati.iter_mut().enumerate() {
        let canale = indice % per_fotogramma;
        if canale == 0 {
            voce.andamento.muovi_porta(!tace);
            apertura = voce.andamento.apertura();
        }
        if tace && voce.andamento.porta == 0 {
            *posto = converti(0.0);
            continue;
        }
        voce.andamento.guadagno = avvicina(voce.andamento.guadagno, voluto);
        match voce.lettore.pop() {
            Ok(campione) => {
                presi += 1;
                let campione = if filtra {
                    voce.filtro.stato.applica(campione, canale)
                } else {
                    campione
                };
                if guarda {
                    voce.spia.campione(campione, per_fotogramma);
                }
                let alzato = campione * voce.andamento.guadagno * apertura;
                *posto = converti(if piega { limita(alzato) } else { alzato });
            }
            Err(_) => {
                if !tace {
                    mancati += 1;
                }
                *posto = converti(0.0);
            }
        }
    }

    let campioni = voce.andamento.resto.saturating_add(presi);
    let canali = u64::from(canali);
    condiviso
        .fotogrammi
        .fetch_add(campioni / canali, Ordering::Relaxed);
    voce.andamento.resto = campioni % canali;
    if mancati > 0 {
        condiviso.vuoti.fetch_add(mancati, Ordering::Relaxed);
    }

    let chiusa = voce.andamento.porta == 0;
    if tace && chiusa {
        voce.andamento.guadagno = 0.0;
        if svuota {
            svuota_anello_di_prima(voce, condiviso);
        }
    }
    voce.veglia.a_riposo.store(
        in_pausa && chiusa && voce.lettore.is_empty(),
        Ordering::Release,
    );
}

fn svuota_anello_di_prima(voce: &mut Voce, condiviso: &Condiviso) {
    while voce.lettore.pop().is_ok() {}
    voce.filtro.stato.azzera();
    voce.spia.azzera();
    voce.andamento.resto = 0;
    condiviso.svuota.store(false, Ordering::Release);
}
