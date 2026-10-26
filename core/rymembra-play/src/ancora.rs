//! L'ancora: quale fotogramma di musica si sente, e in quale istante.
//!
//! # Perché non basta la posizione
//!
//! Perché [`crate::Posizione`] risponde a «dove siamo», e lo dice con il passo del
//! filo della decodifica — quattro millisecondi — e in millisecondi interi. Per
//! stare a tempo con un altro computer serve un'altra domanda: **quale
//! fotogramma del brano arriva all'orecchio in quale istante**, con l'istante
//! su un orologio che si possa confrontare ([`crate::tempo`]). Una coppia così
//! la può scrivere solo la callback, che è l'unica a sapere insieme quale
//! fotogramma sta consegnando e — da `cpal` — quando il primo campione del
//! blocco suonerà.
//!
//! # Due metà, scritte da due fili
//!
//! La callback non sa cos'è un brano: conta fotogrammi d'anello. Quindi
//! pubblica un'ancora **grezza** — «il fotogramma numero tanti di questa
//! epoca si sente a quest'istante» — e basta. La traduzione in brano, etichetta
//! e microsecondi la fa chi legge, con la [`Mappa`] che il filo della
//! decodifica pubblica ogni volta che i suoi segni cambiano. L'**epoca** tiene
//! insieme le due metà: cresce a ogni svuotamento dell'anello, quando il
//! conteggio dei fotogrammi ricomincia da zero, e un'ancora di un'epoca non si
//! legge mai attraverso la mappa di un'altra.
//!
//! # Perché un sigillo e non un lucchetto
//!
//! Perché chi scrive è la callback, che non può prendere lucchetti, e i numeri
//! da pubblicare sono cinque: scritti uno per uno in cinque atomici, chi legge
//! potrebbe prenderne tre di un blocco e due del blocco dopo. Il [`Sigillo`] è
//! un *seqlock*: un contatore dispari mentre si scrive, pari a scrittura
//! finita, e chi legge tiene la lettura solo se il contatore era pari e non è
//! cambiato nel frattempo. Chi scrive non aspetta mai nessuno; chi legge, al
//! peggio, riprova.

use std::sync::atomic::{AtomicU64, Ordering, fence};

/// Quante volte chi legge riprova prima di arrendersi.
///
/// Otto: la callback scrive un'ancora per blocco, cioè ogni dieci millisecondi,
/// e una scrittura dura qualche nanosecondo. Trovarla a metà due volte di fila
/// è già improbabile; otto vuol dire che lo scrittore è fermo **dentro** la
/// scrittura — il filo audio sospeso dallo scheduler — e aspettarlo sarebbe
/// esattamente l'attesa che il sigillo esiste per non avere.
const TENTATIVI: u32 = 8;

/// Il bit di [`Sigillo::epoca_e_bandiere`] che dice «la musica era voluta».
const BIT_IN_CORSO: u64 = 1 << 32;

/// Il bit che dice «la latenza misurata era assurda, e si è usata la callback».
const BIT_LATENZA_SCARTATA: u64 = 1 << 33;

/// L'ancora come la scrive la callback: fotogrammi d'anello e istanti, senza
/// sapere niente di brani.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct AncoraGrezza {
    /// Il numero del fotogramma d'anello, nell'epoca, a cui l'ancora si
    /// riferisce: lo stesso conteggio di `Condiviso::fotogrammi`.
    pub contenuto: u64,
    /// Quando quel fotogramma lascia il dispositivo, in nanosecondi di
    /// [`crate::tempo`]. **Senza** la latenza dichiarata a mano: la aggiunge chi
    /// legge, che la conosce dalla [`Mappa`].
    pub udito_ns: u64,
    /// `playback − callback` del blocco, come lo dice `cpal`.
    pub anticipo_ns: u64,
    /// Lo stesso istante di `udito_ns`, ricavato con l'altro metodo: l'orologio
    /// del processo letto all'ingresso della callback, più l'anticipo. Serve
    /// solo a misurare quanto i due metodi si scostano.
    pub controprova_ns: u64,
    /// L'epoca dell'anello in cui il fotogramma è stato contato.
    pub epoca: u32,
    /// La musica era voluta: né pausa né svuotamento. Un'ancora di un blocco
    /// che sta sfumando verso la pausa è vera, ma non dice dove la musica
    /// continuerà.
    pub in_corso: bool,
    /// L'anticipo superava [`crate::uscita::LATENZA_MASSIMA_MS`]: l'istante è
    /// quello della callback e non quello di `playback`. Stessa regola di
    /// `Contesto::ritardo_fotogrammi`.
    pub latenza_scartata: bool,
}

