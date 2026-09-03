//! Riparare il testo che una decodifica sbagliata ha reso illeggibile.
//!
//! # Il guasto, detto per bene
//!
//! Un tag è una sequenza di byte più, forse, la dichiarazione di come vanno
//! letti. ID3v1 quella dichiarazione non ce l'ha proprio — trentadue anni fa
//! andava bene, perché i byte erano ASCII — e chi legge un ID3v1 deve scegliere
//! una codifica per conto suo: `lofty` sceglie Latin-1, che è l'unica scelta
//! difendibile e per il russo, il giapponese e il cinese è sempre sbagliata.
//! ID3v2 la dichiarazione ce l'ha, e i taggatori ci mentono: scrivono
//! «Latin-1» sopra byte che sono UTF-8, perché il campo lo riempiva una
//! libreria che non si è mai posta il problema.
//!
//! Il risultato è che «Сегодня» arriva come `Ð¡ÐµÐ³Ð¾Ð´Ð½Ñ` e «Björk» come
//! `BjÃ¶rk`. Non è un difetto estetico: quel testo finisce in
//! [`crate::keys::TrackKey`], cioè nell'identità che attraversa la
//! sincronizzazione, e lo stesso disco rippato due volte non si riconosce più.
//!
//! # Perché si può riparare
//!
//! Perché la decodifica sbagliata è **reversibile**. Latin-1 e windows-1252
//! sono tabelle da un byte a un carattere: ripercorrerle al contrario ridà
//! esattamente i byte che stavano nel file, e a quel punto si può riprovare con
//! la codifica giusta. Non si indovina niente: si rifà il conto.
//!
//! # Perché è comunque pericoloso, e come ci si difende
//!
//! Perché *qualunque* testo si può ri-decodificare in qualunque codifica a un
//! byte, e ne esce sempre qualcosa. `ÅÄÖ` — che è un nome vero — riletto in
//! windows-1251 diventa `ЕДЦ`, che sembra una parola russa quanto l'originale
//! sembra svedese. Tre difese, in quest'ordine:
//!
//! 1. **[`sospetto`] è un cancello, non un suggerimento.** Un testo che non
//!    porta la firma di una decodifica sbagliata non viene nemmeno provato.
//!    Su una libreria taggata bene questo modulo costa una scansione di stringa
//!    e finisce lì.
//! 2. **La firma è strutturale, non statistica.** Una coppia
//!    guida-continuazione dell'UTF-8, un carattere di controllo C1, una parola
//!    intera fatta di sole lettere accentate: sono cose che il testo vero non
//!    contiene, non cose che contiene di rado.
//! 3. **Il vincitore deve battere l'originale di un margine.** A parità, o
//!    quasi, non si tocca niente.
//!
//! E soprattutto: la riparazione non riscrive mai il file dell'utente. Resta un
//! valore di libreria con la sua provenienza, che
//! [`crate::ricostruzione::Origine::TagRiparato`] dichiara e l'interfaccia
//! mostra accanto all'originale.

use encoding_rs::{
    BIG5, EUC_KR, Encoding, GBK, SHIFT_JIS, UTF_8, WINDOWS_1250, WINDOWS_1251, WINDOWS_1253,
    WINDOWS_1254,
};

/// Oltre questa lunghezza non si prova nemmeno.
///
/// I campi che si riparano sono titoli e nomi: qualche decina di caratteri. Una
/// stringa di mille non è un titolo, è un commento o un testo incollato dentro
/// un tag, e non vale il costo di nove decodifiche.
const MASSIMO: usize = 1024;

/// Quanto il candidato deve fare meglio dell'originale per vincere.
///
/// Non zero: a parità si lascia stare quel che c'è. È la stessa prudenza per cui
/// [`crate::enrich::plan_write`] non riscrive un campo con un valore identico.
const MARGINE: u32 = 10;

