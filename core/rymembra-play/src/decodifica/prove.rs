//! Le prove della decodifica: aperture, salti al fotogramma, code passate
//! fra brani, tracce d'immagine. WAV, CAF, Matroska, MP3 e APE di prova si
//! costruiscono qui byte per byte; FLAC e Opus vengono da `tests/campioni`.

use super::*;

#[test]
fn i_fotogrammi_di_una_durata() {
    assert_eq!(fotogrammi_da_ms(1_000, 48_000), 48_000);
    assert_eq!(fotogrammi_da_ms(0, 44_100), 0);
    assert_eq!(ms_da_fotogrammi(48_000, 48_000), 1_000);
    assert_eq!(
        ms_da_fotogrammi(1, 0),
        1_000,
        "una frequenza a zero non divide per zero"
    );
}

#[test]
fn la_fine_del_flusso_dai_fotogrammi_dichiarati() {
    // Mezzo secondo a 44,1 kHz: il campione FLAC di `motore/prove.rs`.
    assert_eq!(fine_da_fotogrammi(Some(22_050), 44_100), Some(500));
    assert_eq!(fine_da_fotogrammi(Some(48_000), 48_000), Some(1_000));
    // Senza `num_frames` non c'è tetto: è il caso di Matroska e di un MP3
    // senza Xing su un flusso, e il salto deve partire com'è per finire
    // nella rete di sicurezza di `cerca`.
    assert_eq!(fine_da_fotogrammi(None, 44_100), None);
    // Una frequenza a zero non deve dividere per zero.
    assert_eq!(fine_da_fotogrammi(Some(22_050), 0), None);
    // Un `num_frames` assurdo satura invece di traboccare: la moltiplicazione
    // per mille si ferma a `u64::MAX`, e la divisione la riporta giù. Il
    // valore non vuol dire niente, ma non è un panico — ed è quel che serve
    // a un contenitore malformato che arriva dal disco di chiunque.
    assert_eq!(
        fine_da_fotogrammi(Some(u64::MAX), 1_000),
        Some(18_446_744_073_709_551)
    );
}

#[test]
fn un_guasto_di_rete_non_e_una_fine_di_brano() {
    // L'ordine dei rami di `cerca` in miniatura. `ERROR_NETNAME_DELETED`,
    // il 64 di Windows, arriva come `IoError` esattamente come la fine del
    // flusso: se questa funzione lo prendesse per una fine, una share morta
    // durante un salto passerebbe in silenzio e nessuno offrirebbe
    // «Riprova».
    let rete = symphonia::core::errors::Error::IoError(std::io::Error::from_raw_os_error(64));
    assert!(!oltre_la_fine(&rete));

    // E i due modi in cui symphonia dice davvero «oltre la fine».
    assert!(oltre_la_fine(&symphonia::core::errors::Error::SeekError(
        SeekErrorKind::OutOfRange
    )));
    assert!(oltre_la_fine(&symphonia::core::errors::Error::IoError(
        std::io::Error::from(std::io::ErrorKind::UnexpectedEof)
    )));
}

/// Una traccia come la descrive un demultiplatore 0.6: codec, frequenza,
/// scala dei tempi, primo timestamp e ritardo stanno in posti diversi.
fn traccia(
    codec: AudioCodecId,
    frequenza: u32,
    scala: u32,
    inizio: i64,
    ritardo: Option<u32>,
) -> Track {
    let mut parametri = AudioCodecParameters::new();
    parametri.for_codec(codec).with_sample_rate(frequenza);
    let mut traccia = Track::new(0);
    traccia
        .with_codec_params(CodecParameters::Audio(parametri))
        .with_time_base(
            symphonia::core::units::TimeBase::try_new(1, scala).expect("scala non nulla"),
        )
        .with_start_ts(Timestamp::new(inizio));
    if let Some(ritardo) = ritardo {
        traccia.with_delay(ritardo);
    }
    traccia
}

#[test]
fn l_orologio_conta_il_pre_skip_di_opus() {
    // Ogg Opus: la posizione granulare conta anche il pre-skip, e il primo
    // pacchetto comincia 312 fotogrammi prima del brano.
    let orologio = Orologio::da(
        &traccia(CODEC_ID_OPUS, 48_000, 48_000, 0, Some(312)),
        48_000,
        true,
    );
    assert_eq!(orologio.fotogramma(Timestamp::new(0)), -312);
    assert_eq!(orologio.fotogramma(Timestamp::new(72_312)), 72_000);
    assert_eq!(orologio.ts(72_000), Timestamp::new(72_312));

    // In MP3 il ritardo sta nei timestamp negativi, e il fotogramma zero è
    // il timestamp zero anche se la traccia lo dichiara.
    let orologio = Orologio::da(
        &traccia(CODEC_ID_MP3, 44_100, 44_100, 0, Some(1_105)),
        44_100,
        false,
    );
    assert_eq!(orologio.fotogramma(Timestamp::new(44_100)), 44_100);
    assert_eq!(orologio.fotogramma(Timestamp::new(-1_105)), -1_105);
    assert_eq!(orologio.ts(44_100), Timestamp::new(44_100));

    // Ogg Vorbis: la traccia comincia al ritardo con il segno meno, e il
    // ritardo la riporta a zero.
    let orologio = Orologio::da(
        &traccia(CODEC_ID_VORBIS, 44_100, 44_100, -1_024, Some(1_024)),
        44_100,
        true,
    );
    assert_eq!(orologio.fotogramma(Timestamp::new(0)), 0);
    assert_eq!(orologio.ts(0), Timestamp::new(0));

    // Una scala dei tempi che non è la frequenza, come in certi MP4.
    let orologio = Orologio::da(
        &traccia(CODEC_ID_AAC, 44_100, 90_000, 0, None),
        44_100,
        false,
    );
    assert_eq!(orologio.fotogramma(Timestamp::new(90_000)), 44_100);
    assert_eq!(orologio.ts(44_100), Timestamp::new(90_000));
}

/// Byte in memoria che si comportano come un file.
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

impl Flusso for Byte {
    fn lunghezza(&self) -> Option<u64> {
        u64::try_from(self.0.get_ref().len()).ok()
    }
}

/// Un WAV a 16 bit con i campioni dati, interlacciati su `canali`.
fn wav(frequenza: u32, canali: u16, campioni: &[i16]) -> Vec<u8> {
    let dati = u32::try_from(campioni.len() * 2).unwrap_or(0);
    let allineamento = canali * 2;
    let mut byte = Vec::with_capacity(44 + campioni.len() * 2);
    byte.extend_from_slice(b"RIFF");
    byte.extend_from_slice(&(36 + dati).to_le_bytes());
    byte.extend_from_slice(b"WAVEfmt ");
    byte.extend_from_slice(&16u32.to_le_bytes());
    byte.extend_from_slice(&1u16.to_le_bytes()); // PCM
    byte.extend_from_slice(&canali.to_le_bytes());
    byte.extend_from_slice(&frequenza.to_le_bytes());
    byte.extend_from_slice(
        &frequenza
            .saturating_mul(u32::from(allineamento))
            .to_le_bytes(),
    );
    byte.extend_from_slice(&allineamento.to_le_bytes());
    byte.extend_from_slice(&16u16.to_le_bytes()); // bit per campione
    byte.extend_from_slice(b"data");
    byte.extend_from_slice(&dati.to_le_bytes());
    for campione in campioni {
        byte.extend_from_slice(&campione.to_le_bytes());
    }
    byte
}

/// Un WAV mono a 16 bit che dichiara la frequenza data.
///
/// I campioni sono silenzio: quel che si prova è l'intestazione.
fn wav_a(frequenza: u32, fotogrammi: usize) -> Vec<u8> {
    wav(frequenza, 1, &vec![0; fotogrammi])
}

fn sorgente(byte: Vec<u8>, estensione: &str) -> Sorgente {
    Sorgente {
        track_id: 1,
        media: Box::new(Byte(std::io::Cursor::new(byte))),
        estensione: Some(estensione.to_owned()),
        durata_ms: 0,
        replaygain_db: None,
        picco: None,
        segmento: None,
    }
}

fn sorgente_wav(frequenza: u32) -> Sorgente {
    sorgente(wav_a(frequenza, 1_000), "wav")
}

/// Una rampa: il campione `n` vale `n` (modulo 32 768), così il valore
/// dice da dove viene.
fn rampa(quanti: usize) -> Vec<i16> {
    (0..quanti)
        .map(|n| i16::try_from(n % 32_768).expect("la rampa sta in un i16"))
        .collect()
}

/// Tutto quel che un decodificatore consegna, fino alla fine.
fn tutto(d: &mut Decodificatore) -> Vec<f32> {
    let mut fuori = Vec::new();
    let mut blocco = Vec::new();
    while d.prossimo(&mut blocco).expect("decodifica") {
        fuori.extend_from_slice(&blocco);
    }
    fuori
}

fn attesi(quanti: u64, da: u64, a: u64) -> usize {
    usize::try_from((quanti * a).div_ceil(da)).expect("sta in un usize")
}

