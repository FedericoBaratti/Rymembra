//! Le cornici: come un messaggio diventa byte sul filo, e ritorno.
//!
//! # Due strati
//!
//! Sul TCP viaggiano **messaggi Noise**, ciascuno preceduto dalla sua
//! lunghezza in un `u16` (big-endian): è il tetto del protocollo Noise, 65535
//! byte cifrati, cioè 65519 in chiaro più i 16 dell'etichetta d'autenticità.
//!
//! Dentro, in chiaro, viaggiano i **messaggi del trasporto**: un `u32` di
//! lunghezza del carico, un `u8` di [`Tipo`], e il carico. Un messaggio può
//! essere lungo fino a [`MASSIMO_MESSAGGIO`], quindi si spezza su più messaggi
//! Noise: la testa sta sempre nel primo, e un messaggio Noise non porta mai
//! pezzi di due messaggi diversi. Così chi riceve sa sempre dove comincia il
//! prossimo, e un pezzo che sborda è un errore e non un'ambiguità.
//!
//! # Il controllo prima dell'allocazione
//!
//! La lunghezza annunciata si confronta col tetto **prima** di riservare la
//! memoria. Il contrario — riservare e poi guardare — vorrebbe dire che una
//! testa di cinque byte con scritto quattro gigabyte basta a far cadere il
//! processo di chi la riceve, ed è proprio la cosa che un tetto esiste per
//! impedire. Vedi [`Ricompositore::aggiungi`].
//!
//! # Niente base64
//!
//! I byte dei brani viaggiano come [`Tipo::Byte`], allegati a un messaggio
//! JSON che li annuncia: in base64 sarebbero un terzo in più sul filo, e una
//! codifica e una decodifica per ogni finestra di due megabyte.
//!
//! # La compressione
//!
//! Un messaggio JSON oltre [`SOGLIA_COMPRESSIONE`] viaggia compresso
//! (deflate grezzo, [`LIVELLO`]) come [`Tipo::JsonCompresso`]; tutti gli altri
//! viaggiano come sempre, senza un byte né un microsecondo in più.
//!
//! Perché: lo stato della sessione (vedi `Messaggio::Stato`) porta tutta la
//! finestra della coda, mille voci, e misurato su un collegamento come un
//! Wi-Fi (20 Mbit/s, 5 ± 15 ms) sono 330–580 KB di JSON, a ogni membro, fino
//! a uno al secondo mentre l'host prepara le voci e a ogni cambio della coda.
//! Sulla connessione di controllo le risposte ai gesti e i pong dell'orologio
//! gli stanno dietro: un `Esito` tornava in 300–460 ms invece di 50, e a
//! 5 Mbit/s i membri non arrivavano mai a una stima dell'orologio. Lo stesso
//! stato compresso è fra un quinto e un decimo (582 KB → 99 KB al livello 1
//! nelle misure della revisione, 603 KB → 118 KB al livello scelto con le
//! voci di prova di questo crate, che hanno più impronte e gettoni a caso):
//! il JSON ripete i nomi dei campi a ogni voce.
//!
//! Chi riceve decomprime **con un tetto**: quello dei messaggi JSON di quel
//! capo, e la decompressione si ferma appena lo passerebbe, prima di
//! riservare memoria oltre ([`gonfia`]) — una «bomba», pochi kilobyte che
//! diventano gigabyte, chiude con `insieme.protocollo` come una testa che
//! annuncia troppo. E da un estraneo un messaggio compresso non si accetta
//! affatto: il suo tetto di quattro kibibyte non deve poter essere aggirato
//! comprimendo. Il tetto del messaggio compresso, sul filo, è quello di
//! sempre, [`MASSIMO_MESSAGGIO`].
//!
//! L'ordine dei messaggi non cambia: compresso o no, un messaggio prende il
//! suo posto nella stessa coda e sullo stesso flusso.

/// Il tetto di un messaggio Noise, etichetta compresa.
pub(crate) const MASSIMO_NOISE: usize = 65_535;

/// L'etichetta d'autenticità di ChaChaPoly, in coda a ogni messaggio Noise.
pub(crate) const ETICHETTA: usize = 16;