/// Le codifiche in cui si prova a rileggere i byte recuperati.
///
/// **windows-1252 non c'è, ed è deliberato**: i byte da cui si parte sono già
/// stati letti come Latin-1 o come windows-1252, e rileggerli nella seconda
/// riprodurrebbe quasi esattamente l'originale — un candidato che non può
/// vincere e che costa comunque una decodifica per ogni campo di ogni brano.
const CODIFICHE: [&Encoding; 9] = [
    // Per prima perché è il guasto di gran lunga più frequente: byte UTF-8
    // dichiarati Latin-1.
    UTF_8,
    WINDOWS_1251, // cirillico
    WINDOWS_1250, // europa centrale
    WINDOWS_1253, // greco
    WINDOWS_1254, // turco
    SHIFT_JIS,
    GBK,
    BIG5,
    EUC_KR,
];

/// I trentadue caratteri che windows-1252 mette dove Latin-1 tiene i controlli.
///
/// Scritta a mano e non presa dal codificatore di `encoding_rs`: quello, davanti
/// a un carattere che windows-1252 non sa scrivere, produce un riferimento
/// numerico HTML (`&#1057;`) invece di rifiutare. Dentro un tag musicale sarebbe
/// un guasto peggiore di quello che si stava riparando.
///
/// Le cinque voci che rimandano a sé stesse — 0x81, 0x8D, 0x8F, 0x90, 0x9D — non
/// sono una svista: è la tabella dello standard, dove quei byte non hanno un
/// carattere e passano invariati.
const CP1252_ALTO: [(u8, char); 32] = [
    (0x80, '\u{20AC}'),
    (0x81, '\u{0081}'),
    (0x82, '\u{201A}'),
    (0x83, '\u{0192}'),
    (0x84, '\u{201E}'),
    (0x85, '\u{2026}'),
    (0x86, '\u{2020}'),
    (0x87, '\u{2021}'),
    (0x88, '\u{02C6}'),
    (0x89, '\u{2030}'),
    (0x8A, '\u{0160}'),
    (0x8B, '\u{2039}'),
    (0x8C, '\u{0152}'),
    (0x8D, '\u{008D}'),
    (0x8E, '\u{017D}'),
    (0x8F, '\u{008F}'),
    (0x90, '\u{0090}'),
    (0x91, '\u{2018}'),
    (0x92, '\u{2019}'),
    (0x93, '\u{201C}'),
    (0x94, '\u{201D}'),
    (0x95, '\u{2022}'),
    (0x96, '\u{2013}'),
    (0x97, '\u{2014}'),
    (0x98, '\u{02DC}'),
    (0x99, '\u{2122}'),
    (0x9A, '\u{0161}'),
    (0x9B, '\u{203A}'),
    (0x9C, '\u{0153}'),
    (0x9D, '\u{009D}'),
    (0x9E, '\u{017E}'),
    (0x9F, '\u{0178}'),
];

/// I caratteri di windows-1252 che in un titolo vero non compaiono quasi mai.
///
/// Il trattino lungo, i puntini di sospensione, le virgolette curve, il simbolo
/// di marchio e l'euro **non** stanno in questo elenco: sono punteggiatura
/// normale, e contarli come sospetti farebbe scattare la riparazione su mezza
/// libreria. Restano quelli che in italiano, inglese, spagnolo o tedesco non
/// hanno un motivo per esserci: la f con l'uncino, i circonflessi isolati, le
/// virgolette ad angolo singolo.
///
/// Š, š, Ž, ž ci sono, e sono lettere vere (Ženski). Per questo la soglia non è
/// «uno» ma tre: un nome sloveno ne ha una, un titolo giapponese letto male ne
/// ha una manciata.
const RARI: [char; 12] = [
    '\u{0192}', // ƒ
    '\u{02C6}', // ˆ
    '\u{02DC}', // ˜
    '\u{2020}', // †
    '\u{2021}', // ‡
    '\u{2030}', // ‰
    '\u{201A}', // ‚
    '\u{201E}', // „
    '\u{2039}', // ‹
    '\u{203A}', // ›
    '\u{0161}', // š
    '\u{017E}', // ž
];