#[test]
fn un_wav_che_dichiara_una_frequenza_impossibile_non_si_apre() {
    // Il giro intero, dall'intestazione al rifiuto: un contenitore che si
    // riconosce benissimo e che dichiara tre megahertz. Prima il numero
    // passava di qui senza che nessuno lo guardasse e arrivava al
    // ricampionatore, che ci dimensionava sopra i propri buffer.
    let Err(err) = Decodificatore::apri(sorgente_wav(3_000_000), 48_000, 1) else {
        panic!("una frequenza di tre megahertz non descrive della musica");
    };
    assert_eq!(
        err.code().kind(),
        rymembra_domain::errors::ErrorCodeKind::PlaybackDecodeFailed,
        "il file c'è e si legge: non è un guasto di rete né un formato ignoto"
    );
    assert!(
        err.cause().unwrap_or_default().contains("fuori scala"),
        "il rifiuto non è il nostro: causa {:?}",
        err.cause()
    );

    // Il contrappeso: la stessa intestazione con un numero vero si apre.
    assert!(Decodificatore::apri(sorgente_wav(44_100), 48_000, 1).is_ok());
}

#[test]
fn la_scala_delle_frequenze_accettabili() {
    // Gli estremi si accettano: 8000 è il telefono, 384000 il DXD, e
    // rifiutare proprio il valore di confine sarebbe un file legittimo che
    // non si apre.
    assert!(frequenza_accettabile(FREQUENZA_MIN));
    assert!(frequenza_accettabile(FREQUENZA_MAX));
    // Le due che questa libreria incontra davvero.
    assert!(frequenza_accettabile(44_100));
    assert!(frequenza_accettabile(48_000));
    // E quel che esce da un'intestazione letta male.
    assert!(!frequenza_accettabile(0));
    assert!(!frequenza_accettabile(FREQUENZA_MIN - 1));
    assert!(!frequenza_accettabile(3_000_000));
    assert!(!frequenza_accettabile(u32::MAX));
}

#[test]
fn un_uscita_a_768_khz_apre_un_brano_a_44_1() {
    // Il giro intero: il formato dell'uscita arriva dal dispositivo, e un
    // DAC impostato a 768 kHz non deve far dire a ogni brano che il file è
    // danneggiato.
    let mut d = Decodificatore::apri(sorgente_wav(44_100), 768_000, 2)
        .expect("un'uscita a 768 kHz ricampiona, non rifiuta il file");
    assert!(!tutto(&mut d).is_empty());
}

#[test]
fn la_scala_delle_frequenze_d_uscita() {
    // Gli estremi si accettano: il tetto è 768 kHz, la stessa portata di
    // campioni al secondo — in stereo — del caso peggiore su cui l'anello è
    // dimensionato (192 kHz su otto canali). 705,6 kHz è l'altro formato
    // alto dei DAC USB.
    assert!(frequenza_d_uscita_accettabile(FREQUENZA_MIN));
    assert!(frequenza_d_uscita_accettabile(48_000));
    assert!(frequenza_d_uscita_accettabile(705_600));
    assert!(frequenza_d_uscita_accettabile(USCITA_MAX));
    assert!(!frequenza_d_uscita_accettabile(0));
    assert!(!frequenza_d_uscita_accettabile(FREQUENZA_MIN - 1));
    assert!(!frequenza_d_uscita_accettabile(USCITA_MAX + 1));
    assert!(!frequenza_d_uscita_accettabile(u32::MAX));
}

#[test]
fn un_brano_ricampionato_esce_lungo_esatto() {
    // Un secondo a 44,1 kHz è un secondo a 48 kHz: 48000 fotogrammi, non
    // uno di più né uno di meno. Prima ne mancava una ventina di
    // millisecondi in coda, ed erano sei millisecondi e mezzo di silenzio
    // in testa: la tolleranza delle prove di allora era di mille.
    for quanti in [44_100_usize, 12_345, 1_000] {
        let byte = wav(44_100, 1, &vec![8_000; quanti]);
        let mut d = Decodificatore::apri(sorgente(byte, "wav"), 48_000, 1).expect("si apre");
        let campioni = tutto(&mut d);
        assert_eq!(
            campioni.len(),
            attesi(u64::try_from(quanti).expect("sta"), 44_100, 48_000),
            "{quanti} fotogrammi a 44,1 kHz"
        );
        assert_eq!(d.consegnati(), u64::try_from(campioni.len()).expect("sta"));
        // E nessun silenzio in testa: il livello c'è dal primo fotogramma
        // utile, non dopo i 320 del ritardo del filtro. I primissimi
        // risentono del bordo; al ventesimo il livello è già quello.
        let livello = 8_000.0 / 32_768.0;
        assert!(
            (campioni[20] - livello).abs() < 0.02,
            "al ventesimo fotogramma {} invece di {livello}",
            campioni[20]
        );
    }
}

#[test]
fn un_salto_su_un_wav_atterra_al_fotogramma() {
    // A 48 kHz non c'è ricampionatore, e la rampa dice di sé dove si è: il
    // WAV di symphonia salta al confine del proprio pacchetto simulato, e
    // prima si ricominciava da lì con il cursore sul millisecondo chiesto.
    let byte = wav(48_000, 1, &rampa(24_000));
    let mut d = Decodificatore::apri(sorgente(byte, "wav"), 48_000, 1).expect("si apre");
    d.cerca(250).expect("il salto riesce");
    assert_eq!(d.consegnati(), 12_000);
    let resto = tutto(&mut d);
    assert_eq!(resto.len(), 12_000, "restano esattamente 250 ms");
    assert!(
        (resto[0] - 12_000.0 / 32_768.0).abs() < 1e-6,
        "il primo campione dopo il salto è {} invece del dodicimillesimo",
        resto[0] * 32_768.0
    );
    assert!((resto[1] - 12_001.0 / 32_768.0).abs() < 1e-6);
}

#[test]
fn un_salto_ricampionato_lascia_la_coda_esatta() {
    // A 44,1 kHz con uscita a 48: dopo il salto a 250 ms restano 11 025
    // fotogrammi di sorgente, cioè 12 000 d'uscita tondi. Né il ritardo
    // del filtro in testa né il pacchetto intero di prima.
    let byte = wav(44_100, 1, &vec![4_000; 22_050]);
    let mut d = Decodificatore::apri(sorgente(byte, "wav"), 48_000, 1).expect("si apre");
    d.cerca(250).expect("il salto riesce");
    assert_eq!(d.consegnati(), 12_000);
    let resto = tutto(&mut d);
    assert_eq!(resto.len(), 12_000);
    assert_eq!(d.consegnati(), 24_000);
}

/// Un coseno a 1 kHz, a 16 bit e mezza ampiezza, dal fotogramma `da` per
/// `quanti` fotogrammi: due pezzi consecutivi sono lo stesso segnale.
#[expect(
    clippy::cast_possible_truncation,
    reason = "mezza ampiezza di un i16 arrotondata sta in un i16"
)]
fn coseno(frequenza: u32, da: usize, quanti: usize) -> Vec<i16> {
    (da..da + quanti)
        .map(|n| {
            let t = n as f64 / f64::from(frequenza);
            ((2.0 * std::f64::consts::PI * 1_000.0 * t).cos() * 16_000.0).round() as i16
        })
        .collect()
}

/// Un WAV mono a 44,1 kHz col coseno dato, aperto per un'uscita a 48.
fn brano_a_44(da: usize, quanti: usize) -> Decodificatore {
    let byte = wav(44_100, 1, &coseno(44_100, da, quanti));
    Decodificatore::apri(sorgente(byte, "wav"), 48_000, 1).expect("si apre")
}

/// Tutto un brano chiamando [`Decodificatore::prossimo_lasciando_la_coda`],
/// come fa il motore quando il seguito è in canna.
fn tutto_lasciando_la_coda(d: &mut Decodificatore) -> Vec<f32> {
    let mut fuori = Vec::new();
    let mut blocco = Vec::new();
    while d
        .prossimo_lasciando_la_coda(&mut blocco)
        .expect("decodifica")
    {
        fuori.extend_from_slice(&blocco);
    }
    fuori
}

fn scarto(uno: &[f32], altro: &[f32]) -> f32 {
    uno.iter()
        .zip(altro)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0_f32, f32::max)
}

// Il disco spezzato su un confine che non cade sulla griglia d'uscita:
// 22 051 fotogrammi a 44,1 kHz sono 24 001,09 a 48.
const PRIMO: usize = 22_051;
const SECONDO: usize = 13_230;

#[test]
fn un_ricampionatore_solo_attraversa_la_giuntura() {
    // Lo stesso disco in un file solo e in due brani: il secondo prende il
    // ricampionatore del primo, e quel che esce deve essere il disco
    // intero, campione per campione.
    let atteso = tutto(&mut brano_a_44(0, PRIMO + SECONDO));

    let mut primo = brano_a_44(0, PRIMO);
    let mut secondo = brano_a_44(PRIMO, SECONDO);
    assert!(primo.puo_passare_la_coda(&secondo));
    let mut fuori = tutto_lasciando_la_coda(&mut primo);
    assert!(primo.coda_sospesa(), "la coda deve restare nel filtro");
    let rinvio = primo.passa_la_coda(&mut secondo).expect("la coda passa");
    assert!(
        rinvio > 0,
        "a fine brano il filtro ha sempre qualcosa dentro"
    );
    assert_eq!(secondo.ereditati(), rinvio);

    // Il conto del primo è quello di un brano intero, coda compresa, anche
    // se l'ultima parte la consegna l'altro.
    let del_primo = attesi(u64::try_from(PRIMO).expect("sta"), 44_100, 48_000);
    let rinvio = usize::try_from(rinvio).expect("sta");
    assert_eq!(fuori.len() + rinvio, del_primo);
    assert_eq!(primo.consegnati(), u64::try_from(del_primo).expect("sta"));

    fuori.extend(tutto(&mut secondo));
    assert_eq!(
        fuori.len(),
        atteso.len(),
        "né un fotogramma in più né in meno"
    );
    assert_eq!(
        secondo.consegnati(),
        u64::try_from(atteso.len() - del_primo).expect("sta"),
        "la coda del primo non è fra i fotogrammi del secondo"
    );
    assert_eq!(secondo.ereditati(), 0);
    let giuntura = scarto(&fuori, &atteso);
    assert!(giuntura < 1e-6, "scarto dal disco intero: {giuntura}");

    // E la prova vede davvero la giuntura: con due ricampionatori, come
    // prima, il secondo brano esce spostato di una frazione di campione e
    // se ne discosta ben oltre.
    let mut vecchio = tutto(&mut brano_a_44(0, PRIMO));
    vecchio.extend(tutto(&mut brano_a_44(PRIMO, SECONDO)));
    assert_eq!(vecchio.len(), atteso.len());
    let prima = scarto(&vecchio, &atteso);
    assert!(prima > 1e-2, "due ricampionatori davano {prima}");
}