/// Il sigillo: l'ultima [`AncoraGrezza`], pubblicata senza lucchetti.
///
/// Uno scrittore solo — la callback — e quanti lettori si vuole.
#[derive(Debug, Default)]
pub(crate) struct Sigillo {
    /// Dispari mentre si scrive, pari a scrittura finita, zero se non si è
    /// mai scritto niente.
    seq: AtomicU64,
    contenuto: AtomicU64,
    udito_ns: AtomicU64,
    anticipo_ns: AtomicU64,
    /// Un campo in più rispetto ai quattro del contratto, e voluto: la
    /// controprova deve viaggiare **con** l'ancora che verifica, altrimenti il
    /// confronto mescolerebbe due blocchi.
    controprova_ns: AtomicU64,
    /// L'epoca nei 32 bit bassi, le due bandiere sopra: un atomico solo, così
    /// le tre cose non si separano mai.
    epoca_e_bandiere: AtomicU64,
}

impl Sigillo {
    /// Pubblica un'ancora. Solo dalla callback: lo scrittore è uno.
    ///
    /// # Gli ordini
    ///
    /// Il contatore dispari si scrive **prima** dei campi, e la barriera
    /// `Release` in mezzo impedisce che un campo nuovo diventi visibile prima di
    /// lui; il contatore pari si scrive **dopo**, con `Release`, così chi lo
    /// vede pari vede anche tutti i campi. È la ricetta di Boehm per i seqlock
    /// fatti di soli atomici, senza `unsafe` e senza letture strappate: ogni
    /// campo è un atomico, quindi il peggio che può capitare a chi legge è una
    /// lettura **mista**, e quella la scarta il confronto dei due contatori.
    ///
    /// Niente allocazioni, niente lucchetti, niente attese: sei `store`.
    pub(crate) fn scrivi(&self, ancora: &AncoraGrezza) {
        let seq = self.seq.load(Ordering::Relaxed);
        self.seq.store(seq.wrapping_add(1), Ordering::Relaxed);
        fence(Ordering::Release);
        self.contenuto.store(ancora.contenuto, Ordering::Relaxed);
        self.udito_ns.store(ancora.udito_ns, Ordering::Relaxed);
        self.anticipo_ns
            .store(ancora.anticipo_ns, Ordering::Relaxed);
        self.controprova_ns
            .store(ancora.controprova_ns, Ordering::Relaxed);
        let mut bandiere = u64::from(ancora.epoca);
        if ancora.in_corso {
            bandiere |= BIT_IN_CORSO;
        }
        if ancora.latenza_scartata {
            bandiere |= BIT_LATENZA_SCARTATA;
        }
        self.epoca_e_bandiere.store(bandiere, Ordering::Relaxed);
        self.seq.store(seq.wrapping_add(2), Ordering::Release);
    }

    /// L'ultima ancora pubblicata, se si riesce a leggerla intera.
    ///
    /// `None` se non è mai stata scritta, o se per [`TENTATIVI`] volte lo
    /// scrittore era a metà: chi chiede riprova al giro dopo, che è meglio di un
    /// numero mescolato.
    pub(crate) fn leggi(&self) -> Option<AncoraGrezza> {
        for _ in 0..TENTATIVI {
            let prima = self.seq.load(Ordering::Acquire);
            if prima == 0 {
                return None;
            }
            if prima % 2 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let contenuto = self.contenuto.load(Ordering::Relaxed);
            let udito_ns = self.udito_ns.load(Ordering::Relaxed);
            let anticipo_ns = self.anticipo_ns.load(Ordering::Relaxed);
            let controprova_ns = self.controprova_ns.load(Ordering::Relaxed);
            let bandiere = self.epoca_e_bandiere.load(Ordering::Relaxed);
            // La barriera che fa da specchio a quella dello scrittore: se uno
            // dei campi qui sopra viene da una scrittura cominciata dopo
            // `prima`, la seconda lettura del contatore la vede.
            fence(Ordering::Acquire);
            if self.seq.load(Ordering::Relaxed) != prima {
                std::hint::spin_loop();
                continue;
            }
            return Some(AncoraGrezza {
                contenuto,
                udito_ns,
                anticipo_ns,
                controprova_ns,
                epoca: u32::try_from(bandiere & u64::from(u32::MAX)).unwrap_or(0),
                in_corso: bandiere & BIT_IN_CORSO != 0,
                latenza_scartata: bandiere & BIT_LATENZA_SCARTATA != 0,
            });
        }
        None
    }
}