/// Quante occorrenze di [`RARI`] bastano a insospettire.
const RARI_MINIMO: usize = 3;

/// Quante lettere deve avere una parola tutta accentata perché sia un indizio.
///
/// Quattro e non tre: `ÅÄÖ` è la sigla di un gruppo vero, e a tre lettere non si
/// distingue da una parola russa riletta male. A quattro il testo latino non
/// arriva — nemmeno il tedesco, nemmeno lo svedese, che le accentate le mette
/// **fra** lettere ASCII, non una di fila all'altra.
const PAROLA_ALTA_MINIMO: usize = 4;

/// Se valga la pena provare a riparare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sospetto {
    /// Il testo non porta nessuna firma di decodifica sbagliata.
    No,
    /// Il testo porta almeno una firma: si può provare.
    Probabile,
}

/// Un testo riparato, con la codifica che lo ha prodotto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Riparazione {
    /// Il testo com'era prima della riparazione.
    pub originale: String,
    /// Il testo rileggendo i byte nella codifica giusta.
    pub testo: String,
    /// Il nome della codifica che ha vinto, come lo dà `encoding_rs`.
    pub codifica: &'static str,
    /// Di quanto il candidato ha battuto l'originale. Solo per essere guardato.
    pub guadagno: u32,
}

/// La scrittura a cui un carattere appartiene, per quel poco che serve qui.
///
/// Non è una classificazione Unicode completa e non deve esserlo: serve a
/// riconoscere una parola che mescola due alfabeti, che è una cosa che il testo
/// vero non fa e la spazzatura fa continuamente.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scrittura {
    /// Cifre, punteggiatura, spazi, simboli: non dicono niente.
    Muta,
    Latina,
    Cirillica,
    Greca,
    Han,
    Kana,
    Hangul,
    Araba,
    Ebraica,
}

const fn scrittura(c: char) -> Scrittura {
    match c {
        'a'..='z' | 'A'..='Z' => Scrittura::Latina,
        '\u{00C0}'..='\u{024F}' | '\u{1E00}'..='\u{1EFF}' => Scrittura::Latina,
        '\u{0370}'..='\u{03FF}' | '\u{1F00}'..='\u{1FFF}' => Scrittura::Greca,
        '\u{0400}'..='\u{052F}' => Scrittura::Cirillica,
        '\u{0590}'..='\u{05FF}' => Scrittura::Ebraica,
        '\u{0600}'..='\u{06FF}' => Scrittura::Araba,
        '\u{3040}'..='\u{30FF}' | '\u{31F0}'..='\u{31FF}' => Scrittura::Kana,
        '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}' => {
            Scrittura::Han
        }
        '\u{1100}'..='\u{11FF}' | '\u{3130}'..='\u{318F}' | '\u{AC00}'..='\u{D7AF}' => {
            Scrittura::Hangul
        }
        _ => Scrittura::Muta,
    }
}

/// Due scritture possono stare nella stessa parola?
///
/// Il giapponese mescola kana e han in una parola sola, e il coreano ci mette i
/// caratteri cinesi: sono gli unici accostamenti che il testo vero produce.
const fn convivono(a: Scrittura, b: Scrittura) -> bool {
    matches!(
        (a, b),
        (Scrittura::Han, Scrittura::Kana)
            | (Scrittura::Kana, Scrittura::Han)
            | (Scrittura::Han, Scrittura::Hangul)
            | (Scrittura::Hangul, Scrittura::Han)
    )
}

/// Il carattere può essere il primo byte di una sequenza UTF-8 letto come Latin-1.
///
/// I byte guida dell'UTF-8 vanno da 0xC2 a 0xF4; letti come Latin-1 — o come
/// windows-1252, che lì sopra coincide — danno esattamente questo intervallo.
const fn e_guida(c: char) -> bool {
    matches!(c, '\u{00C2}'..='\u{00F4}')
}

