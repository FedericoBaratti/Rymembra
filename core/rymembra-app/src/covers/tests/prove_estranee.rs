//! Le copertine di un altro dispositivo: i limiti stretti di decodifica (S8-2),
//! il filtro dei formati (S8-11), il numero di decodifiche insieme e lo spazio
//! che possono occupare nello store (S8-9).
//!
//! In fondo le **misure** di S8-2 (`#[ignore]`, a mano, una per processo): quanta
//! memoria costa decodificare una JPEG progressiva 8192×8192 che sta in
//! meno di mezzo mebibyte. Il file lo costruisce la prova stessa, a mano: `image`
//! non scrive JPEG progressive.
//!
//! ```text
//! cargo test -p rymembra-app --locked --lib covers::tests::prove_estranee::misura_jpeg_progressiva_8192 -- --ignored --nocapture --exact
//! ```
//!
//! # I numeri del 25/11/2026
//!
//! Profilo di sviluppo, un processo per misura, picco **in più** rispetto a prima
//! della chiamata a `ricodifica_estranea`. «Prima» è `762ae73`, con i limiti dei
//! tag (8192 pixel, 192 MiB) anche per i byte di un altro dispositivo. I tempi
//! sono del profilo di sviluppo, dove i decoder non sono ottimizzati: in
//! `--release` sono una frazione.
//!
//! | | prima: working set / commit / tempo | dopo |
//! |---|---|---|
//! | JPEG progressiva 8192×8192, 4:4:4, 393.753 byte | +577,2 / +577,3 MiB / 36,6 s | Err dall'intestazione: +1,2 / +0,1 MiB / 0 ms |
//! | la stessa, 1024×1024 (il peggio che passa) | +15,5 / +14,3 MiB / 0,92 s | uguale |
//! | una media vera da 320×320 | +1,4 / +0,4 MiB / 56 ms | uguale |
//!
//! Il numero del reperto (~576 MiB: tre buffer di coefficienti da 128 MiB di
//! `zune-jpeg`, che non entrano nel `max_alloc` di `image`) era giusto: 577,3 MiB.

use std::io::Cursor;
use std::sync::atomic::Ordering;
use std::time::Instant;

use super::*;

// ── la JPEG progressiva «piatta», fatta a mano ──────────────────────────────

/// Scrive bit, dal più alto, con il riempimento a uno e il byte di riempimento
/// dopo ogni `0xFF` che la sintassi JPEG vuole.
#[derive(Default)]
struct ScrittoreDiBit {
    byte: Vec<u8>,
    corrente: u8,
    quanti: u8,
}

impl ScrittoreDiBit {
    fn bit(&mut self, bit: u8) {
        self.corrente = (self.corrente << 1) | (bit & 1);
        self.quanti += 1;
        if self.quanti == 8 {
            self.byte.push(self.corrente);
            if self.corrente == 0xFF {
                self.byte.push(0);
            }
            self.corrente = 0;
            self.quanti = 0;
        }
    }

    fn bits(&mut self, valore: u32, quanti: u32) {
        for posto in (0..quanti).rev() {
            self.bit(u8::try_from((valore >> posto) & 1).expect("un bit"));
        }
    }

    fn chiudi(mut self) -> Vec<u8> {
        while self.quanti != 0 {
            self.bit(1);
        }
        self.byte
    }
}

fn segmento(marcatore: u8, corpo: &[u8]) -> Vec<u8> {
    let lunghezza = u16::try_from(corpo.len() + 2).expect("un segmento corto");
    let mut fuori = vec![0xFF, marcatore];
    fuori.extend_from_slice(&lunghezza.to_be_bytes());
    fuori.extend_from_slice(corpo);
    fuori
}