/// Quale fotogramma di quale brano si sente, e quando.
///
/// La legge `crate::Motore::ancora`. Un'ancora è un fatto passato — un blocco
/// già consegnato — ma i suoi due numeri descrivono una retta: il fotogramma
/// `posizione_us` si sente a `udito_ns`, e quel che viene dopo si sente dopo,
/// al passo della frequenza d'uscita, finché la musica non si ferma.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ancora {
    /// L'etichetta del brano, se ne ha una: vedi
    /// [`crate::BranoAperto::con_etichetta`].
    pub etichetta: Option<u64>,
    /// Quale brano.
    pub track_id: i64,
    /// Quale ascolto: un numero nuovo a ogni inizio di brano, lo stesso per i
    /// salti dentro l'ascolto in corso.
    pub ascolto: u64,
    /// A che punto del brano è il fotogramma dell'ancora, in microsecondi.
    pub posizione_us: u64,
    /// Quando quel fotogramma arriva all'orecchio, in nanosecondi di
    /// [`crate::tempo`]: latenza dichiarata a mano compresa.
    pub udito_ns: u64,
    /// La frequenza d'uscita: il passo con cui la retta avanza.
    pub frequenza: u32,
    /// L'epoca dell'anello: cambia a ogni salto e a ogni brano scelto a mano.
    pub epoca: u32,
    /// La misura della latenza era assurda e l'istante viene dalla callback:
    /// vedi [`crate::uscita::LATENZA_MASSIMA_MS`].
    pub latenza_scartata: bool,
    /// La musica era voluta quando il blocco è stato consegnato.
    pub in_corso: bool,
    /// Lo stesso istante di `udito_ns`, misurato con l'orologio del processo
    /// all'ingresso della callback. Diagnostica: la differenza fra i due dice
    /// quanto il ponte fra i due orologi regge.
    pub controprova_ns: u64,
}

/// Un segno del filo della decodifica, come lo vede chi traduce le ancore.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SegnoPubblico {
    /// Da quale fotogramma d'anello comincia.
    pub da: u64,
    /// A che punto del brano si è a `da`, in fotogrammi d'uscita.
    pub offset_fotogrammi: u64,
    pub track_id: i64,
    pub ascolto: u64,
    pub etichetta: Option<u64>,
}

/// La traduzione da fotogrammi d'anello a brani, per un'epoca.
///
/// La pubblica il filo della decodifica, dietro un lucchetto, **solo quando
/// cambia**: i segni cambiano a ogni brano e a ogni salto, non a ogni giro.
#[derive(Debug, Clone, Default)]
pub(crate) struct Mappa {
    /// L'epoca a cui valgono questi segni.
    pub epoca: u32,
    /// La frequenza d'uscita, per i microsecondi.
    pub frequenza: u32,
    /// La latenza della catena dichiarata a mano: si somma agli istanti.
    pub latenza_manuale_ms: i64,
    /// I segni ancora in fila, nell'ordine del filo.
    pub segni: Vec<SegnoPubblico>,
    /// L'ultimo segno tolto dalla fila.
    ///
    /// # Perché serve
    ///
    /// Perché la fila la sfoltisce `Contesto::aggiorna` guardando gli **uditi**,
    /// e con una latenza dichiarata negativa gli uditi stanno **davanti** al
    /// conteggio della callback: il segno di un brano esce dalla fila mentre la
    /// callback sta ancora consegnando i suoi ultimi fotogrammi. Un'ancora di
    /// quei fotogrammi non troverebbe più il suo brano.
    pub tolto: Option<SegnoPubblico>,
}

impl Mappa {
    /// Il segno a cui appartiene un fotogramma d'anello: l'ultimo che comincia
    /// non dopo di lui.
    fn segno_di(&self, contenuto: u64) -> Option<&SegnoPubblico> {
        self.segni
            .iter()
            .rev()
            .find(|s| s.da <= contenuto)
            .or_else(|| self.tolto.as_ref().filter(|s| s.da <= contenuto))
    }