/// Quanti byte in chiaro stanno in un messaggio Noise.
pub(crate) const CHIARO_MASSIMO: usize = MASSIMO_NOISE - ETICHETTA;

/// Il tetto del carico di un messaggio del trasporto: un mebibyte.
///
/// Basta allo stato di una sessione di mille voci (un centinaio di kilobyte)
/// con un margine largo, e a una copertina «piena». I byte dei brani ne usano
/// più d'uno per lettura (vedi `servente::LETTURA_MASSIMA`).
pub(crate) const MASSIMO_MESSAGGIO: usize = 1024 * 1024;

/// La testa: `u32` di lunghezza e `u8` di tipo.
const TESTA: usize = 5;

/// Che cosa porta un messaggio del trasporto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tipo {
    /// Un oggetto JSON: lo strato del trasporto, e dentro i `Messaggio`.
    Json,
    /// Byte grezzi: l'allegato di un `Pezzo` o di un'`Immagine`.
    Byte,
    /// Un oggetto JSON compresso: vedi «La compressione». Chi legge una
    /// connessione lo riceve già decompresso, come [`Tipo::Json`].
    JsonCompresso,
}

impl Tipo {
    /// Il numero sul filo.
    const fn numero(self) -> u8 {
        match self {
            Self::Json => 1,
            Self::Byte => 2,
            Self::JsonCompresso => 3,
        }
    }

    /// Dal numero sul filo; `None` per un tipo che questo PC non conosce.
    ///
    /// Non è un «sconosciuto, si ignora» come per i messaggi JSON: un tipo di
    /// cornice nuovo cambierebbe il modo di leggere i byte che seguono, e
    /// leggerli a caso sarebbe peggio che chiudere.
    const fn da_numero(numero: u8) -> Option<Self> {
        match numero {
            1 => Some(Self::Json),
            2 => Some(Self::Byte),
            3 => Some(Self::JsonCompresso),
            _ => None,
        }
    }
}

/// Da quanti byte di JSON in su un messaggio si comprime: otto kibibyte.
///
/// Sotto, il messaggio è un'ancora, un esito, un ping, un comando: pochi
/// byte, che compressi non diventerebbero più corti di quanto costa
/// comprimerli, e che devono partire subito. Sopra c'è lo stato della
/// sessione. Ed è più alta del tetto di un estraneo (`TETTO_ESTRANEO`, quattro
/// kibibyte): un messaggio che un estraneo non potrebbe ricevere compresso non
/// lo potrebbe ricevere nemmeno in chiaro.
pub(crate) const SOGLIA_COMPRESSIONE: usize = 8 * 1024;

/// Il livello di deflate: 3.
///
/// Misurato in release su uno stato di 971 voci, 603 KB di JSON (la prova
/// `si_misura_quanto_comprime_uno_stato`, il migliore di tre giri), coi due
/// motori che `flate2` può avere sotto:
///
/// | livello | `zlib-rs` (il desktop) | `miniz_oxide` (le prove) |
/// |---|---|---|
/// | 1 | 180 KB in 1,0 ms | 128 KB in 1,3 ms |
/// | 3 | 118 KB in 2,2 ms | 124 KB in 4,5 ms |
/// | 6 | 113 KB in 3,5 ms | 119 KB in 7,9 ms |
///
/// Conta il primo, che è quello del programma vero: al livello 1 `zlib-rs`
/// non cerca le corrispondenze pigre e lascia un terzo di byte in più, e
/// sessanta kilobyte a 5 Mbit/s sono cento millisecondi per membro, contro
/// uno di processore. Il 6 stringe ancora un po', e costa il tempo di
/// un'altra compressione. Decomprimere costa meno di un millisecondo a tutti
/// i livelli.
pub(crate) const LIVELLO: u32 = 3;

/// Il JSON compresso, se conviene: oltre [`SOGLIA_COMPRESSIONE`] e solo se
/// viene davvero più corto. `None` vuol dire «si manda in chiaro».
pub(crate) fn comprimi(json: &[u8]) -> Option<Vec<u8>> {
    comprimi_al_livello(json, LIVELLO)
}