/// Una JPEG **progressiva** `lato`×`lato`, 4:4:4, tutta grigia: lo scan dei DC dà
/// «differenza zero» a ogni blocco (un bit), e uno scan di AC per componente è
/// fatto di sole corse di «fine blocco». I blocchi sono quelli di una foto
/// vera; i byte quasi niente: per 8192×8192 sono circa 0,4 MiB. È la forma che
/// un decoder paga di più per byte: i coefficienti di ogni blocco, di ogni
/// componente, stanno in memoria fino all'ultimo scan.
fn jpeg_progressiva_piatta(lato: u16) -> Vec<u8> {
    let blocchi_per_lato = u32::from(lato).div_ceil(8);
    let blocchi = blocchi_per_lato * blocchi_per_lato;

    let mut jpeg = vec![0xFF, 0xD8];
    // Quantizzazione: tutta uno.
    let mut dqt = vec![0x00];
    dqt.extend_from_slice(&[1_u8; 64]);
    jpeg.extend(segmento(0xDB, &dqt));
    // Progressiva, 8 bit, tre componenti 1×1.
    let mut sof = vec![8];
    sof.extend_from_slice(&lato.to_be_bytes());
    sof.extend_from_slice(&lato.to_be_bytes());
    sof.extend_from_slice(&[3, 1, 0x11, 0, 2, 0x11, 0, 3, 0x11, 0]);
    jpeg.extend(segmento(0xC2, &sof));
    // Huffman dei DC: la categoria 0 («niente») col codice «0», la 1 col «10».
    let mut dc = vec![0x00, 1, 1];
    dc.extend_from_slice(&[0; 14]);
    dc.extend_from_slice(&[0, 1]);
    jpeg.extend(segmento(0xC4, &dc));
    // Huffman degli AC: i quindici «fine blocco» da 0 a 14, ognuno con un codice
    // di quattro bit uguale al suo numero.
    let mut ac = vec![0x10, 0, 0, 0, 15];
    ac.extend_from_slice(&[0; 12]);
    ac.extend((0..15_u8).map(|n| n << 4));
    jpeg.extend(segmento(0xC4, &ac));

    // Lo scan dei DC, per tutti e tre i componenti insieme: tre bit a zero per
    // ogni gruppo di blocchi.
    jpeg.extend(segmento(0xDA, &[3, 1, 0x00, 2, 0x00, 3, 0x00, 0, 0, 0x00]));
    let bit_dei_dc = blocchi * 3;
    let mut dati = vec![0_u8; usize::try_from(bit_dei_dc.div_ceil(8)).expect("pochi byte")];
    if bit_dei_dc % 8 != 0
        && let Some(ultimo) = dati.last_mut()
    {
        *ultimo |= 0xFF >> (bit_dei_dc % 8);
    }
    jpeg.extend(dati);

    // Uno scan di AC per componente: tutte corse di «fine blocco».
    for componente in 1..=3_u8 {
        jpeg.extend(segmento(0xDA, &[1, componente, 0x00, 1, 63, 0x00]));
        let mut scrittore = ScrittoreDiBit::default();
        let mut restanti = blocchi;
        while restanti > 0 {
            let corsa = restanti.min(32_767);
            let n = 31 - corsa.leading_zeros();
            scrittore.bits(n, 4);
            scrittore.bits(corsa - (1 << n), n);
            restanti -= corsa;
        }
        jpeg.extend(scrittore.chiudi());
    }
    jpeg.extend_from_slice(&[0xFF, 0xD9]);
    jpeg
}

/// Una GIF e una BMP vere, da due pixel: i due formati che `image` apre e che una
/// copertina di un altro dispositivo non può essere (decisione D-32).
fn immagine_in(formato: image::ImageFormat) -> Vec<u8> {
    let mut fuori = Vec::new();
    image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(2, 2, |x, y| {
        image::Rgb([
            u8::try_from(x * 100).unwrap_or(0),
            u8::try_from(y * 100).unwrap_or(0),
            9,
        ])
    }))
    .write_to(&mut Cursor::new(&mut fuori), formato)
    .expect("codifica di prova");
    fuori
}

fn rifiuto(byte: &[u8]) -> AppError {
    let errore = ricodifica_estranea(byte).expect_err("va rifiutata");
    assert_eq!(
        errore.code().kind(),
        rymembra_domain::errors::ErrorCodeKind::MetadataTagReadFailed,
        "{errore}"
    );
    errore
}