/// Il carattere può essere un byte di continuazione UTF-8 letto male.
///
/// I byte 0x80–0xBF letti come Latin-1 danno U+0080–U+00BF; letti come
/// windows-1252, i primi trentadue danno la punteggiatura di [`CP1252_ALTO`].
fn e_continuazione(c: char) -> bool {
    matches!(c, '\u{0080}'..='\u{00BF}') || CP1252_ALTO.iter().any(|(_, alto)| *alto == c)
}

/// La lettera sta nella metà alta di Latin-1.
///
/// È dove finiscono, lette come Latin-1, le lettere del cirillico di
/// windows-1251 e del greco di windows-1253: l'alfabeto intero sta fra 0xC0 e
/// 0xFF, quindi una parola tradotta male è una parola di sole lettere di qui.
///
/// I due segni di moltiplicazione e divisione stanno nell'intervallo e non sono
/// lettere: lasciarli dentro spezzerebbe il conto sulla parola.
const fn e_lettera_alta(c: char) -> bool {
    matches!(c, '\u{00C0}'..='\u{00FF}') && !matches!(c, '\u{00D7}' | '\u{00F7}')
}

/// Il testo porta la firma di una decodifica sbagliata?
///
/// Quattro firme, e sono strutturali: non «capita di rado nel testo vero», ma
/// «nel testo vero non capita».
///
/// ```
/// use aether_domain::codifica::{Sospetto, sospetto};
/// // Byte UTF-8 letti come Latin-1: la coppia guida-continuazione.
/// assert_eq!(sospetto("BjÃ¶rk"), Sospetto::Probabile);
/// // Cirillico di windows-1251 letto come Latin-1: la parola tutta accentata.
/// assert_eq!(sospetto("Ñåãîäíÿ"), Sospetto::Probabile);
/// // E quel che è già giusto si lascia stare.
/// assert_eq!(sospetto("Björk"), Sospetto::No);
/// assert_eq!(sospetto("Сегодня"), Sospetto::No);
/// assert_eq!(sospetto("Motörhead"), Sospetto::No);
/// ```
#[must_use]
pub fn sospetto(testo: &str) -> Sospetto {
    let mut precedente: Option<char> = None;
    let mut rari = 0_usize;
    // Lunghezza della parola in corso, e se finora è fatta di sole lettere alte.
    let mut lettere = 0_usize;
    let mut tutte_alte = true;

    for c in testo.chars() {
        if c == '\u{FFFD}' || matches!(c, '\u{0080}'..='\u{009F}') {
            return Sospetto::Probabile;
        }
        if precedente.is_some_and(e_guida) && e_continuazione(c) {
            return Sospetto::Probabile;
        }
        if RARI.contains(&c) {
            rari = rari.saturating_add(1);
            if rari >= RARI_MINIMO {
                return Sospetto::Probabile;
            }
        }

        if c.is_alphabetic() {
            lettere = lettere.saturating_add(1);
            tutte_alte = tutte_alte && e_lettera_alta(c);
        } else if !c.is_numeric() {
            // La parola è finita: si giudica e si riparte. Le cifre non
            // chiudono una parola — «Ñåãîäíÿ2» è la stessa parola.
            if tutte_alte && lettere >= PAROLA_ALTA_MINIMO {
                return Sospetto::Probabile;
            }
            lettere = 0;
            tutte_alte = true;
        }
        precedente = Some(c);
    }

    if tutte_alte && lettere >= PAROLA_ALTA_MINIMO {
        return Sospetto::Probabile;
    }
    Sospetto::No
}