#[test]
fn una_coda_che_non_passa_esce_lo_stesso() {
    // Il seguito è già a 48 kHz: non ricampiona, e non può prendere niente.
    // La coda resta dentro finché qualcuno non la chiede, poi esce come a
    // ogni fine brano.
    let mut primo = brano_a_44(0, PRIMO);
    let byte = wav(48_000, 1, &coseno(48_000, 0, 4_800));
    let mut secondo = Decodificatore::apri(sorgente(byte, "wav"), 48_000, 1).expect("si apre");
    assert!(!primo.puo_passare_la_coda(&secondo));

    let mut fuori = tutto_lasciando_la_coda(&mut primo);
    assert!(primo.coda_sospesa());
    assert_eq!(primo.passa_la_coda(&mut secondo), None);
    let mut blocco = Vec::new();
    assert!(primo.consegna_la_coda(&mut blocco).expect("coda"));
    fuori.extend_from_slice(&blocco);
    assert!(!primo.coda_sospesa());
    assert!(!primo.prossimo(&mut blocco).expect("fine"));
    assert_eq!(
        fuori.len(),
        attesi(u64::try_from(PRIMO).expect("sta"), 44_100, 48_000)
    );
    assert_eq!(secondo.ereditati(), 0);
}

#[test]
fn un_salto_butta_la_coda_ereditata() {
    // Dopo il passaggio, un salto sul secondo — o il suo ritorno
    // all'inizio, che è quel che fa il motore quando rimette in canna un
    // brano — non deve far uscire niente del primo.
    let mut primo = brano_a_44(0, PRIMO);
    let mut secondo = brano_a_44(PRIMO, SECONDO);
    let _ = tutto_lasciando_la_coda(&mut primo);
    assert!(primo.passa_la_coda(&mut secondo).is_some());

    secondo.cerca(0).expect("il salto riesce");
    assert_eq!(secondo.ereditati(), 0);
    let da_capo = tutto(&mut secondo);
    let solo = tutto(&mut brano_a_44(PRIMO, SECONDO));
    assert_eq!(da_capo.len(), solo.len());
    let differenza = scarto(&da_capo, &solo);
    assert!(differenza < 1e-6, "è rimasto del primo: {differenza}");

    // E il primo, che in cambio ha avuto il filtro intatto del secondo,
    // riprende da un salto come un brano qualunque.
    primo.cerca(250).expect("il salto riesce");
    assert_eq!(primo.consegnati(), 12_000);
    let resto = tutto(&mut primo);
    assert_eq!(
        resto.len(),
        attesi(u64::try_from(PRIMO).expect("sta"), 44_100, 48_000) - 12_000
    );
}

#[test]
fn un_salto_su_un_flusso_di_rete_e_esatto_lo_stesso() {
    // Il salto «a grandi linee» dei flussi: lo scarto fino al bersaglio
    // rimette la precisione.
    struct DallaRete(Byte);
    impl std::io::Read for DallaRete {
        fn read(&mut self, dove: &mut [u8]) -> std::io::Result<usize> {
            self.0.read(dove)
        }
    }
    impl std::io::Seek for DallaRete {
        fn seek(&mut self, da: std::io::SeekFrom) -> std::io::Result<u64> {
            self.0.seek(da)
        }
    }
    impl Flusso for DallaRete {
        fn lunghezza(&self) -> Option<u64> {
            self.0.lunghezza()
        }
        fn remoto(&self) -> bool {
            true
        }
    }
    let byte = wav(48_000, 1, &rampa(24_000));
    let s = Sorgente {
        track_id: 1,
        media: Box::new(DallaRete(Byte(std::io::Cursor::new(byte)))),
        estensione: Some("wav".to_owned()),
        durata_ms: 0,
        replaygain_db: None,
        picco: None,
        segmento: None,
    };
    let mut d = Decodificatore::apri(s, 48_000, 1).expect("si apre");
    assert!(d.remoto);
    d.cerca(100).expect("il salto riesce");
    let resto = tutto(&mut d);
    assert_eq!(resto.len(), 24_000 - 4_800);
    assert!((resto[0] - 4_800.0 / 32_768.0).abs() < 1e-6);
}

/// Il FLAC di prova di `motore/prove.rs`: mezzo secondo di livello costante a
/// 44,1 kHz, mono.
const FLAC_COSTANTE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/campioni/costante.flac"
));

/// Il tono Opus di prova di `motore/prove.rs`: tre secondi esatti a 48 kHz,
/// 144 000 fotogrammi tolti pre-skip e coda.
const OPUS_TONO: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/campioni/tono.opus"
));

#[test]
fn un_salto_su_un_flac_non_si_ferma_al_blocco() {
    // Il FLAC salta al blocco di 4096 fotogrammi: prima si ricominciava
    // da lì, fino a 93 ms prima del punto chiesto. Ora restano
    // esattamente i 250 ms che mancano: 11 025 fotogrammi di sorgente,
    // 12 000 d'uscita.
    let mut d = Decodificatore::apri(sorgente(FLAC_COSTANTE.to_vec(), "flac"), 48_000, 1)
        .expect("il FLAC si apre");
    assert_eq!(tutto(&mut d).len(), 24_000, "mezzo secondo a 48 kHz");

    let mut d = Decodificatore::apri(sorgente(FLAC_COSTANTE.to_vec(), "flac"), 48_000, 1)
        .expect("il FLAC si apre");
    d.cerca(250).expect("il salto riesce");
    assert_eq!(tutto(&mut d).len(), 12_000);
}

#[test]
fn il_formato_suonato_di_un_flac_ricampionato() {
    // Il FLAC di prova è 44,1 kHz, 16 bit, mono, 154 byte per mezzo
    // secondo: su un'uscita a 48 kHz il motore deve dire tutte e cinque
    // le cose, e la sesta — il ricampionamento — da come le due frequenze
    // differiscono.
    let d = Decodificatore::apri(sorgente(FLAC_COSTANTE.to_vec(), "flac"), 48_000, 2)
        .expect("il FLAC si apre");
    assert_eq!(
        d.formato_suonato(),
        FormatoSuonato {
            codec: Some("FLAC"),
            frequenza: 44_100,
            bit: Some(16),
            canali: 1,
            frequenza_uscita: 48_000,
            kbps: Some(2),
            remoto: false,
        }
    );
}

#[test]
fn il_formato_suonato_di_un_wav_e_di_un_opus() {
    // Il PCM prende il nome dal contenitore; un codec con perdita non ha
    // bit da dire.
    let byte = wav(48_000, 2, &vec![1_000; 48_000]);
    let d = Decodificatore::apri(sorgente(byte, "wav"), 48_000, 2).expect("il WAV si apre");
    let wav = d.formato_suonato();
    assert_eq!((wav.codec, wav.bit, wav.canali), (Some("WAV"), Some(16), 2));
    // 1536 kbit/s di campioni più i 44 byte dell'intestazione.
    assert!(matches!(wav.kbps, Some(1536 | 1537)), "{:?}", wav.kbps);

    let d = Decodificatore::apri(sorgente(OPUS_TONO.to_vec(), "opus"), 48_000, 2)
        .expect("l'Opus si apre");
    let opus = d.formato_suonato();
    assert_eq!((opus.codec, opus.bit), (Some("Opus"), None));
    assert_eq!((opus.frequenza, opus.frequenza_uscita), (48_000, 48_000));
}

#[test]
fn i_nomi_dei_codec() {
    use symphonia::core::codecs::audio::well_known::{CODEC_ID_AC3, CODEC_ID_PCM_S16LE};
    use symphonia::core::formats::well_known::FORMAT_ID_MKV;
    assert_eq!(
        nome_del_codec(CODEC_ID_VORBIS, FORMAT_ID_OGG),
        Some("Ogg Vorbis")
    );
    assert_eq!(
        nome_del_codec(CODEC_ID_VORBIS, FORMAT_ID_MKV),
        Some("Vorbis")
    );
    assert_eq!(
        nome_del_codec(CODEC_ID_PCM_S16LE, FORMAT_ID_AIFF),
        Some("AIFF")
    );
    assert_eq!(
        nome_del_codec(CODEC_ID_PCM_S16LE, FORMAT_ID_MKV),
        Some("PCM")
    );
    assert_eq!(nome_del_codec(CODEC_ID_ALAC, FORMAT_ID_MKV), Some("ALAC"));
    // Un codec che la tabella non nomina: chi mostra ripiega sul database.
    assert_eq!(nome_del_codec(CODEC_ID_AC3, FORMAT_ID_MKV), None);
    assert!(senza_perdita(CODEC_ID_PCM_S16LE));
    assert!(!senza_perdita(CODEC_ID_MP3));
}