/// [`comprimi`] a un livello qualunque: serve alle misure.
pub(crate) fn comprimi_al_livello(json: &[u8], livello: u32) -> Option<Vec<u8>> {
    use std::io::Write as _;
    if json.len() <= SOGLIA_COMPRESSIONE {
        return None;
    }
    let mut compressore = flate2::write::DeflateEncoder::new(
        // Un quarto: è circa quel che viene da uno stato.
        Vec::with_capacity(json.len() >> 2),
        flate2::Compression::new(livello),
    );
    compressore.write_all(json).ok()?;
    let compresso = compressore.finish().ok()?;
    (compresso.len() < json.len()).then_some(compresso)
}

/// Decomprime un [`Tipo::JsonCompresso`], senza mai passare `tetto` byte.
///
/// La memoria cresce con quel che esce, raddoppiando, e mai oltre `tetto`
/// più un byte: quel byte in più è il modo di sapere che il messaggio non ci
/// sta, invece di fidarsi di quel che dice. Non è «decomprimi e poi guarda»:
/// è proprio la bomba che quel modo lascerebbe passare.
///
/// Il motore si chiama un passo alla volta senza dirgli che l'ingresso è
/// finito: è il solo modo che si comporta allo stesso modo con i due motori
/// che `flate2` può avere sotto (`miniz_oxide` nelle prove di questo crate,
/// `zlib-rs` nel desktop, che lo accende per altre dipendenze). La fine è
/// buona solo se arriva la fine del flusso deflate, e con tutto l'ingresso
/// consumato.
///
/// # Errori
///
/// `lunghezza` oltre il tetto; `compresso` per un flusso rotto o monco;
/// `eccedenza` per byte dopo la fine del flusso.
pub(crate) fn gonfia(compresso: &[u8], tetto: usize) -> Result<Vec<u8>, Storta> {
    use flate2::{Decompress, FlushDecompress, Status};
    let limite = tetto.saturating_add(1);
    let mut motore = Decompress::new(false);
    let mut fuori: Vec<u8> = Vec::new();
    loop {
        if fuori.len() == fuori.capacity() {
            if fuori.capacity() >= limite {
                return Err(Storta("lunghezza"));
            }
            let voluta = fuori
                .capacity()
                .saturating_mul(2)
                .max(compresso.len().saturating_mul(4))
                .max(1024)
                .min(limite);
            fuori.reserve_exact(voluta.saturating_sub(fuori.len()));
        }
        let consumati = usize::try_from(motore.total_in()).map_err(|_| Storta("compresso"))?;
        let resto = compresso.get(consumati..).ok_or(Storta("compresso"))?;
        let prima = (motore.total_in(), motore.total_out());
        let stato = motore
            .decompress_vec(resto, &mut fuori, FlushDecompress::None)
            .map_err(|_| Storta("compresso"))?;
        if fuori.len() > tetto {
            return Err(Storta("lunghezza"));
        }
        match stato {
            Status::StreamEnd => break,
            Status::Ok | Status::BufError => {
                // Nessun passo avanti con posto libero in uscita: l'ingresso è
                // finito prima del flusso.
                let fermo = prima == (motore.total_in(), motore.total_out());
                if fermo && fuori.len() < fuori.capacity() {
                    return Err(Storta("compresso"));
                }
            }
        }
    }
    if usize::try_from(motore.total_in()).ok() != Some(compresso.len()) {
        return Err(Storta("eccedenza"));
    }
    Ok(fuori)
}

/// Perché una sequenza di cornici non è un messaggio: il punto, e nient'altro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Storta(pub(crate) &'static str);