/// Quanto un testo somiglia a spazzatura. Più basso è meglio.
///
/// Serve solo a confrontare fra loro i candidati e l'originale, quindi i pesi
/// non hanno un'unità di misura: hanno un ordine. Un carattere di rimpiazzo è
/// peggio di un controllo, che è peggio di una coppia guida-continuazione, che è
/// peggio di una parola tutta accentata.
fn penalita(testo: &str) -> u32 {
    /// Il conto che si chiude quando una parola finisce.
    fn fine_parola(lettere: usize, tutte_alte: bool, scritture: &[Scrittura]) -> u32 {
        let mut punti = 0_u32;
        if tutte_alte && lettere >= PAROLA_ALTA_MINIMO {
            punti = punti.saturating_add(15);
        }
        let mischiata = scritture
            .iter()
            .any(|a| scritture.iter().any(|b| a != b && !convivono(*a, *b)));
        if mischiata {
            punti = punti.saturating_add(6);
        }
        punti
    }

    let mut punti = 0_u32;
    let mut precedente: Option<char> = None;
    let mut lettere = 0_usize;
    let mut tutte_alte = true;
    let mut scritture: Vec<Scrittura> = Vec::new();

    for c in testo.chars() {
        if c == '\u{FFFD}' {
            punti = punti.saturating_add(30);
        }
        if matches!(c, '\u{0080}'..='\u{009F}') {
            punti = punti.saturating_add(20);
        }
        if precedente.is_some_and(e_guida) && e_continuazione(c) {
            punti = punti.saturating_add(12);
        }
        if RARI.contains(&c) {
            punti = punti.saturating_add(3);
        }

        if c.is_alphabetic() {
            lettere = lettere.saturating_add(1);
            tutte_alte = tutte_alte && e_lettera_alta(c);
            let s = scrittura(c);
            if s != Scrittura::Muta && !scritture.contains(&s) {
                scritture.push(s);
            }
        } else if !c.is_numeric() {
            punti = punti.saturating_add(fine_parola(lettere, tutte_alte, &scritture));
            scritture.clear();
            lettere = 0;
            tutte_alte = true;
        }
        precedente = Some(c);
    }
    punti.saturating_add(fine_parola(lettere, tutte_alte, &scritture))
}

/// I byte che, letti come Latin-1, avrebbero prodotto questo testo.
///
/// `None` se un carattere sta sopra U+00FF: allora quel testo, come Latin-1, non
/// è mai stato scritto, e la strada di ritorno non esiste.
fn byte_da_latin1(testo: &str) -> Option<Vec<u8>> {
    let mut byte = Vec::with_capacity(testo.len());
    for c in testo.chars() {
        byte.push(u8::try_from(u32::from(c)).ok()?);
    }
    Some(byte)
}

/// I byte che, letti come windows-1252, avrebbero prodotto questo testo.
///
/// I trentadue caratteri di [`CP1252_ALTO`] tornano al loro byte; il resto della
/// tabella coincide con Latin-1. Un carattere che windows-1252 non produce — per
/// esempio U+0080, che nella sua tabella non c'è — dà `None`: fingere che sia il
/// byte 0x80 vorrebbe dire inventarsi una strada di ritorno che nessun
/// decodificatore ha percorso.
fn byte_da_cp1252(testo: &str) -> Option<Vec<u8>> {
    let mut byte = Vec::with_capacity(testo.len());
    for c in testo.chars() {
        if let Some((codice, _)) = CP1252_ALTO.iter().find(|(_, alto)| *alto == c) {
            byte.push(*codice);
            continue;
        }
        if matches!(c, '\u{0080}'..='\u{009F}') {
            return None;
        }
        byte.push(u8::try_from(u32::from(c)).ok()?);
    }
    Some(byte)
}