#[test]
fn la_jpeg_progressiva_di_prova_e_una_jpeg_vera() {
    // Prima di fidarsi delle misure: la JPEG fatta a mano si decodifica, ed è
    // progressiva.
    for lato in [8_u16, 64, 100, 513] {
        let jpeg = jpeg_progressiva_piatta(lato);
        let immagine =
            image::load_from_memory(&jpeg).unwrap_or_else(|err| panic!("{lato}×{lato}: {err}"));
        assert_eq!(
            (immagine.width(), immagine.height()),
            (u32::from(lato), u32::from(lato))
        );
        // Tutta grigia, qualunque grigio: nessun pixel si distingue dal primo.
        let rgb = immagine.to_rgb8();
        let primo = *rgb.get_pixel(0, 0);
        assert!(rgb.pixels().all(|p| *p == primo), "{lato}×{lato}");
        // E con il marcatore SOF2 («progressiva») e non SOF0.
        assert!(jpeg.windows(2).any(|m| m == [0xFF, 0xC2]));
        assert!(!jpeg.windows(2).any(|m| m == [0xFF, 0xC0]));
    }
    let grande = jpeg_progressiva_piatta(8192);
    assert!(
        grande.len() < MASSIMO_ESTRANEA / 2,
        "8192×8192 in {} byte: sta sotto il tetto di un mebibyte",
        grande.len()
    );
}

#[test]
fn una_copertina_estranea_oltre_i_suoi_limiti_si_rifiuta_dall_intestazione() {
    // Reperto S8-2: i limiti di decodifica erano quelli dei tag (8192 pixel,
    // 192 MiB) anche per i byte di un altro dispositivo, che serve sempre la
    // media da 320 pixel.
    //
    // Un PNG che dice 4000×4000 e poi non ha i dati: se l'errore è «oltre i
    // limiti», l'intestazione è stata letta **prima** di decodificare; se è
    // «dati mancanti», no.
    let bomba = png_finto(4000, 4000, 8, 0);
    let errore = rifiuto(&bomba);
    assert!(
        errore.to_string().contains("oltre i limiti"),
        "4000×4000: {errore}"
    );
    // La JPEG progressiva 8192×8192 da mezzo mebibyte: ai limiti dei tag passava.
    let jpeg = jpeg_progressiva_piatta(8192);
    let errore = rifiuto(&jpeg);
    assert!(
        errore.to_string().contains("oltre i limiti"),
        "8192×8192: {errore}"
    );
    // Il confine: 1024 per lato passa, 1025 no, su tutti e due gli assi.
    assert!(ricodifica_estranea(&png(1024, 1024)).is_ok());
    for (larghezza, altezza) in [(1025, 8), (8, 1025), (1025, 1025)] {
        let errore = rifiuto(&png(larghezza, altezza));
        assert!(
            errore.to_string().contains("oltre i limiti"),
            "{larghezza}×{altezza}: {errore}"
        );
    }
    // Lo stesso per chi la salva nello store, e non lascia niente.
    let dir = tempfile::tempdir().expect("cartella");
    let store = CoverStore::open(dir.path()).expect("store");
    let impronta = "ab".repeat(32);
    let errore = store
        .salva_estranea(&impronta, &jpeg)
        .expect_err("va rifiutata");
    assert!(errore.to_string().contains("oltre i limiti"), "{errore}");
    assert!(!store.path_for(&impronta).exists());
    // Le copertine dei tag, invece, hanno ancora i loro limiti di sempre.
    assert!(decodifica(&png(2000, 2000), "tag").is_ok());
}

#[test]
fn una_copertina_vera_di_un_altro_dispositivo_passa_ancora() {
    // La media da 320 pixel, e la «piena» da 640 che serve «Ascolta insieme»,
    // sono sotto il tetto con ampio margine.
    for lato in [320, 640, 1024] {
        let jpeg = media_di_un_altro_di(lato);
        assert!(ricodifica_estranea(&jpeg).is_ok(), "{lato}");
    }
}

fn media_di_un_altro_di(lato: u32) -> Vec<u8> {
    let immagine = image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(lato, lato, |x, y| {
        image::Rgb([
            u8::try_from(x % 251).unwrap_or(0),
            u8::try_from(y % 251).unwrap_or(0),
            77,
        ])
    }));
    encode_jpeg(&immagine, QUALITA_MEDIA, "prova").expect("jpeg")
}