/// I pezzi in chiaro di un messaggio, ciascuno da cifrare in un messaggio Noise.
///
/// # Errori
///
/// Un carico oltre [`MASSIMO_MESSAGGIO`]: l'altro capo lo rifiuterebbe e
/// chiuderebbe la connessione, quindi non si manda.
pub(crate) fn spezza(tipo: Tipo, carico: &[u8]) -> Result<Vec<Vec<u8>>, Storta> {
    if carico.len() > MASSIMO_MESSAGGIO {
        return Err(Storta("lunghezza"));
    }
    let lunghezza = u32::try_from(carico.len()).map_err(|_| Storta("lunghezza"))?;
    let (subito, resto) = carico
        .split_at_checked(carico.len().min(CHIARO_MASSIMO - TESTA))
        .unwrap_or((carico, &[]));
    let mut primo = Vec::with_capacity(TESTA + subito.len());
    primo.extend_from_slice(&lunghezza.to_be_bytes());
    primo.push(tipo.numero());
    primo.extend_from_slice(subito);
    let mut pezzi = Vec::with_capacity(1 + resto.len().div_ceil(CHIARO_MASSIMO));
    pezzi.push(primo);
    pezzi.extend(resto.chunks(CHIARO_MASSIMO).map(<[u8]>::to_vec));
    Ok(pezzi)
}

/// Rimette insieme i messaggi del trasporto dai messaggi Noise decifrati.
#[derive(Debug)]
pub(crate) struct Ricompositore {
    /// Il messaggio a metà: tipo, lunghezza annunciata, byte arrivati.
    in_corso: Option<(Tipo, usize, Vec<u8>)>,
    /// Il tetto dei messaggi JSON che si accettano: [`MASSIMO_MESSAGGIO`], o
    /// meno.
    tetto_json: usize,
    /// Il tetto dei messaggi di byte grezzi, idem.
    tetto_byte: usize,
    /// Il tetto dei messaggi JSON compressi, **sul filo**: [`MASSIMO_MESSAGGIO`]
    /// o zero, che vuol dire «compressi no». Decompressi, valgono per loro
    /// `tetto_json`: vedi [`gonfia`].
    tetto_compresso: usize,
}

impl Default for Ricompositore {
    fn default() -> Self {
        Self {
            in_corso: None,
            tetto_json: MASSIMO_MESSAGGIO,
            tetto_byte: MASSIMO_MESSAGGIO,
            tetto_compresso: MASSIMO_MESSAGGIO,
        }
    }
}

impl Ricompositore {
    /// Abbassa (o rialza, fino a [`MASSIMO_MESSAGGIO`]) i tetti dei messaggi
    /// che cominciano da qui in avanti, uno per tipo.
    ///
    /// Serve ai PC non fidati: da loro arrivano il saluto e l'associazione,
    /// pochi byte di JSON, e un mebibyte di JSON che `serde` gonfia
    /// ventiquattro volte nel leggerlo (misurato dalla revisione del 26
    /// ottobre: 24 MiB per un messaggio) sarebbe memoria data a uno
    /// sconosciuto. I byte grezzi non si gonfiano, ma da chi non può mandarne
    /// non si accettano nemmeno. Vedi `collegamento::TETTO_ESTRANEO`. Lo
    /// stesso per i JSON compressi: da un estraneo zero, vedi «La
    /// compressione».
    pub(crate) fn imposta_tetti(&mut self, json: usize, byte: usize, compresso: usize) {
        self.tetto_json = json.min(MASSIMO_MESSAGGIO);
        self.tetto_byte = byte.min(MASSIMO_MESSAGGIO);
        self.tetto_compresso = compresso.min(MASSIMO_MESSAGGIO);
    }

    /// Il tetto dei messaggi JSON di adesso: è anche quello di un JSON
    /// compresso, una volta decompresso. I tetti cambiano solo fra un
    /// messaggio e l'altro, quindi quello di adesso è quello che c'era quando
    /// è arrivata la testa.
    pub(crate) const fn tetto_json(&self) -> usize {
        self.tetto_json
    }

    /// Il prossimo pezzo comincia un messaggio nuovo, con la sua testa: è il
    /// momento di decidere i tetti. Vedi `Decifratore::prossimo_coi_tetti`.
    pub(crate) const fn fra_due_messaggi(&self) -> bool {
        self.in_corso.is_none()
    }