#[test]
fn il_bitrate_medio_vuole_byte_e_durata() {
    // 192 kbit/s: 24 000 byte al secondo.
    assert_eq!(kbps_medi(Some(240_000), Some(441_000), 44_100), Some(192));
    assert_eq!(kbps_medi(None, Some(441_000), 44_100), None);
    assert_eq!(kbps_medi(Some(240_000), None, 44_100), None);
    assert_eq!(kbps_medi(Some(240_000), Some(0), 44_100), None);
    assert_eq!(kbps_medi(Some(0), Some(441_000), 44_100), None);
    // Un conto assurdo non diventa un numero assurdo.
    assert_eq!(kbps_medi(Some(u64::MAX), Some(1), 384_000), None);
}

#[test]
fn un_salto_su_un_opus_atterra_al_fotogramma() {
    // Opus è il caso più storto: la posizione granulare conta anche il
    // pre-skip, e il codec ha memoria. A un secondo e mezzo dall'inizio
    // di tre secondi esatti devono restarne un secondo e mezzo esatto.
    let mut d = Decodificatore::apri(sorgente(OPUS_TONO.to_vec(), "opus"), 48_000, 1)
        .expect("l'Opus si apre");
    d.cerca(1_500).expect("il salto riesce");
    assert_eq!(d.consegnati(), 72_000);
    assert_eq!(tutto(&mut d).len(), 72_000);
}

#[test]
fn tornare_all_inizio_di_un_opus_da_lo_stesso_brano() {
    // `cerca(0)` è quel che il motore fa a un brano preparato che deve
    // tornare in canna: il pre-skip, che all'apertura toglie il codec, dopo
    // un salto lo toglie lo scarto — una volta sola, né zero né due.
    let mut d = Decodificatore::apri(sorgente(OPUS_TONO.to_vec(), "opus"), 48_000, 1)
        .expect("l'Opus si apre");
    let mut blocco = Vec::new();
    for _ in 0..10 {
        assert!(d.prossimo(&mut blocco).expect("decodifica"));
    }
    d.cerca(0).expect("il salto riesce");
    assert_eq!(d.consegnati(), 0);
    assert_eq!(tutto(&mut d).len(), 144_000);
}

#[test]
fn un_ogg_concatenato_suona_fino_in_fondo() {
    // Due flussi Ogg completi uno dopo l'altro sono un Ogg concatenato: è
    // la forma delle registrazioni di trasmissioni e di certi dischi
    // interi in un file solo. Al confine symphonia dice `ResetRequired`, e
    // prima quello era un guasto: il brano si fermava a metà.
    let mut byte = OPUS_TONO.to_vec();
    byte.extend_from_slice(OPUS_TONO);
    let mut d =
        Decodificatore::apri(sorgente(byte, "opus"), 48_000, 1).expect("l'Ogg concatenato si apre");
    let campioni = tutto(&mut d);
    // Ciascun anello toglie il proprio pre-skip e la propria coda.
    assert_eq!(campioni.len(), 2 * 144_000);
    // E dopo il confine c'è ancora il tono, non il silenzio.
    let secondo = &campioni[144_000 + 5_000..2 * 144_000 - 5_000];
    let efficace = (secondo.iter().map(|c| c * c).sum::<f32>() / secondo.len() as f32).sqrt();
    assert!(
        efficace > 0.2,
        "valore efficace dopo il confine: {efficace}"
    );
}

/// Un CAF — il Core Audio Format di Apple — con PCM a 16 bit little-endian,
/// mono, alla frequenza data. Il minimo che il formato ammette: la firma,
/// la descrizione del flusso e i dati.
fn caf(frequenza: u32, campioni: &[i16]) -> Vec<u8> {
    let mut byte = Vec::new();
    byte.extend_from_slice(b"caff");
    byte.extend_from_slice(&1u16.to_be_bytes()); // versione
    byte.extend_from_slice(&0u16.to_be_bytes()); // bandiere
    // `desc`: trentadue byte, tutti big-endian.
    byte.extend_from_slice(b"desc");
    byte.extend_from_slice(&32i64.to_be_bytes());
    byte.extend_from_slice(&f64::from(frequenza).to_be_bytes());
    byte.extend_from_slice(b"lpcm");
    byte.extend_from_slice(&2u32.to_be_bytes()); // intero, little-endian
    byte.extend_from_slice(&2u32.to_be_bytes()); // byte per pacchetto
    byte.extend_from_slice(&1u32.to_be_bytes()); // fotogrammi per pacchetto
    byte.extend_from_slice(&1u32.to_be_bytes()); // canali
    byte.extend_from_slice(&16u32.to_be_bytes()); // bit per canale
    // `data`: il conto delle modifiche, e poi i campioni.
    let dati = i64::try_from(campioni.len() * 2 + 4).expect("sta in un i64");
    byte.extend_from_slice(b"data");
    byte.extend_from_slice(&dati.to_be_bytes());
    byte.extend_from_slice(&0u32.to_be_bytes());
    for campione in campioni {
        byte.extend_from_slice(&campione.to_le_bytes());
    }
    byte
}

/// Un elemento EBML: l'identificativo così com'è, la lunghezza su otto byte
/// — la forma più lunga che la norma ammetta, così che la lunghezza di un
/// elemento non dipenda dai numeri che contiene — e il contenuto.
fn ebml(id: &[u8], contenuto: &[u8]) -> Vec<u8> {
    let mut byte = id.to_vec();
    byte.push(0x01);
    let lunghezza = u64::try_from(contenuto.len())
        .expect("sta in un u64")
        .to_be_bytes();
    byte.extend_from_slice(lunghezza.get(1..).expect("otto byte"));
    byte.extend_from_slice(contenuto);
    byte
}

/// Un intero EBML, su otto byte per la stessa ragione.
fn ebml_intero(id: &[u8], valore: u64) -> Vec<u8> {
    ebml(id, &valore.to_be_bytes())
}

/// Un Matroska audio (`.mka`) con una traccia sola, numero 1.
///
/// `codec` sono gli elementi della traccia che dicono il codec — `CodecID`,
/// `Audio` e quel che serve — e `blocchi` i pacchetti col loro timestamp in
/// millisecondi, la scala di quasi tutti i Matroska. Un cluster ogni cento
/// millisecondi, e l'indice (`Cues`) prima dei cluster: senza, il
/// demultiplatore di symphonia sa cercare solo in avanti.
fn matroska(codec: &[u8], blocchi: &[(u64, Vec<u8>)]) -> Vec<u8> {
    const CLUSTER_MS: u64 = 100;

    let testa = ebml(
        &[0x1A, 0x45, 0xDF, 0xA3],
        &[
            ebml_intero(&[0x42, 0x86], 1),
            ebml_intero(&[0x42, 0xF7], 1),
            ebml_intero(&[0x42, 0xF2], 4),
            ebml_intero(&[0x42, 0xF3], 8),
            ebml(&[0x42, 0x82], b"matroska"),
            ebml_intero(&[0x42, 0x87], 4),
            ebml_intero(&[0x42, 0x85], 2),
        ]
        .concat(),
    );
    // Un millisecondo per unità di timestamp, e i nomi di chi l'ha
    // scritto, che la norma vuole e symphonia pretende.
    let info = ebml(
        &[0x15, 0x49, 0xA9, 0x66],
        &[
            ebml_intero(&[0x2A, 0xD7, 0xB1], 1_000_000),
            ebml(&[0x4D, 0x80], b"rymembra"),
            ebml(&[0x57, 0x41], b"rymembra"),
        ]
        .concat(),
    );
    let traccia = ebml(
        &[0xAE],
        &[
            ebml_intero(&[0xD7], 1),
            ebml_intero(&[0x73, 0xC5], 1),
            ebml_intero(&[0x83], 2),
            codec.to_vec(),
        ]
        .concat(),
    );
    let tracce = ebml(&[0x16, 0x54, 0xAE, 0x6B], &traccia);

    let mut cluster: Vec<(u64, Vec<u8>)> = Vec::new();
    for (ms, dati) in blocchi {
        let inizio = ms - ms % CLUSTER_MS;
        if cluster.last().is_none_or(|(ts, _)| *ts != inizio) {
            cluster.push((inizio, ebml_intero(&[0xE7], inizio)));
        }
        let mut semplice = vec![0x81];
        semplice.extend_from_slice(
            &i16::try_from(ms - inizio)
                .expect("sta in un i16")
                .to_be_bytes(),
        );
        semplice.push(0x80);
        semplice.extend_from_slice(dati);
        if let Some((_, dentro)) = cluster.last_mut() {
            dentro.extend(ebml(&[0xA3], &semplice));
        }
    }
    let cluster: Vec<(u64, Vec<u8>)> = cluster
        .into_iter()
        .map(|(ts, dentro)| (ts, ebml(&[0x1F, 0x43, 0xB6, 0x75], &dentro)))
        .collect();

    // L'indice punta ai cluster con la loro posizione dall'inizio del
    // contenuto del segmento, e sta prima di loro: la sua lunghezza non
    // dipende dai numeri che contiene, quindi si calcola con posizioni
    // qualunque e si riscrive con quelle vere.
    let indice = |posizioni: &[(u64, u64)]| {
        let punti: Vec<u8> = posizioni
            .iter()
            .flat_map(|(ts, posizione)| {
                ebml(
                    &[0xBB],
                    &[
                        ebml_intero(&[0xB3], *ts),
                        ebml(
                            &[0xB7],
                            &[ebml_intero(&[0xF7], 1), ebml_intero(&[0xF1], *posizione)].concat(),
                        ),
                    ]
                    .concat(),
                )
            })
            .collect();
        ebml(&[0x1C, 0x53, 0xBB, 0x6B], &punti)
    };
    let finte: Vec<(u64, u64)> = cluster.iter().map(|(ts, _)| (*ts, 0)).collect();
    let mut qui = info.len() + tracce.len() + indice(&finte).len();
    let mut posizioni = Vec::new();
    for (ts, c) in &cluster {
        posizioni.push((*ts, u64::try_from(qui).expect("sta")));
        qui += c.len();
    }
    let tutti: Vec<u8> = cluster.into_iter().flat_map(|(_, c)| c).collect();
    let segmento = ebml(
        &[0x18, 0x53, 0x80, 0x67],
        &[info, tracce, indice(&posizioni), tutti].concat(),
    );
    [testa, segmento].concat()
}