    /// Un'ancora grezza tradotta in brano e microsecondi.
    ///
    /// `None` se l'epoca non è quella della mappa — il salto è appena
    /// successo, da una parte o dall'altra — o se il fotogramma non appartiene
    /// a nessun segno noto.
    pub(crate) fn traduci(&self, grezza: &AncoraGrezza) -> Option<Ancora> {
        if grezza.epoca != self.epoca {
            return None;
        }
        let segno = self.segno_di(grezza.contenuto)?;
        let latenza_ns = i128::from(self.latenza_manuale_ms).saturating_mul(1_000_000);
        Some(Ancora {
            etichetta: segno.etichetta,
            track_id: segno.track_id,
            ascolto: segno.ascolto,
            posizione_us: self.posizione_us(segno, grezza.contenuto),
            udito_ns: sposta(grezza.udito_ns, latenza_ns),
            frequenza: self.frequenza,
            epoca: grezza.epoca,
            latenza_scartata: grezza.latenza_scartata,
            in_corso: grezza.in_corso,
            controprova_ns: sposta(grezza.controprova_ns, latenza_ns),
        })
    }

    /// L'etichetta e la posizione del fotogramma d'anello `prossimo`, se
    /// questa mappa vale per `epoca`. È la domanda di chi aspetta in pausa: da
    /// dove ripartirà la musica.
    pub(crate) fn attesa(&self, epoca: u32, prossimo: u64) -> Option<(Option<u64>, u64)> {
        if epoca != self.epoca {
            return None;
        }
        let segno = self.segno_di(prossimo)?;
        Some((segno.etichetta, self.posizione_us(segno, prossimo)))
    }

    /// I microsecondi di brano di un fotogramma d'anello dentro il suo segno.
    ///
    /// Dai fotogrammi e non dai millisecondi del segno: la somma si fa in
    /// fotogrammi esatti e si divide una volta sola, alla fine.
    fn posizione_us(&self, segno: &SegnoPubblico, contenuto: u64) -> u64 {
        let nel_brano = segno
            .offset_fotogrammi
            .saturating_add(contenuto.saturating_sub(segno.da));
        us_da_fotogrammi(nel_brano, self.frequenza)
    }
}

/// Un istante spostato di tanti nanosecondi, col loro segno, senza uscire dal
/// `u64`.
fn sposta(ns: u64, di: i128) -> u64 {
    let spostato = i128::from(ns).saturating_add(di).max(0);
    u64::try_from(spostato).unwrap_or(u64::MAX)
}