/// Il testo riletto nella codifica che lo rende sensato, se ce n'è una.
///
/// Restituisce `None` quando il testo non è sospetto, quando la strada di
/// ritorno non esiste, o quando nessun candidato batte l'originale di
/// [`MARGINE`]. Non lancia mai una moneta: a parità vince quel che c'era.
///
/// ```
/// use aether_domain::codifica::ripara;
/// let r = ripara("BjÃ¶rk").expect("è riparabile");
/// assert_eq!(r.testo, "Björk");
/// assert_eq!(r.codifica, "UTF-8");
/// // Un nome vero non si tocca.
/// assert!(ripara("Tiësto").is_none());
/// assert!(ripara("坂本龍一").is_none());
/// ```
#[must_use]
pub fn ripara(testo: &str) -> Option<Riparazione> {
    if sospetto(testo) == Sospetto::No {
        return None;
    }
    if testo.chars().count() > MASSIMO {
        return None;
    }

    let base = penalita(testo);
    let mut migliore: Option<Riparazione> = None;

    // windows-1252 per prima: è la tabella con cui legge il software che gira
    // sulle macchine dove questi tag vengono scritti. A parità di guadagno la
    // prima esaminata resta, e questa è quella da preferire.
    let strade = [byte_da_cp1252(testo), byte_da_latin1(testo)];
    for byte in strade.into_iter().flatten() {
        for codifica in CODIFICHE {
            let (letto, _, guasti) = codifica.decode(&byte);
            // `guasti` è vero per le codifiche che possono rifiutare una
            // sequenza — UTF-8 e le doppie-byte. Un rifiuto è la risposta più
            // informativa che ci sia: quei byte non sono questa codifica, e
            // insistere produrrebbe una collana di U+FFFD.
            if guasti {
                continue;
            }
            if letto == testo || letto.trim().is_empty() {
                continue;
            }
            let punti = penalita(&letto);
            if punti.saturating_add(MARGINE) > base {
                continue;
            }
            let guadagno = base.saturating_sub(punti);
            if migliore.as_ref().is_some_and(|m| m.guadagno >= guadagno) {
                continue;
            }
            migliore = Some(Riparazione {
                originale: testo.to_owned(),
                testo: letto.into_owned(),
                codifica: codifica.name(),
                guadagno,
            });
        }
    }
    migliore
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Le scritture vere che NON si devono toccare.
    ///
    /// Contano più dei casi da riparare: sbagliare qui vuol dire prendere un tag
    /// giusto e romperlo, che è l'unico esito peggiore del non fare niente.
    const INTATTI: [&str; 14] = [
        "Björk",
        "Tiësto",
        "Motörhead",
        "Beyoncé",
        "Sigur Rós",
        "Mötley Crüe",
        "坂本龍一",
        "テスト",
        "Сегодня",
        "Кино",
        "Cœur de pirate",
        "Ženski",
        "ÅÄÖ",
        "Café del Mar — Volume 1",
    ];

    #[test]
    fn quel_che_e_gia_giusto_non_si_tocca() {
        for testo in INTATTI {
            assert_eq!(sospetto(testo), Sospetto::No, "{testo} non è sospetto");
            assert!(ripara(testo).is_none(), "{testo} non si deve riparare");
        }
    }

    /// Il testo che si otterrebbe leggendo come Latin-1 i byte UTF-8 di `giusto`.
    ///
    /// Costruito invece che scritto a mano perché la spazzatura vera contiene i
    /// controlli C1 — «Сегодня» finisce con U+008F — che in un sorgente non si
    /// vedono e si perdono al primo copia-incolla. Un vettore di prova che si
    /// tronca da solo prova un caso diverso da quello che dice di provare.
    fn come_latin1(giusto: &str) -> String {
        giusto.bytes().map(char::from).collect()
    }

    #[test]
    fn utf8_letto_come_latin1_si_ripara() {
        let r = ripara("BjÃ¶rk").expect("riparabile");
        assert_eq!(r.testo, "Björk");
        assert_eq!(r.codifica, "UTF-8");
        assert_eq!(r.originale, "BjÃ¶rk");

        for giusto in ["Сегодня", "Кино", "坂本龍一", "Björk", "Tiësto"] {
            let storto = come_latin1(giusto);
            let r = ripara(&storto).unwrap_or_else(|| panic!("{storto} è riparabile"));
            assert_eq!(r.testo, giusto);
            assert_eq!(r.codifica, "UTF-8");
        }
    }

    #[test]
    fn a_parita_vince_utf8_perche_e_il_guasto_piu_frequente() {
        // Gli stessi byte letti in windows-1251 danno un cirillico che nessuno
        // ha mai scritto, e per la penalità vale quanto quello giusto: una
        // scrittura sola, nessun controllo. A distinguerli resta solo l'ordine
        // di CODIFICHE, ed è deliberato che UTF-8 sia il primo.
        let storto = come_latin1("Сегодня");
        let r = ripara(&storto).expect("riparabile");
        assert_eq!(r.codifica, "UTF-8");
    }

    #[test]
    fn utf8_letto_come_cp1252_si_ripara() {
        // I byte 0x80–0x9F qui non sono controlli ma punteggiatura: senza la
        // tabella scritta a mano la strada di ritorno non esisterebbe.
        let r = ripara("ãƒ†ã‚¹ãƒˆ").expect("riparabile");
        assert_eq!(r.testo, "テスト");
    }

    #[test]
    fn cirillico_di_windows_1251_letto_come_latin1_si_ripara() {
        // Il caso dell'ID3v1: nessuna dichiarazione di codifica, e chi legge
        // sceglie Latin-1 perché è l'unica scelta che può fare.
        let r = ripara("Ñåãîäíÿ").expect("riparabile");
        assert_eq!(r.testo, "Сегодня");
        assert_eq!(r.codifica, "windows-1251");
    }

    #[test]
    fn una_parola_corta_tutta_accentata_non_basta() {
        // `ÅÄÖ` è un gruppo vero. A tre lettere non si distingue da una parola
        // russa riletta male, e nel dubbio non si tocca.
        assert_eq!(sospetto("ÅÄÖ"), Sospetto::No);
    }

    #[test]
    fn la_punteggiatura_curva_non_e_un_sospetto() {
        // Trattino lungo, virgolette curve, puntini: sono punteggiatura vera, e
        // contarli farebbe scattare la riparazione su mezza libreria.
        for testo in [
            "Rock ’n’ Roll",
            "Album — Deluxe",
            "Tutto…",
            "“Live”",
            "100 €",
        ] {
            assert_eq!(sospetto(testo), Sospetto::No, "{testo}");
        }
    }

    #[test]
    fn il_testo_vuoto_e_lascii_puro_non_costano_niente() {
        assert_eq!(sospetto(""), Sospetto::No);
        assert_eq!(sospetto("Karma Police"), Sospetto::No);
        assert!(ripara("").is_none());
    }

    #[test]
    fn la_riparazione_e_stabile() {
        // Riparare due volte non deve dare due risultati: il secondo giro parte
        // da un testo che non è più sospetto.
        let r = ripara("BjÃ¶rk").expect("riparabile");
        assert!(ripara(&r.testo).is_none());
    }

    #[test]
    fn una_stringa_lunghissima_non_si_prova() {
        let lunga: String = std::iter::repeat_n('Ã', MASSIMO + 1).collect();
        assert!(ripara(&lunga).is_none());
    }

    #[test]
    fn la_strada_di_ritorno_manca_e_non_si_inventa() {
        // U+0130 sta sopra Latin-1 e non è nella tabella di windows-1252:
        // nessuna delle due decodifiche può averlo prodotto.
        assert_eq!(byte_da_latin1("İ"), None);
        assert_eq!(byte_da_cp1252("İ"), None);
        // U+0080 windows-1252 non lo scrive: fingere il byte 0x80 vorrebbe dire
        // inventarsi un percorso che nessun decodificatore ha fatto.
        assert_eq!(byte_da_cp1252("\u{0080}"), None);
        assert_eq!(byte_da_latin1("\u{0080}"), Some(vec![0x80]));
    }

    #[test]
    fn la_penalita_ordina_come_deve() {
        assert!(penalita("Björk") < penalita("BjÃ¶rk"));
        assert!(penalita("Сегодня") < penalita("Ñåãîäíÿ"));
        assert_eq!(penalita("Karma Police"), 0);
    }
}