#[test]
fn una_gif_o_una_bmp_non_e_una_copertina_di_un_altro_dispositivo() {
    // Reperto S8-11: il filtro JPEG/PNG/WebP (decisione D-32) stava solo sulla via
    // del collegamento; «Ascolta insieme» passava da `ricodifica_estranea`, e
    // `image` apre anche GIF e BMP: due decoder in più sui byte di un altro.
    let dir = tempfile::tempdir().expect("cartella");
    let store = CoverStore::open(dir.path()).expect("store");
    for (formato, nome, inizio) in [
        (image::ImageFormat::Gif, "gif", "ab"),
        (image::ImageFormat::Bmp, "bmp", "ba"),
    ] {
        let byte = immagine_in(formato);
        // Che sia davvero un'immagine che `image` apre: il rifiuto è per il
        // formato.
        assert!(image::load_from_memory(&byte).is_ok(), "{nome}");
        let errore = rifiuto(&byte);
        assert!(
            errore.to_string().contains("JPEG, PNG o WebP"),
            "{nome}: {errore}"
        );
        let impronta = format!("{inizio}{}", "cd".repeat(31));
        assert!(
            store
                .salva_estranea(&impronta, &byte)
                .expect_err("va rifiutata")
                .to_string()
                .contains("JPEG, PNG o WebP"),
            "{nome}"
        );
        assert!(!store.path_for(&impronta).exists());
    }
    // Le tre che si accettano.
    for byte in [
        media_di_un_altro_di(64),
        png(64, 64),
        immagine_in(image::ImageFormat::WebP),
    ] {
        assert!(ricodifica_estranea(&byte).is_ok());
    }
}

#[test]
fn le_decodifiche_di_byte_altrui_non_sono_piu_di_due_insieme() {
    // Reperto S8-2: tre per dispositivo e sei dalla finestra, tutte sul pool
    // bloccante, e nessun semaforo sulla memoria e sulla CPU di una decodifica.
    // Il conto lo tiene `decodifica_estranea`, il solo posto da cui passano.
    let png = png(1000, 1000);
    DECODIFICHE_ESTRANEE.1.store(0, Ordering::SeqCst);
    let partenza = std::sync::Arc::new(std::sync::Barrier::new(8));
    let fili: Vec<_> = (0..8)
        .map(|_| {
            let partenza = std::sync::Arc::clone(&partenza);
            let png = png.clone();
            std::thread::spawn(move || {
                partenza.wait();
                for _ in 0..3 {
                    ricodifica_estranea(&png).expect("ricodificata");
                }
            })
        })
        .collect();
    for filo in fili {
        filo.join().expect("il filo finisce");
    }
    let massimo = DECODIFICHE_ESTRANEE.1.load(Ordering::SeqCst);
    assert!(
        (1..=DECODIFICHE_ESTRANEE_INSIEME).contains(&massimo),
        "{massimo} decodifiche di byte altrui insieme, il tetto è {DECODIFICHE_ESTRANEE_INSIEME}"
    );
    assert_eq!(DECODIFICHE_ESTRANEE.0.load(Ordering::SeqCst), 0);
}

#[test]
fn una_decodifica_che_cade_non_tiene_il_suo_posto() {
    // Un panico dentro il decoder, o una `?` presa a metà: il posto si rende
    // comunque, o dopo due cadute nessuna copertina di nessuno si decodifica più.
    for _ in 0..(DECODIFICHE_ESTRANEE_INSIEME * 3) {
        let esito = std::thread::spawn(|| {
            let _posto = PostoDiDecodifica::prendi();
            panic!("il decoder cade");
        })
        .join();
        assert!(esito.is_err());
    }
    let _ = ricodifica_estranea(&png(10, 10)).expect("e un'altra decodifica parte");
}