/// I microsecondi di tanti fotogrammi, alla frequenza d'uscita.
#[expect(
    clippy::integer_division,
    reason = "il resto è meno di un microsecondo: un ventesimo di fotogramma a \
              quarantottomila al secondo"
)]
pub(crate) fn us_da_fotogrammi(fotogrammi: u64, frequenza: u32) -> u64 {
    let frequenza = u128::from(frequenza.max(1));
    let us = u128::from(fotogrammi).saturating_mul(1_000_000) / frequenza;
    u64::try_from(us).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod prove {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    /// Un'ancora in cui ogni campo è ricavato dallo stesso numero: una lettura
    /// mista si riconosce perché i campi non tornano.
    fn coerente(i: u64) -> AncoraGrezza {
        AncoraGrezza {
            contenuto: i,
            udito_ns: i.wrapping_mul(3),
            anticipo_ns: i.wrapping_mul(5),
            controprova_ns: i.wrapping_mul(7),
            epoca: u32::try_from(i % u64::from(u32::MAX)).unwrap_or(0),
            in_corso: i.is_multiple_of(2),
            latenza_scartata: i.is_multiple_of(3),
        }
    }

    #[test]
    fn un_sigillo_mai_scritto_non_ha_niente_da_dire() {
        assert_eq!(Sigillo::default().leggi(), None);
    }

    #[test]
    fn quel_che_si_scrive_si_rilegge_intero() {
        let sigillo = Sigillo::default();
        for i in [0, 1, 6, u64::from(u32::MAX) - 1] {
            sigillo.scrivi(&coerente(i));
            assert_eq!(sigillo.leggi(), Some(coerente(i)));
        }
    }

    #[test]
    fn due_fili_in_gara_non_producono_mai_una_lettura_mista() {
        // Lo scrittore riscrive senza sosta, il lettore legge senza sosta: ogni
        // lettura che riesce deve essere **una** delle ancore scritte, mai un
        // pezzo di una e un pezzo dell'altra.
        let sigillo = Arc::new(Sigillo::default());
        sigillo.scrivi(&coerente(0));
        let basta = Arc::new(AtomicBool::new(false));
        let scrittore = {
            let sigillo = Arc::clone(&sigillo);
            let basta = Arc::clone(&basta);
            std::thread::spawn(move || {
                let mut i = 1u64;
                while !basta.load(Ordering::Acquire) {
                    sigillo.scrivi(&coerente(i));
                    i = i.wrapping_add(1);
                }
            })
        };
        let mut riuscite = 0u32;
        for _ in 0..200_000 {
            if let Some(letta) = sigillo.leggi() {
                assert_eq!(letta, coerente(letta.contenuto), "lettura mista");
                riuscite += 1;
            }
        }
        basta.store(true, Ordering::Release);
        let _ = scrittore.join();
        assert!(riuscite > 0, "nessuna lettura è mai riuscita");
    }

    #[test]
    fn con_lo_scrittore_fermo_a_meta_la_lettura_si_arrende() {
        // Il filo audio sospeso dentro la scrittura: il contatore resta
        // dispari. Chi legge non aspetta — dopo otto tentativi dice di non
        // sapere, e riproverà al giro dopo.
        let sigillo = Sigillo::default();
        sigillo.scrivi(&coerente(4));
        let pari = sigillo.seq.load(Ordering::Relaxed);
        sigillo.seq.store(pari + 1, Ordering::Relaxed);
        sigillo.contenuto.store(999, Ordering::Relaxed);
        assert_eq!(sigillo.leggi(), None);
        // Finita la scrittura, si legge di nuovo.
        sigillo.seq.store(pari + 2, Ordering::Release);
        assert_eq!(sigillo.leggi().map(|a| a.contenuto), Some(999));
    }

    fn segno(da: u64, offset_fotogrammi: u64, track_id: i64) -> SegnoPubblico {
        SegnoPubblico {
            da,
            offset_fotogrammi,
            track_id,
            ascolto: u64::try_from(track_id).unwrap_or(0),
            etichetta: Some(u64::try_from(track_id).unwrap_or(0) * 10),
        }
    }

    #[test]
    fn un_ancora_si_traduce_nel_segno_che_la_contiene() {
        let mappa = Mappa {
            epoca: 3,
            frequenza: 48_000,
            latenza_manuale_ms: 0,
            segni: vec![segno(0, 4_800, 1), segno(48_000, 0, 2)],
            tolto: None,
        };
        let grezza = AncoraGrezza {
            contenuto: 24_000,
            udito_ns: 5_000,
            epoca: 3,
            ..AncoraGrezza::default()
        };
        let ancora = mappa.traduci(&grezza).expect("il fotogramma ha un segno");
        assert_eq!(ancora.track_id, 1);
        // Un decimo di secondo di salto più mezzo secondo di musica.
        assert_eq!(ancora.posizione_us, 600_000);
        assert_eq!(ancora.etichetta, Some(10));
        let dopo = AncoraGrezza {
            contenuto: 48_001,
            ..grezza
        };
        let ancora = mappa.traduci(&dopo).expect("il fotogramma ha un segno");
        assert_eq!((ancora.track_id, ancora.posizione_us), (2, 20));
    }

    #[test]
    fn un_epoca_diversa_non_si_traduce() {
        let mappa = Mappa {
            epoca: 2,
            frequenza: 48_000,
            segni: vec![segno(0, 0, 1)],
            ..Mappa::default()
        };
        let grezza = AncoraGrezza {
            contenuto: 10,
            epoca: 1,
            ..AncoraGrezza::default()
        };
        assert_eq!(mappa.traduci(&grezza), None);
        assert_eq!(mappa.attesa(1, 10), None);
    }

    #[test]
    fn la_latenza_a_mano_sposta_l_istante_col_suo_segno() {
        let mut mappa = Mappa {
            frequenza: 48_000,
            segni: vec![segno(0, 0, 1)],
            ..Mappa::default()
        };
        let grezza = AncoraGrezza {
            udito_ns: 1_000_000_000,
            controprova_ns: 1_000_000_100,
            ..AncoraGrezza::default()
        };
        mappa.latenza_manuale_ms = 120;
        let tardi = mappa.traduci(&grezza).expect("tradotta");
        assert_eq!(tardi.udito_ns, 1_120_000_000);
        assert_eq!(tardi.controprova_ns, 1_120_000_100);
        mappa.latenza_manuale_ms = -120;
        let presto = mappa.traduci(&grezza).expect("tradotta");
        assert_eq!(presto.udito_ns, 880_000_000);
    }
}