/// Un Matroska con PCM a 16 bit little-endian, mono a 48 kHz, in blocchi
/// da dieci millisecondi.
fn mka(campioni: &[i16]) -> Vec<u8> {
    let audio = ebml(
        &[0xE1],
        &[
            ebml(&[0xB5], &48_000.0_f64.to_be_bytes()),
            ebml_intero(&[0x9F], 1),
            ebml_intero(&[0x62, 0x64], 16),
        ]
        .concat(),
    );
    let codec = [
        ebml(&[0x86], b"A_PCM/INT/LIT"),
        // La durata di un blocco, in nanosecondi: senza, symphonia non sa
        // dove finisce un blocco e un salto atterra sul successivo.
        ebml_intero(&[0x23, 0xE3, 0x83], 10_000_000),
        audio,
    ]
    .concat();
    let blocchi: Vec<(u64, Vec<u8>)> = campioni
        .chunks(480)
        .enumerate()
        .map(|(i, pezzo)| {
            let dati = pezzo.iter().flat_map(|c| c.to_le_bytes()).collect();
            (u64::try_from(i * 10).expect("sta"), dati)
        })
        .collect();
    matroska(&codec, &blocchi)
}

/// I pacchetti di un Ogg, nell'ordine in cui escono dalle pagine: il
/// minimo per rimetterli in un altro contenitore.
fn pacchetti_ogg(byte: &[u8]) -> Vec<Vec<u8>> {
    let mut pacchetti = Vec::new();
    let mut corrente = Vec::new();
    let mut qui = 0;
    while byte.get(qui..qui + 4) == Some(&b"OggS"[..]) {
        let segmenti = usize::from(*byte.get(qui + 26).expect("intestazione intera"));
        let tabella = byte
            .get(qui + 27..qui + 27 + segmenti)
            .expect("tabella intera");
        let mut dati = qui + 27 + segmenti;
        for lunghezza in tabella {
            let lunghezza = usize::from(*lunghezza);
            corrente.extend_from_slice(byte.get(dati..dati + lunghezza).expect("dati interi"));
            dati += lunghezza;
            if lunghezza < 255 {
                pacchetti.push(std::mem::take(&mut corrente));
            }
        }
        qui = dati;
    }
    pacchetti
}

#[test]
fn un_caf_suona_e_salta_al_fotogramma() {
    // Il CAF c'era già in symphonia 0.5, spento; in 0.6 è acceso. Mezzo
    // secondo di rampa a 48 kHz: esce identica, e un salto atterra dove
    // deve come su un WAV.
    let mut d = Decodificatore::apri(sorgente(caf(48_000, &rampa(24_000)), "caf"), 48_000, 1)
        .expect("il CAF si apre");
    let tutto_il_brano = tutto(&mut d);
    assert_eq!(tutto_il_brano.len(), 24_000);
    assert!((tutto_il_brano[23_999] - 23_999.0 / 32_768.0).abs() < 1e-6);

    let mut d = Decodificatore::apri(sorgente(caf(48_000, &rampa(24_000)), "caf"), 48_000, 1)
        .expect("il CAF si apre");
    d.cerca(250).expect("il salto riesce");
    let resto = tutto(&mut d);
    assert_eq!(resto.len(), 12_000);
    assert!((resto[0] - 12_000.0 / 32_768.0).abs() < 1e-6);
}

#[test]
fn un_matroska_suona_e_salta_avanti_e_indietro() {
    // Matroska: timestamp al millisecondo, e un salto all'indietro che
    // passa dall'indice. Un secondo di rampa a 48 kHz, mono.
    let mut d = Decodificatore::apri(sorgente(mka(&rampa(48_000)), "mka"), 48_000, 1)
        .expect("il Matroska si apre");
    let tutto_il_brano = tutto(&mut d);
    assert_eq!(tutto_il_brano.len(), 48_000);
    let ultimo = f32::from(rampa(48_000)[47_999]) / 32_768.0;
    assert!((tutto_il_brano[47_999] - ultimo).abs() < 1e-6);

    let mut d = Decodificatore::apri(sorgente(mka(&rampa(48_000)), "mka"), 48_000, 1)
        .expect("il Matroska si apre");
    // Un salto a metà di un blocco: lo scarto fino al bersaglio lo porta
    // al fotogramma, come per gli altri contenitori.
    d.cerca(255).expect("il salto riesce");
    let mut blocco = Vec::new();
    assert!(d.prossimo(&mut blocco).expect("si legge"));
    assert!(
        (blocco[0] - 12_240.0 / 32_768.0).abs() < 1e-6,
        "dopo il salto a 255 ms: {}",
        blocco[0] * 32_768.0
    );
    // E all'indietro, che senza l'indice Matroska non saprebbe fare.
    d.cerca(100).expect("il salto riesce");
    assert!(d.prossimo(&mut blocco).expect("si legge"));
    assert!(
        (blocco[0] - 4_800.0 / 32_768.0).abs() < 1e-6,
        "dopo il salto a 100 ms: {}",
        blocco[0] * 32_768.0
    );
    assert_eq!(d.consegnati(), 4_800 + fotogrammi_in(&blocco, 1));
}

#[test]
fn un_opus_in_matroska_toglie_il_pre_skip_e_non_la_coda() {
    // Gli stessi pacchetti del tono Opus di prova, tolti dall'Ogg e rimessi
    // in un Matroska come li scrive un WebM: l'`OpusHead` nel
    // `CodecPrivate`, il pre-skip come `CodecDelay` in nanosecondi. È la
    // strada per cui il decodificatore Opus riceve canali solo contati, e
    // deve descriverli da sé.
    let pacchetti = pacchetti_ogg(OPUS_TONO);
    let testa = pacchetti.first().expect("l'OpusHead").clone();
    assert_eq!(testa.get(..8), Some(&b"OpusHead"[..]));
    let canali = u64::from(testa[9]);
    let pre_skip = u64::from(u16::from_le_bytes([testa[10], testa[11]]));
    let audio = ebml(
        &[0xE1],
        &[
            ebml(&[0xB5], &48_000.0_f64.to_be_bytes()),
            ebml_intero(&[0x9F], canali),
        ]
        .concat(),
    );
    let codec = [
        ebml(&[0x86], b"A_OPUS"),
        ebml(&[0x63, 0xA2], &testa),
        ebml_intero(&[0x56, 0xAA], (pre_skip * 1_000_000_000).div_ceil(48_000)),
        ebml_intero(&[0x56, 0xBB], 80_000_000),
        audio,
    ]
    .concat();
    // Il primo pacchetto dopo l'intestazione è quello dei commenti; ogni
    // pacchetto audio del tono dura venti millisecondi.
    let blocchi: Vec<(u64, Vec<u8>)> = pacchetti
        .iter()
        .skip(2)
        .enumerate()
        .map(|(i, p)| (u64::try_from(i * 20).expect("sta"), p.clone()))
        .collect();
    let decodificati = u64::try_from(blocchi.len() * 960).expect("sta");
    let byte = matroska(&codec, &blocchi);

    let mut d =
        Decodificatore::apri(sorgente(byte.clone(), "webm"), 48_000, 1).expect("il WebM si apre");
    let campioni = tutto(&mut d);
    // Il pre-skip lo toglie il decodificatore, come in Ogg. L'imbottitura
    // in coda invece resta: Matroska la dice con `DiscardPadding`, che
    // symphonia 0.6.1 non legge. È il limite per cui un album WebM non
    // sarebbe senza stacchi, e questa prova lo tiene scritto.
    assert_eq!(
        u64::try_from(campioni.len()).expect("sta"),
        decodificati - pre_skip
    );
    assert!(campioni.len() >= 144_000);
    let mezzo = &campioni[48_000..96_000];
    let efficace = (mezzo.iter().map(|c| c * c).sum::<f32>() / mezzo.len() as f32).sqrt();
    assert!(efficace > 0.2, "valore efficace: {efficace}");

    // Il salto: i timestamp di Matroska sono al millisecondo e il ritardo
    // del codec ci entra arrotondato, quindi la precisione è quella — un
    // millisecondo, quarantotto fotogrammi — e non il fotogramma.
    let mut d = Decodificatore::apri(sorgente(byte, "webm"), 48_000, 1).expect("il WebM si apre");
    d.cerca(1_500).expect("il salto riesce");
    assert_eq!(d.consegnati(), 72_000);
    let resto = tutto(&mut d);
    let atteso = campioni.len() - 72_000;
    assert!(
        resto.len().abs_diff(atteso) <= 48,
        "dopo il salto a 1,5 s restano {} fotogrammi invece di {atteso}",
        resto.len()
    );
}