    /// Aggiunge il chiaro di un messaggio Noise. Restituisce il messaggio se
    /// questo pezzo lo completa.
    ///
    /// # Errori
    ///
    /// Una testa monca, un tipo sconosciuto, una lunghezza oltre il tetto, un
    /// pezzo che sborda o uno vuoto a metà messaggio. Ognuno chiude la
    /// connessione con `insieme.protocollo`: dopo un errore di cornice non si
    /// sa più dove comincia il messaggio dopo.
    pub(crate) fn aggiungi(&mut self, chiaro: &[u8]) -> Result<Option<(Tipo, Vec<u8>)>, Storta> {
        let Some((tipo, attesi, mut carico)) = self.in_corso.take() else {
            let (testa, resto) = chiaro.split_first_chunk::<TESTA>().ok_or(Storta("testa"))?;
            let [a, b, c, d, numero] = *testa;
            let attesi = usize::try_from(u32::from_be_bytes([a, b, c, d]))
                .map_err(|_| Storta("lunghezza"))?;
            let tipo = Tipo::da_numero(numero).ok_or(Storta("tipo"))?;
            let tetto = match tipo {
                Tipo::Json => self.tetto_json,
                Tipo::Byte => self.tetto_byte,
                Tipo::JsonCompresso => self.tetto_compresso,
            };
            // Qui, prima di `with_capacity`: vedi «Il controllo prima
            // dell'allocazione» nel `//!` del modulo.
            if attesi > tetto {
                return Err(Storta("lunghezza"));
            }
            if resto.len() > attesi {
                return Err(Storta("eccedenza"));
            }
            let mut carico = Vec::with_capacity(attesi);
            carico.extend_from_slice(resto);
            return Ok(self.forse_finito(tipo, attesi, carico));
        };
        // Un messaggio Noise vuoto a metà non porta niente e non serve a
        // niente: chi lo manda non sta parlando questo protocollo.
        if chiaro.is_empty() {
            return Err(Storta("vuoto"));
        }
        if carico.len().saturating_add(chiaro.len()) > attesi {
            return Err(Storta("eccedenza"));
        }
        carico.extend_from_slice(chiaro);
        Ok(self.forse_finito(tipo, attesi, carico))
    }