#[test]
fn un_decoder_che_cade_non_porta_con_se_chi_lo_chiama() {
    // Reperto S8-7: nessuna `catch_unwind` attorno ai decoder, e il filo che le
    // serve (quello delle copertine di «Ascolta insieme») moriva col primo input
    // che li faceva cadere. Qui cade il lavoro dopo la decodifica, che passa dallo
    // stesso punto: il panico si dice come un errore, e il posto si rende.
    let esito: Result<(), AppError> = con_immagine_estranea(&png(10, 10), "estranea", |_| {
        panic!("il decoder cade");
    });
    let errore = esito.expect_err("un errore, non un panico");
    assert_eq!(
        errore.code().kind(),
        rymembra_domain::errors::ErrorCodeKind::MetadataTagReadFailed
    );
    assert!(errore.to_string().contains("caduto"), "{errore}");
    // E i posti sono tutti liberi: ne servono due insieme, di nuovo.
    for _ in 0..(DECODIFICHE_ESTRANEE_INSIEME * 3) {
        ricodifica_estranea(&png(10, 10)).expect("una decodifica parte");
    }
}

#[test]
fn le_copertine_degli_altri_sono_solo_immagini() {
    // Decisione D-32: JPEG, PNG e WebP, riconosciuti dai byte.
    assert_eq!(
        tipo_immagine(&[0xFF, 0xD8, 0xFF, 0xE0, 0]),
        Some("image/jpeg")
    );
    assert_eq!(tipo_immagine(b"\x89PNG\r\n\x1a\n\0\0"), Some("image/png"));
    assert_eq!(tipo_immagine(b"RIFF\x10\0\0\0WEBPVP8 "), Some("image/webp"));
    for altro in [
        b"".as_slice(),
        b"\xFF\xD8",
        b"<html><script>x</script></html>",
        b"GIF89a",
        b"RIFF\x10\0\0\0WAVEfmt ",
        b"RIFF",
        b"%PDF-1.7",
    ] {
        assert_eq!(tipo_immagine(altro), None, "{altro:?}");
    }
}

// ── lo spazio delle copertine degli altri ───────────────────────────────────

/// Mette nello store una copertina «di un altro dispositivo» da `peso` byte, senza
/// scriverli: il file è lungo quanto serve e il resto è zero.
fn riempi_di_estranee(store: &CoverStore, impronta: &str, peso: u64) {
    let piena = store.path_for(impronta);
    std::fs::create_dir_all(piena.parent().expect("la sottocartella")).expect("cartella");
    let file = std::fs::File::create(&piena).expect("file");
    file.set_len(peso).expect("lunghezza");
    std::fs::write(store.percorso_segno(impronta), b"").expect("segno");
}

#[test]
fn le_copertine_degli_altri_hanno_un_tetto_di_spazio() {
    // Reperto S8-9: un catalogo ostile con duecentomila impronte diverse e
    // immagini rumorose riempiva il disco quanto l'utente scorreva la libreria, e
    // niente le toglieva finché la riga esisteva.
    let dir = tempfile::tempdir().expect("cartella");
    let store = CoverStore::open(dir.path()).expect("store");
    let vecchia = "11".repeat(32);
    riempi_di_estranee(&store, &vecchia, SPAZIO_MASSIMO_ESTRANEE - 1000);
    let nuova = "22".repeat(32);
    let errore = store
        .salva_estranea(&nuova, &media_di_un_altro_di(320))
        .expect_err("lo spazio è finito");
    assert!(
        errore.to_string().contains("spazio"),
        "il motivo si dice: {errore}"
    );
    assert!(!store.path_for(&nuova).exists(), "e non lascia niente");
    assert!(!store.ha_spazio_per_estranee(5000));
    assert!(store.ha_spazio_per_estranee(500));

    // Una copertina **di qui** non conta, e non è fermata: il tetto è per quelle
    // degli altri.
    let locale = store
        .store(&png(300, 300), CoverSource::Tag)
        .expect("la scansione salva le sue");
    assert!(store.path_for(&locale.hash).exists());

    // Dimenticare il dispositivo libera lo spazio, e le nuove tornano a entrare.
    let _ = store.togli_tutte(std::slice::from_ref(&vecchia));
    store
        .salva_estranea(&nuova, &media_di_un_altro_di(320))
        .expect("lo spazio è tornato");
    assert!(store.path_for(&nuova).exists());
}