#[test]
fn lo_stereo_su_un_5_1_esce_solo_davanti() {
    // Il giro intero, dal file al dispositivo: sinistro e destro distinti,
    // uscita a sei canali.
    let mut campioni = Vec::new();
    for _ in 0..4_800 {
        campioni.push(10_000);
        campioni.push(-20_000);
    }
    let byte = wav(48_000, 2, &campioni);
    let mut d = Decodificatore::apri(sorgente(byte, "wav"), 48_000, 6).expect("si apre");
    let fuori = tutto(&mut d);
    assert_eq!(fuori.len(), 4_800 * 6);
    let (fotogrammi, avanzo) = fuori.as_chunks::<6>();
    assert!(avanzo.is_empty());
    for fotogramma in fotogrammi {
        assert!((fotogramma[0] - 10_000.0 / 32_768.0).abs() < 1e-6);
        assert!((fotogramma[1] + 20_000.0 / 32_768.0).abs() < 1e-6);
        assert!(fotogramma[2..].iter().all(|c| *c == 0.0), "{fotogramma:?}");
    }
}

/// Byte che, a bandiera alzata, rispondono con un errore come farebbe un
/// flusso di rete a cui è stato chiesto di smettere.
struct Annullabile {
    dentro: Byte,
    bandiera: Option<Arc<AtomicBool>>,
    /// Da quale byte in poi le letture guardano la bandiera: prima c'è
    /// l'intestazione, che si legge all'apertura.
    dopo: u64,
}

impl std::io::Read for Annullabile {
    fn read(&mut self, dove: &mut [u8]) -> std::io::Result<usize> {
        let posizione = self.dentro.0.position();
        if posizione >= self.dopo
            && self
                .bandiera
                .as_ref()
                .is_some_and(|b| b.load(Ordering::Relaxed))
        {
            // Lo stesso numero che `rymembra-net` usa per la rete: senza la
            // bandiera diventerebbe un guasto di rete.
            return Err(std::io::Error::from_raw_os_error(64));
        }
        self.dentro.read(dove)
    }
}

impl std::io::Seek for Annullabile {
    fn seek(&mut self, da: std::io::SeekFrom) -> std::io::Result<u64> {
        self.dentro.seek(da)
    }
}

impl Flusso for Annullabile {
    fn lunghezza(&self) -> Option<u64> {
        self.dentro.lunghezza()
    }
    fn ascolta_annullo(&mut self, annullo: Arc<AtomicBool>) {
        self.bandiera = Some(annullo);
    }
}

#[test]
fn una_lettura_annullata_non_e_un_guasto() {
    // Dieci secondi, e non mezzo: il flusso di symphonia tiene in memoria
    // fino a 64 KiB letti in anticipo, e un file che ci stesse tutto non
    // leggerebbe mai più niente dopo l'apertura.
    let quanti = 480_000;
    let bandiera = Arc::new(AtomicBool::new(false));
    let s = Sorgente {
        track_id: 1,
        media: Box::new(Annullabile {
            dentro: Byte(std::io::Cursor::new(wav(48_000, 1, &rampa(quanti)))),
            bandiera: None,
            dopo: 44,
        }),
        estensione: Some("wav".to_owned()),
        durata_ms: 0,
        replaygain_db: None,
        picco: None,
        segmento: None,
    };
    let mut d =
        Decodificatore::apri_con_annullo(s, 48_000, 1, Arc::clone(&bandiera)).expect("si apre");

    // A bandiera alzata la lettura si ferma, e quel che esce è un
    // annullamento: non «la rete non risponde», che mostrerebbe «Riprova»
    // — il flusso ha risposto col numero della rete apposta — e nemmeno
    // la fine del brano.
    bandiera.store(true, Ordering::Relaxed);
    let mut blocco = Vec::new();
    let mut esito = Ok(true);
    for _ in 0..1_000 {
        esito = d.prossimo(&mut blocco);
        if esito.is_err() {
            break;
        }
    }
    let Err(err) = esito else {
        panic!("a bandiera alzata la lettura deve fermarsi");
    };
    assert_eq!(
        err.code().kind(),
        rymembra_domain::errors::ErrorCodeKind::InternalAborted
    );
    assert!(!d.esaurito, "un annullo non chiude il brano");

    // Abbassata la bandiera, il comando nuovo — un salto — trova il brano
    // intero, e atterra dove deve.
    bandiera.store(false, Ordering::Relaxed);
    d.cerca(200).expect("il salto riesce");
    assert!(d.prossimo(&mut blocco).expect("si legge"));
    assert!((blocco[0] - 9_600.0 / 32_768.0).abs() < 1e-6);
    d.cerca(0).expect("il salto riesce");
    assert_eq!(tutto(&mut d).len(), quanti);
}

// ── le tracce d'immagine ────────────────────────────────────────────────

/// Una rampa su uno o due canali: il fotogramma `n` vale `n` (modulo
/// 32 768) a sinistra e il suo opposto a destra, così ogni campione dice da
/// che punto del file viene e da che canale.
fn rampa_su(canali: u16, quanti: usize) -> Vec<i16> {
    let mut campioni = Vec::with_capacity(quanti * usize::from(canali));
    for valore in rampa(quanti) {
        campioni.push(valore);
        if canali == 2 {
            campioni.push(-valore);
        }
    }
    campioni
}

/// Il valore che la rampa ha al fotogramma `n` del file, come lo consegna
/// il decodificatore.
fn della_rampa(n: u64) -> f32 {
    (n % 32_768) as f32 / 32_768.0
}

/// Un pezzo di un WAV, come lo descrive un `.cue`.
fn d_immagine(byte: Vec<u8>, inizio_cd: u32, fine_cd: Option<u32>) -> Sorgente {
    Sorgente {
        segmento: Some(Segmento {
            immagine: "disco.wav".to_owned(),
            inizio_cd,
            fine_cd,
        }),
        ..sorgente(byte, "wav")
    }
}

fn apri_a(s: Sorgente, frequenza: u32, canali: u16) -> Decodificatore {
    Decodificatore::apri(s, frequenza, canali).expect("si apre")
}

#[test]
fn un_segmento_suona_esattamente_il_suo_pezzo() {
    // Da 40 a 130 fotogrammi CD, cioè 1,2 secondi a partire da 0,533: a
    // 44,1 kHz sono i campioni da 23 520 a 76 440, a 48 kHz da 25 600 a
    // 83 200. Né uno in più né uno in meno, con i valori di quel punto del
    // file e non di un altro.
    for (frequenza, canali) in [(44_100_u32, 1_u16), (44_100, 2), (48_000, 1), (48_000, 2)] {
        let quanti = usize::try_from(campioni_da_cd(225, frequenza)).expect("sta");
        let byte = wav(frequenza, canali, &rampa_su(canali, quanti));
        let mut d = apri_a(d_immagine(byte, 40, Some(130)), frequenza, canali);
        assert_eq!(
            d.fine_flusso_ms(),
            Some(1_200),
            "la durata è quella del pezzo"
        );
        let fuori = tutto(&mut d);

        let attesi = campioni_da_cd(130, frequenza) - campioni_da_cd(40, frequenza);
        assert_eq!(
            attesi,
            90 * u64::from(frequenza).div_ceil(75),
            "a 44,1 e 48 kHz è esatto"
        );
        let c = usize::from(canali);
        assert_eq!(
            fuori.len(),
            usize::try_from(attesi).expect("sta") * c,
            "{frequenza} Hz, {canali} canali"
        );
        assert_eq!(d.consegnati(), attesi, "la posizione conta dal suo inizio");
        let primo = campioni_da_cd(40, frequenza);
        for (i, fotogramma) in fuori.chunks_exact(c).enumerate() {
            let atteso = della_rampa(primo + u64::try_from(i).expect("sta"));
            assert!(
                (fotogramma[0] - atteso).abs() < 1e-6,
                "{frequenza} Hz: il fotogramma {i} vale {} invece di {atteso}",
                fotogramma[0] * 32_768.0
            );
            if c == 2 {
                assert!((fotogramma[1] + atteso).abs() < 1e-6, "il canale destro");
            }
        }
        // L'ultimo è l'ultimo prima della traccia dopo.
        let ultimo = fuori.get(fuori.len() - c).copied().expect("c'è");
        assert!((ultimo - della_rampa(campioni_da_cd(130, frequenza) - 1)).abs() < 1e-6);
    }
}

#[test]
fn le_tracce_di_un_immagine_la_ricoprono_esatte() {
    // A 32 kHz e a 22 050 Hz un fotogramma CD non è un numero intero di
    // campioni: le tracce messe in fila devono essere il file, campione per
    // campione, perché i confini si arrotondano tutti nello stesso modo.
    for frequenza in [32_000_u32, 22_050, 44_100] {
        let totali = usize::try_from(campioni_da_cd(300, frequenza)).expect("sta");
        let byte = wav(frequenza, 1, &rampa(totali));
        let confini = [0_u32, 7, 13, 101, 202];
        let mut di_fila = Vec::new();
        for (i, inizio) in confini.iter().enumerate() {
            let fine = confini.get(i + 1).copied();
            let mut d = apri_a(d_immagine(byte.clone(), *inizio, fine), frequenza, 1);
            di_fila.extend(tutto(&mut d));
        }
        let intero = tutto(&mut apri_a(sorgente(byte, "wav"), frequenza, 1));
        assert_eq!(di_fila.len(), intero.len(), "{frequenza} Hz");
        assert!(di_fila == intero, "{frequenza} Hz: un campione fuori posto");
    }
}