    /// Il messaggio se è completo; altrimenti lo mette da parte.
    fn forse_finito(
        &mut self,
        tipo: Tipo,
        attesi: usize,
        carico: Vec<u8>,
    ) -> Option<(Tipo, Vec<u8>)> {
        if carico.len() == attesi {
            return Some((tipo, carico));
        }
        self.in_corso = Some((tipo, attesi, carico));
        None
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Il carico rimesso insieme dai suoi pezzi, come lo farebbe chi riceve.
    fn ricomponi(pezzi: &[Vec<u8>]) -> Result<Vec<(Tipo, Vec<u8>)>, Storta> {
        let mut ricompositore = Ricompositore::default();
        let mut messaggi = Vec::new();
        for pezzo in pezzi {
            if let Some(messaggio) = ricompositore.aggiungi(pezzo)? {
                messaggi.push(messaggio);
            }
        }
        Ok(messaggi)
    }

    #[test]
    fn un_mebibyte_passa_spezzato_e_si_ricompone_uguale() {
        let carico: Vec<u8> = (0..MASSIMO_MESSAGGIO)
            .map(|i| u8::try_from(i % 251).unwrap_or(0))
            .collect();
        let pezzi = spezza(Tipo::Byte, &carico).expect("un mebibyte sta nel tetto");
        assert!(pezzi.len() > 16, "il carico va spezzato: {}", pezzi.len());
        assert!(
            pezzi.iter().all(|p| p.len() <= CHIARO_MASSIMO),
            "ogni pezzo sta in un messaggio Noise"
        );
        let messaggi = ricomponi(&pezzi).expect("si ricompone");
        assert_eq!(messaggi, vec![(Tipo::Byte, carico)]);
    }

    #[test]
    fn un_mebibyte_e_un_byte_non_si_manda() {
        let carico = vec![0_u8; MASSIMO_MESSAGGIO + 1];
        assert_eq!(spezza(Tipo::Byte, &carico), Err(Storta("lunghezza")));
    }

    #[test]
    fn una_testa_che_annuncia_un_byte_di_troppo_si_rifiuta_prima_di_allocare() {
        // La testa da sola, senza un byte del carico: se l'errore arriva qui
        // vuol dire che la memoria non è mai stata chiesta.
        let mut testa = u32::try_from(MASSIMO_MESSAGGIO + 1)
            .expect("sta in u32")
            .to_be_bytes()
            .to_vec();
        testa.push(1);
        let mut ricompositore = Ricompositore::default();
        assert_eq!(ricompositore.aggiungi(&testa), Err(Storta("lunghezza")));

        // E quattro gigabyte annunciati, lo stesso.
        let mut enorme = u32::MAX.to_be_bytes().to_vec();
        enorme.push(2);
        assert_eq!(
            Ricompositore::default().aggiungi(&enorme),
            Err(Storta("lunghezza"))
        );

        // Mentre un mebibyte esatto si accetta e aspetta il resto.
        let mut giusta = u32::try_from(MASSIMO_MESSAGGIO)
            .expect("sta in u32")
            .to_be_bytes()
            .to_vec();
        giusta.push(2);
        assert_eq!(Ricompositore::default().aggiungi(&giusta), Ok(None));
    }

    #[test]
    fn i_confini_del_primo_pezzo_reggono() {
        // Il primo pezzo porta la testa: il carico che ci sta tutto, quello che
        // sborda di un byte, e quello vuoto.
        for lunghezza in [
            0,
            1,
            CHIARO_MASSIMO - TESTA - 1,
            CHIARO_MASSIMO - TESTA,
            CHIARO_MASSIMO - TESTA + 1,
            CHIARO_MASSIMO * 2,
        ] {
            let carico = vec![7_u8; lunghezza];
            let pezzi = spezza(Tipo::Json, &carico).expect("nel tetto");
            assert_eq!(
                ricomponi(&pezzi).expect("si ricompone"),
                vec![(Tipo::Json, carico)],
                "lunghezza {lunghezza}"
            );
        }
    }

    #[test]
    fn due_messaggi_di_seguito_restano_due() {
        let mut pezzi = spezza(Tipo::Json, b"{\"tipo\":\"battito\"}").expect("piccolo");
        pezzi.extend(spezza(Tipo::Byte, &[1, 2, 3]).expect("piccolo"));
        let messaggi = ricomponi(&pezzi).expect("si ricompone");
        assert_eq!(messaggi.len(), 2);
        assert_eq!(messaggi.get(1), Some(&(Tipo::Byte, vec![1, 2, 3])));
    }

    /// `quanti` zeri compressi al livello più stretto, senza tenerli in
    /// memoria: una bomba.
    fn zeri_compressi(quanti: usize) -> Vec<u8> {
        use std::io::Write as _;
        let mut compressore =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
        let blocco = vec![0_u8; 64 * 1024];
        let mut resta = quanti;
        while resta > 0 {
            let ora = resta.min(blocco.len());
            compressore
                .write_all(blocco.get(..ora).expect("dentro"))
                .expect("comprime");
            resta -= ora;
        }
        compressore.finish().expect("finisce")
    }

    #[test]
    fn un_json_piccolo_non_si_comprime_e_uno_grande_va_e_torna() {
        assert_eq!(
            comprimi(&[b'a'; SOGLIA_COMPRESSIONE]),
            None,
            "fino alla soglia"
        );
        assert_eq!(comprimi(b"{\"tipo\":\"battito\"}"), None);
        let grande: Vec<u8> = (0..SOGLIA_COMPRESSIONE * 4)
            .map(|i| b"{\"titolo\":\"brano\"},"[i % 19])
            .collect();
        let compresso = comprimi(&grande).expect("oltre la soglia, e si stringe");
        assert!(compresso.len() * 4 < grande.len(), "{}", compresso.len());
        assert_eq!(gonfia(&compresso, MASSIMO_MESSAGGIO), Ok(grande));
        // Dei byte a caso non si stringono: partono in chiaro.
        let a_caso: Vec<u8> = (0..SOGLIA_COMPRESSIONE * 2)
            .flat_map(|i| *blake3::hash(&u64::try_from(i).unwrap_or(0).to_le_bytes()).as_bytes())
            .take(SOGLIA_COMPRESSIONE * 2)
            .collect();
        assert_eq!(comprimi(&a_caso), None);
    }

    #[test]
    fn una_bomba_si_ferma_al_tetto_senza_riservare_oltre() {
        // Sedici mebibyte di zeri in qualche kibibyte: sotto il tetto del filo,
        // sedici volte quello del JSON decompresso.
        let bomba = zeri_compressi(16 * MASSIMO_MESSAGGIO);
        assert!(bomba.len() < 64 * 1024, "{}", bomba.len());
        assert_eq!(gonfia(&bomba, MASSIMO_MESSAGGIO), Err(Storta("lunghezza")));
        // Al tetto esatto passa, e la memoria non va oltre il tetto e un byte.
        let pieno = gonfia(&zeri_compressi(MASSIMO_MESSAGGIO), MASSIMO_MESSAGGIO)
            .expect("al tetto si accetta");
        assert_eq!(pieno.len(), MASSIMO_MESSAGGIO);
        assert!(
            pieno.capacity() <= MASSIMO_MESSAGGIO + 1,
            "{}",
            pieno.capacity()
        );
        // Un byte in più, no.
        assert_eq!(
            gonfia(&zeri_compressi(MASSIMO_MESSAGGIO + 1), MASSIMO_MESSAGGIO),
            Err(Storta("lunghezza"))
        );
    }

    #[test]
    fn un_flusso_compresso_monco_o_con_la_coda_non_passa() {
        let compresso = zeri_compressi(100_000);
        let monco = compresso.get(..compresso.len() - 1).expect("dentro");
        assert_eq!(gonfia(monco, MASSIMO_MESSAGGIO), Err(Storta("compresso")));
        let mut con_la_coda = compresso.clone();
        con_la_coda.push(0);
        assert_eq!(
            gonfia(&con_la_coda, MASSIMO_MESSAGGIO),
            Err(Storta("eccedenza"))
        );
        assert_eq!(gonfia(b"non deflate", MASSIMO_MESSAGGIO).ok(), None);
        assert_eq!(gonfia(&[], MASSIMO_MESSAGGIO), Err(Storta("compresso")));
    }

    #[test]
    fn senza_tetto_dei_compressi_un_compresso_si_rifiuta_dalla_testa() {
        // Il caso di un estraneo: la testa basta, prima di qualunque byte.
        let mut ricompositore = Ricompositore::default();
        ricompositore.imposta_tetti(4096, 0, 0);
        let pezzi = spezza(Tipo::JsonCompresso, &[1, 2, 3]).expect("piccolo");
        let testa = pezzi.first().expect("uno");
        assert_eq!(ricompositore.aggiungi(testa), Err(Storta("lunghezza")));
        // Con il tetto, lo stesso arriva com'è: la decompressione è di chi legge.
        let mut ricompositore = Ricompositore::default();
        assert_eq!(
            ricompositore.aggiungi(testa),
            Ok(Some((Tipo::JsonCompresso, vec![1, 2, 3])))
        );
    }

    #[test]
    fn le_cornici_storte_si_riconoscono() {
        // Testa monca.
        assert_eq!(
            Ricompositore::default().aggiungi(&[0, 0, 0]),
            Err(Storta("testa"))
        );
        // Tipo che non esiste.
        assert_eq!(
            Ricompositore::default().aggiungi(&[0, 0, 0, 1, 9, 0]),
            Err(Storta("tipo"))
        );
        // Più byte di quelli annunciati, nel primo pezzo.
        assert_eq!(
            Ricompositore::default().aggiungi(&[0, 0, 0, 1, 1, 0, 0]),
            Err(Storta("eccedenza"))
        );
        // Più byte di quelli annunciati, in un pezzo dopo.
        let mut ricompositore = Ricompositore::default();
        assert_eq!(ricompositore.aggiungi(&[0, 0, 0, 3, 2, 1]), Ok(None));
        assert_eq!(ricompositore.aggiungi(&[2, 3, 4]), Err(Storta("eccedenza")));
        // Un pezzo vuoto a metà messaggio.
        let mut ricompositore = Ricompositore::default();
        assert_eq!(ricompositore.aggiungi(&[0, 0, 0, 3, 2, 1]), Ok(None));
        assert_eq!(ricompositore.aggiungi(&[]), Err(Storta("vuoto")));
    }
}