#[test]
fn lo_spazio_delle_copertine_degli_altri_si_conta_da_quel_che_c_e_sul_disco() {
    // Un riavvio non azzera il conto: si rifà dai segni lasciati dalle copertine
    // prese da fuori.
    let dir = tempfile::tempdir().expect("cartella");
    {
        let store = CoverStore::open(dir.path()).expect("store");
        riempi_di_estranee(&store, &"33".repeat(32), SPAZIO_MASSIMO_ESTRANEE - 10);
    }
    let riaperto = CoverStore::open(dir.path()).expect("store");
    assert!(!riaperto.ha_spazio_per_estranee(u64::try_from(MASSIMO_ESTRANEA).expect("un numero")));
    assert!(riaperto.ha_spazio_per_estranee(1));
}

// ── le misure ───────────────────────────────────────────────────────────────

/// Il picco di memoria del processo fin qui, in byte: (working set, commit).
/// `None` dove non si sa chiederlo. Vedi `rymembra_lontano::protocollo::misure`.
fn picco() -> Option<(u64, u64)> {
    if cfg!(windows) {
        let pid = std::process::id();
        let comando = format!(
            "$p = Get-Process -Id {pid}; \"$($p.PeakWorkingSet64) $($p.PeakPagedMemorySize64)\""
        );
        let fuori = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &comando])
            .output()
            .ok()?;
        let testo = String::from_utf8_lossy(&fuori.stdout).into_owned();
        let mut numeri = testo
            .split_whitespace()
            .filter_map(|n| n.parse::<u64>().ok());
        Some((numeri.next()?, numeri.next()?))
    } else {
        let stato = std::fs::read_to_string("/proc/self/status").ok()?;
        let kb = |chiave: &str| -> Option<u64> {
            let riga = stato.lines().find(|r| r.starts_with(chiave))?;
            Some(riga.split_whitespace().nth(1)?.parse::<u64>().ok()? * 1024)
        };
        Some((kb("VmHWM:")?, kb("VmPeak:")?))
    }
}

fn mib(byte: u64) -> f64 {
    byte as f64 / (1024.0 * 1024.0)
}

/// `ricodifica_estranea` su `byte`, col suo costo.
fn misura(nome: &str, byte: &[u8]) {
    let prima = picco();
    let inizio = Instant::now();
    let esito = ricodifica_estranea(byte);
    let tempo = inizio.elapsed();
    let dopo = picco();
    let come = match &esito {
        Ok(fuori) => format!("Ok({} byte JPEG)", fuori.len()),
        Err(err) => format!("Err({})", err.cause().unwrap_or("senza causa")),
    };
    println!(
        "MISURA {nome}: {} byte in ingresso, {come}, {} ms",
        byte.len(),
        tempo.as_millis()
    );
    if let (Some((ws0, cm0)), Some((ws1, cm1))) = (prima, dopo) {
        println!(
            "MISURA {nome}: picco working set {:.1} -> {:.1} MiB (+{:.1}), picco commit {:.1} -> {:.1} MiB (+{:.1})",
            mib(ws0),
            mib(ws1),
            mib(ws1.saturating_sub(ws0)),
            mib(cm0),
            mib(cm1),
            mib(cm1.saturating_sub(cm0)),
        );
    }
}

/// La copertina ostile: JPEG progressiva 8192×8192, 4:4:4, sotto il mebibyte.
#[test]
#[ignore = "misura: si lancia a mano, una per volta"]
fn misura_jpeg_progressiva_8192() {
    let jpeg = jpeg_progressiva_piatta(8192);
    misura("jpeg progressiva 8192x8192", &jpeg);
}

/// Il peggio che resta ammesso: la stessa, 1024×1024.
#[test]
#[ignore = "misura: si lancia a mano, una per volta"]
fn misura_jpeg_progressiva_1024() {
    let jpeg = jpeg_progressiva_piatta(1024);
    misura("jpeg progressiva 1024x1024", &jpeg);
}

/// Una copertina vera: la media da 320 pixel che un proprietario serve.
#[test]
#[ignore = "misura: si lancia a mano, una per volta"]
fn misura_la_media_vera_da_320() {
    let jpeg = media_di_un_altro_di(320);
    misura("media 320x320", &jpeg);
}