#[test]
fn un_salto_dentro_un_segmento_conta_dal_suo_inizio() {
    let byte = wav(48_000, 1, &rampa(144_000));
    let primo = campioni_da_cd(40, 48_000);
    let lungo = 90 * 640;
    let mut d = apri_a(d_immagine(byte, 40, Some(130)), 48_000, 1);
    assert_eq!(d.consegnati(), 0);

    d.cerca(250).expect("il salto riesce");
    assert_eq!(d.consegnati(), 12_000);
    let resto = tutto(&mut d);
    assert_eq!(resto.len(), lungo - 12_000);
    assert!((resto[0] - della_rampa(primo + 12_000)).abs() < 1e-6);

    // Lo zero è l'inizio del brano, non del file.
    d.cerca(0).expect("il salto riesce");
    assert_eq!(d.consegnati(), 0);
    let da_capo = tutto(&mut d);
    assert_eq!(da_capo.len(), lungo);
    assert!((da_capo[0] - della_rampa(primo)).abs() < 1e-6);

    // Oltre la fine ci si ferma al margine prima della fine del pezzo, e la
    // fine resta quella del pezzo: niente della traccia dopo.
    d.cerca(60_000).expect("il salto riesce");
    let coda = tutto(&mut d);
    assert_eq!(coda.len(), 5_760, "i 120 ms del margine");
    let ultimo = coda.last().copied().expect("c'è");
    assert!((ultimo - della_rampa(primo + 57_599)).abs() < 1e-6);
}

#[test]
fn una_traccia_d_immagine_riprende_dov_era() {
    // «Riprendi dov'eri»: chi sta sopra apre il brano e salta alla
    // posizione salvata, che conta dall'inizio della traccia. Qui con
    // l'uscita a 48 kHz, come quasi sempre: il segmento si conta alla
    // frequenza del file, la posizione a quella dell'uscita.
    let byte = wav(44_100, 2, &rampa_su(2, 132_300));
    let mut d = apri_a(d_immagine(byte, 75, Some(150)), 48_000, 2);
    assert_eq!(d.fine_flusso_ms(), Some(1_000));
    d.cerca(500).expect("il salto riesce");
    assert_eq!(d.consegnati(), 24_000);
    let resto = tutto(&mut d);
    assert_eq!(resto.len(), 24_000 * 2, "mezzo secondo a 48 kHz, stereo");
    assert_eq!(d.consegnati(), 48_000);
}

#[test]
fn due_tracce_contigue_sono_il_disco_senza_buchi_ne_doppioni() {
    // Senza ricampionatore: la fine della prima e l'inizio della seconda
    // sono lo stesso campione, e in fila sono il file.
    let byte = wav(48_000, 2, &rampa_su(2, 144_000));
    let mut primo = apri_a(d_immagine(byte.clone(), 0, Some(100)), 48_000, 2);
    let mut secondo = apri_a(d_immagine(byte.clone(), 100, None), 48_000, 2);
    assert!(primo.contiguo_a(&secondo));
    assert!(!secondo.contiguo_a(&primo), "all'indietro non lo sono");
    let mut fuori = tutto_lasciando_la_coda(&mut primo);
    assert!(!primo.coda_sospesa(), "senza ricampionatore non c'è coda");
    assert_eq!(primo.consegnati(), 100 * 640);
    fuori.extend(tutto(&mut secondo));
    let intero = tutto(&mut apri_a(sorgente(byte, "wav"), 48_000, 2));
    assert_eq!(fuori.len(), intero.len());
    assert!(
        fuori == intero,
        "la giuntura ha perso o doppiato un campione"
    );
}

#[test]
fn due_tracce_contigue_ricampionate_passano_per_un_filtro_solo() {
    // Il caso di quasi tutti i dischi: 44,1 kHz su un'uscita a 48. La
    // seconda traccia si apre già saltata al suo inizio, e per il passaggio
    // della coda quel salto vale «all'inizio»: prende il ricampionatore della
    // prima con la coda dentro, e le due escono come il file intero.
    let totali = 3 * 44_100;
    let byte = wav(44_100, 1, &coseno(44_100, 0, totali));
    let atteso = tutto(&mut apri_a(sorgente(byte.clone(), "wav"), 48_000, 1));

    let mut primo = apri_a(d_immagine(byte.clone(), 0, Some(97)), 48_000, 1);
    let mut secondo = apri_a(d_immagine(byte, 97, None), 48_000, 1);
    assert!(primo.contiguo_a(&secondo));
    assert!(
        primo.puo_passare_la_coda(&secondo),
        "un seguito aperto al suo inizio_cd è all'inizio"
    );
    let mut fuori = tutto_lasciando_la_coda(&mut primo);
    assert!(primo.coda_sospesa(), "la coda deve restare nel filtro");
    let rinvio = primo.passa_la_coda(&mut secondo).expect("la coda passa");
    assert!(rinvio > 0);
    assert_eq!(
        primo.consegnati(),
        97 * 640,
        "il primo conta tutto il suo pezzo"
    );
    fuori.extend(tutto(&mut secondo));
    assert_eq!(
        fuori.len(),
        atteso.len(),
        "né un fotogramma in più né in meno"
    );
    let giuntura = scarto(&fuori, &atteso);
    assert!(giuntura < 1e-6, "scarto dal disco intero: {giuntura}");
}

#[test]
fn la_contiguita_e_quella_del_foglio_e_basta() {
    let traccia = |immagine: &str, inizio_cd: u32, fine_cd: Option<u32>| Segmento {
        immagine: immagine.to_owned(),
        inizio_cd,
        fine_cd,
    };
    let prima = traccia("disco.ape", 0, Some(100));
    let seconda = traccia("disco.ape", 100, Some(250));
    let ultima = traccia("disco.ape", 250, None);
    assert!(
        prima.precede(&seconda),
        "la traccia dopo, nell'ordine del disco"
    );
    assert!(seconda.precede(&ultima));
    // Ripeti uno: lo stesso pezzo non comincia dove finisce.
    assert!(!seconda.precede(&seconda));
    // L'ordine inverso, e il casuale che salta.
    assert!(!seconda.precede(&prima));
    assert!(!prima.precede(&ultima));
    // Dall'ultima alla prima: l'ultima non ha una fine scritta.
    assert!(!ultima.precede(&prima));
    assert!(!ultima.precede(&ultima));
    // Due dischi diversi con gli stessi tempi.
    assert!(!prima.precede(&traccia("altro.ape", 100, Some(250))));
}

#[test]
fn un_segmento_oltre_la_fine_del_file_e_un_brano_vuoto() {
    // Un foglio che descrive più di quel che il file contiene: la traccia
    // si apre e finisce subito, senza un errore e senza suonare altro.
    let byte = wav(48_000, 1, &rampa(48_000));
    let mut d = apri_a(d_immagine(byte, 300, None), 48_000, 1);
    assert!(tutto(&mut d).is_empty());
    assert_eq!(d.consegnati(), 0);
}

#[test]
fn un_segmento_di_un_flac_si_taglia_dentro_il_blocco() {
    // Il FLAC salta a blocchi di 4096 fotogrammi: l'inizio va raggiunto
    // scartando e la fine tagliata dentro il blocco. Da 10 a 30 fotogrammi
    // CD del mezzo secondo di prova sono 11 760 campioni, 12 800 a 48 kHz.
    let s = Sorgente {
        segmento: Some(Segmento {
            immagine: "costante.flac".to_owned(),
            inizio_cd: 10,
            fine_cd: Some(30),
        }),
        ..sorgente(FLAC_COSTANTE.to_vec(), "flac")
    };
    let mut d = apri_a(s, 48_000, 1);
    assert_eq!(tutto(&mut d).len(), 12_800);
}

#[test]
fn un_segmento_di_un_codec_con_memoria_parte_con_la_rincorsa() {
    // Il secondo centrale del tono Opus. Il pezzo comincia con la rincorsa
    // presa nella «traccia prima», che contata dal brano sarebbe negativa:
    // quel che esce deve essere esattamente quel che dà un salto a un
    // secondo nel file intero — stessa rincorsa, stesso scarto — e non un
    // codec che si riavvia sul primo campione del pezzo.
    let s = Sorgente {
        segmento: Some(Segmento {
            immagine: "tono.opus".to_owned(),
            inizio_cd: 75,
            fine_cd: Some(150),
        }),
        ..sorgente(OPUS_TONO.to_vec(), "opus")
    };
    let pezzo = tutto(&mut apri_a(s, 48_000, 1));
    assert_eq!(pezzo.len(), 48_000, "un secondo esatto");
    let mut d = apri_a(sorgente(OPUS_TONO.to_vec(), "opus"), 48_000, 1);
    d.cerca(1_000).expect("il salto riesce");
    let saltato = tutto(&mut d);
    assert!(
        pezzo[..] == saltato[..48_000],
        "il pezzo non è il salto a un secondo"
    );

    // E sta al suo posto nel file suonato di fila. Le prime migliaia di
    // campioni dopo un salto il codec li ricostruisce un po' diversi — è
    // il prezzo di ogni salto su Opus, e la rincorsa lo tiene piccolo —
    // ma passati quelli lo scarto è del codec che converge, non di un
    // fotogramma fuori posto: spostato di uno sarebbe venti volte tanto.
    let intero = tutto(&mut apri_a(sorgente(OPUS_TONO.to_vec(), "opus"), 48_000, 1));
    let a_regime = scarto(&pezzo[4_000..], &intero[52_000..96_000]);
    assert!(a_regime < 0.01, "il pezzo è fuori posto di {a_regime}");
    let spostato = scarto(&pezzo[4_000..47_999], &intero[52_001..96_000]);
    assert!(
        spostato > 0.05,
        "la prova non vede un fotogramma di scarto: {spostato}"
    );
}

#[test]
fn un_ogg_concatenato_dentro_un_immagine_si_dichiara_non_supportato() {
    // I tempi del foglio contano dal file, quelli di un Ogg ricominciano a
    // ogni anello: quando il secondo anello comincia, il brano si ferma
    // col codice dei formati che non sappiamo suonare.
    let mut byte = OPUS_TONO.to_vec();
    byte.extend_from_slice(OPUS_TONO);
    let s = Sorgente {
        segmento: Some(Segmento {
            immagine: "disco.opus".to_owned(),
            inizio_cd: 0,
            fine_cd: None,
        }),
        ..sorgente(byte, "opus")
    };
    let mut d = apri_a(s, 48_000, 1);
    let mut blocco = Vec::new();
    let err = loop {
        match d.prossimo(&mut blocco) {
            Ok(true) => {}
            Ok(false) => panic!("il secondo anello è passato come se niente fosse"),
            Err(err) => break err,
        }
    };
    assert_eq!(
        err.code().kind(),
        rymembra_domain::errors::ErrorCodeKind::PlaybackFormatUnsupported
    );
}

#[test]
fn un_brano_nel_secondo_anello_di_un_ogg_non_e_un_brano_vuoto() {
    // Due toni da tre secondi in fila: il brano che comincia a quattro
    // secondi sta nel secondo anello. symphonia non sa saltarci, e prima il
    // rifiuto diventava un brano vuoto senza un perché.
    let mut byte = OPUS_TONO.to_vec();
    byte.extend_from_slice(OPUS_TONO);
    let a_quattro = |byte: Vec<u8>| Sorgente {
        segmento: Some(Segmento {
            immagine: "disco.opus".to_owned(),
            inizio_cd: 300,
            fine_cd: None,
        }),
        ..sorgente(byte, "opus")
    };
    let Err(err) = Decodificatore::apri(a_quattro(byte), 48_000, 1) else {
        panic!("un brano nel secondo anello si è aperto");
    };
    assert_eq!(
        err.code().kind(),
        rymembra_domain::errors::ErrorCodeKind::PlaybackFormatUnsupported
    );

    // Lo stesso foglio su un Ogg di un anello solo descrive un brano oltre
    // la fine del file, come in un'immagine troncata: vuoto, come in ogni
    // altro formato, e non un errore a ogni brano che segue.
    let mut d = apri_a(a_quattro(OPUS_TONO.to_vec()), 48_000, 1);
    assert!(tutto(&mut d).is_empty());
}

/// I byte di un fotogramma dell'MP3 sintetico: MPEG-1, strato III,
/// 128 kbit/s a 44,1 kHz, mono, senza imbottitura.
const FOTOGRAMMA_MP3: usize = 417;

/// Un MP3 sintetico di `quanti` fotogrammi, tutti lunghi uguale e con le
/// informazioni laterali a zero, cioè silenzio per il decodificatore.
/// Davanti, il fotogramma del tag `tag` — `Info` o `Xing` — col numero dei
/// fotogrammi; in coda a ognuno, dove il decodificatore non guarda, il suo
/// numero, che dice quale fotogramma è stato letto.
fn mp3(tag: &[u8; 4], quanti: u32) -> Vec<u8> {
    let testa = [0xff, 0xfb, 0x90, 0xc0];
    let mut byte = Vec::new();
    let mut primo = vec![0_u8; FOTOGRAMMA_MP3];
    primo[..4].copy_from_slice(&testa);
    // Dopo i diciassette byte laterali del mono: il tag, le bandiere —
    // solo il numero dei fotogrammi — e il numero.
    primo[21..25].copy_from_slice(tag);
    primo[25..29].copy_from_slice(&1_u32.to_be_bytes());
    primo[29..33].copy_from_slice(&quanti.to_be_bytes());
    byte.extend_from_slice(&primo);
    for n in 0..quanti {
        let mut fotogramma = vec![0_u8; FOTOGRAMMA_MP3];
        fotogramma[..4].copy_from_slice(&testa);
        fotogramma[FOTOGRAMMA_MP3 - 4..].copy_from_slice(&n.to_be_bytes());
        byte.extend_from_slice(&fotogramma);
    }
    byte
}

/// Il numero del fotogramma MPEG che il contenitore consegna adesso, e il
/// suo timestamp.
fn dove_sta(d: &mut Decodificatore) -> (u64, i64) {
    let p = d.formato.next_packet().expect("si legge").expect("c'è");
    let coda: [u8; 4] = p.data[p.data.len() - 4..].try_into().expect("quattro byte");
    (u64::from(u32::from_be_bytes(coda)), p.pts.get())
}

#[test]
fn un_brano_a_meta_di_un_mp3_lungo_atterra_esatto() {
    // Cinque minuti, e la traccia da 3:00 a 3:10, col tag `Info` del
    // bitrate costante e con `Xing` del variabile. Tutte e due si aprono
    // sul fotogramma MPEG che contiene il primo campione della rincorsa,
    // col timestamp che è davvero il suo, e durano esatte: è il salto
    // accurato, che conta i fotogrammi dall'inizio del file. Un salto «a
    // grandi linee» leggerebbe meno, ma stimerebbe il timestamp dai byte —
    // e fra due tracce contigue un fotogramma sbagliato si sente. Vedi
    // `cerca`.
    let quanti = 11_500;
    let a_tre_minuti = |tag: &[u8; 4]| Sorgente {
        segmento: Some(Segmento {
            immagine: "disco.mp3".to_owned(),
            inizio_cd: 13_500,
            fine_cd: Some(14_250),
        }),
        ..sorgente(mp3(tag, quanti), "mp3")
    };
    // L'inizio della traccia meno gli 80 ms di rincorsa, in campioni.
    let partenza = 13_500 * 588 - fotogrammi_da_ms(RINCORSA_MS, 44_100);
    let atteso = (
        partenza.div_euclid(1_152),
        i64::try_from(partenza.div_euclid(1_152) * 1_152).expect("sta"),
    );
    for tag in [b"Info", b"Xing"] {
        let mut d = apri_a(a_tre_minuti(tag), 44_100, 1);
        assert_eq!(dove_sta(&mut d), atteso, "{tag:?}: all'apertura");

        let mut d = apri_a(a_tre_minuti(tag), 44_100, 1);
        assert_eq!(tutto(&mut d).len(), 750 * 588, "{tag:?}");
        // E tornare all'inizio del brano, da in fondo, atterra lì di nuovo.
        d.cerca(0).expect("il salto riesce");
        assert_eq!(dove_sta(&mut d), atteso, "{tag:?}: dopo un salto");
    }
}

#[test]
fn un_ape_che_non_si_sa_leggere_e_un_formato_non_supportato() {
    // La firma di Monkey's Audio con una versione che `ape-decoder` non
    // decodifica — la 3.97 di un'immagine vera del NAS, vedi
    // `ape::VERSIONE_MINIMA` — e una con la versione giusta e
    // l'intestazione troncata:
    // tutte e due devono arrivare come formato non supportato — non come un
    // file rotto, né come un panico in mezzo al riconoscimento.
    let mut vecchio = b"MAC ".to_vec();
    vecchio.extend_from_slice(&3_970_u16.to_le_bytes());
    vecchio.resize(4_096, 0);
    let mut troncato = b"MAC ".to_vec();
    troncato.extend_from_slice(&3_990_u16.to_le_bytes());
    troncato.extend_from_slice(&[0; 10]);
    for byte in [vecchio, troncato] {
        let Err(err) = Decodificatore::apri(sorgente(byte, "ape"), 48_000, 2) else {
            panic!("un APE illeggibile si è aperto");
        };
        assert_eq!(
            err.code().kind(),
            rymembra_domain::errors::ErrorCodeKind::PlaybackFormatUnsupported
        );
    }
}

#[test]
fn un_ape_troppo_vecchio_non_diventa_un_wav_di_rumore() {
    // I Monkey's Audio vecchi conservano dentro di sé l'intestazione del
    // WAV da cui sono nati. Quando il lettore li lasciava passare senza
    // riconoscerli, il riconoscitore cercava ancora, trovava quella, e il
    // lettore WAV suonava i byte compressi come campioni: rumore bianco.
    // Qui la firma della 3.97 e, poco dopo, un WAV vero: deve restare un
    // formato non supportato, e non aprirsi come WAV.
    let mut byte = b"MAC ".to_vec();
    byte.extend_from_slice(&3_970_u16.to_le_bytes());
    byte.resize(64, 0);
    byte.extend_from_slice(&wav(44_100, 2, &rampa_su(2, 44_100)));
    let Err(err) = Decodificatore::apri(sorgente(byte, "ape"), 48_000, 2) else {
        panic!("un APE della 3.97 si è aperto: come WAV, cioè come rumore");
    };
    assert_eq!(
        err.code().kind(),
        rymembra_domain::errors::ErrorCodeKind::PlaybackFormatUnsupported
    );
}
