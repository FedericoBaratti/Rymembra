//! Dov'è scritta ogni cosa: dal percorso nel documento alla riga nel file.
//!
//! # Perché un secondo passaggio sul testo
//!
//! Perché il primo lo butta via. `serde_json::Value` è un albero di valori e non
//! ricorda da che carattere veniva ognuno; [`crate::document::SkinDocument`]
//! ancora meno — è la stessa ragione per cui non è riserializzabile. Quando la
//! validazione dice «`tokens.color.accent` non è un colore», il percorso ce l'ha
//! e la riga no.
//!
//! Nel vecchio albero la riga la indovinava la finestra, cercando `"accent"` con
//! un `indexOf` e prendendo la **prima** occorrenza del file. Su un percorso
//! profondo — `parts.section-card.states.hover.background.0.stops.1.color` —
//! nessun sotto-percorso esiste come chiave scritta, si ripiegava fino a
//! `"color"`, e si sottolineava la prima riga che nominava un colore qualunque.
//! Cioè quasi sempre quella sbagliata, con l'aria di essere quella giusta.
//!
//! # Perché sta qui e non nella finestra
//!
//! Stessa ragione del resto del crate: qui si prova. `npm run verify` non ha
//! prove TypeScript, e questo è un lettore di testo scritto a mano — cioè
//! esattamente il genere di codice che si rompe sui casi che nessuno guarda a
//! occhio: una barra rovescia prima di una virgoletta, un `\r\n`, una chiave che
//! contiene un punto.
//!
//! # La grafia dei percorsi è quella di `giu()`
//!
//! Chiavi unite da un punto, indici di lista come segmenti decimali:
//! `parts.section-card.background.0.stops.1.color`. È **la stessa** che
//! costruisce [`crate::document`] mentre valida, e non per convenzione: è la
//! condizione perché il confronto qui sia un'uguaglianza invece di
//! un'euristica. Una chiave che contiene punti — `color.surface.0`, che di token
//! ce n'è a decine — produce da entrambe le parti la stessa stringa
//! `tokens.color.surface.0`, e va bene: il formato non ammette insieme un
//! `tokens.color` annidato e un `tokens."color.surface.0"` piatto, quindi due
//! percorsi uguali non sono mai due posti diversi.
//!
//! # La colonna si conta in unità UTF-16
//!
//! Non in byte e non in `char`. Perché il numero che esce di qui finisce in due
//! posti: sotto gli occhi di chi legge, e dentro `setSelectionRange` di una
//! `<textarea>`, che indicizza in unità UTF-16 come tutte le stringhe di
//! JavaScript. Contare i `char` darebbe la stessa risposta finché il documento è
//! latino e una diversa al primo emoji dentro un `meta.name`: il cursore
//! finirebbe una posizione prima del punto nominato, ed è un difetto che si
//! manifesta solo a casa di qualcun altro.

use std::collections::HashMap;

/// Un posto nel file, contando da uno: è come si leggono le righe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Punto {
    /// La riga, da uno.
    pub riga: u32,
    /// La colonna in unità UTF-16, da uno. Vedi il preambolo.
    pub colonna: u32,
}

/// Dove sta ogni cosa del documento.
#[derive(Debug, Clone, Default)]
pub struct Posizioni {
    dove: HashMap<String, Punto>,
}

impl Posizioni {
    /// Il punto di questo percorso, o dell'antenato più vicino che esiste.
    ///
    /// Il ripiego non è pigrizia: «campo obbligatorio, manca» nomina un percorso
    /// che nel file **non c'è per definizione**, e la risposta utile è la riga
    /// dell'oggetto a cui manca — non nessuna riga.
    #[must_use]
    pub fn di(&self, percorso: &str) -> Option<Punto> {
        if let Some(punto) = self.dove.get(percorso) {
            return Some(*punto);
        }
        let mut resto = percorso;
        while let Some((prima, _)) = resto.rsplit_once('.') {
            if let Some(punto) = self.dove.get(prima) {
                return Some(*punto);
            }
            resto = prima;
        }
        // La radice, per un documento che almeno comincia.
        self.dove.get("").copied()
    }

    /// Quante voci ha. Serve alle prove, e a niente altro.
    #[must_use]
    pub fn quante(&self) -> usize {
        self.dove.len()
    }
}

/// Quanti valori si segnano al massimo.
///
/// Un documento vero ne ha qualche migliaio. Il tetto non serve contro le skin:
/// serve contro un testo a metà — quel che c'è nell'editor fra una battuta e
/// l'altra — in cui una virgoletta aperta fa leggere il resto del file come una
/// stringa sola e poi ripartire storto. Meglio una mappa incompleta di un ciclo
/// che tiene occupata la finestra.
const TETTO: u32 = 20_000;

/// Quanto può essere profondo l'annidamento prima che ci si fermi.
///
/// La ricorsione qui è vera ricorsione, e il testo lo scrive chiunque: senza un
/// tetto, `[[[[[…` fa saltare la pila. Trentadue è più di quanto il formato
/// ammetta — la parte più profonda che esiste sta in otto passi.
const PROFONDITA: usize = 32;

/// Il lettore: scorre il testo una volta e segna dove comincia ogni valore.
struct Lettore<'a> {
    testo: &'a [u8],
    /// Dove siamo, in byte.
    i: usize,
    /// La riga corrente, da uno.
    riga: u32,
    /// Le unità UTF-16 già viste su questa riga, più uno.
    colonna: u32,
    dove: HashMap<String, Punto>,
    /// Quanti valori restano da segnare. Vedi [`TETTO`].
    restanti: u32,
}

impl<'a> Lettore<'a> {
    fn nuovo(testo: &'a str) -> Self {
        Self {
            testo: testo.as_bytes(),
            i: 0,
            riga: 1,
            colonna: 1,
            dove: HashMap::new(),
            restanti: TETTO,
        }
    }

    /// Il byte corrente, se c'è.
    fn guarda(&self) -> Option<u8> {
        self.testo.get(self.i).copied()
    }

    /// Il punto in cui siamo adesso.
    const fn qui(&self) -> Punto {
        Punto {
            riga: self.riga,
            colonna: self.colonna,
        }
    }

    /// Avanza di un byte, tenendo il conto di righe e colonne.
    ///
    /// Le colonne si contano sui **byte iniziali** di UTF-8 — quelli che non
    /// sono `10xxxxxx` — più uno in più per chi sta fuori dal piano base: in
    /// UTF-16 quello è una coppia surrogata, cioè due unità. È la stessa
    /// aritmetica che fa `String.prototype.length` dall'altra parte.
    ///
    /// Un `\r` da solo non va a capo: in JSON un a capo dentro una stringa non
    /// esiste, e fuori è spazio. La finestra conta le righe su `\n`, e contarle
    /// qui su `\r` darebbe due numeri diversi per lo stesso file.
    fn avanti(&mut self) {
        let Some(byte) = self.guarda() else {
            return;
        };
        self.i += 1;
        if byte == b'\n' {
            self.riga = self.riga.saturating_add(1);
            self.colonna = 1;
            return;
        }
        if byte & 0b1100_0000 == 0b1000_0000 {
            // Byte di continuazione: non è un carattere nuovo.
            return;
        }
        let quante = u32::from(byte >= 0b1111_0000) + 1;
        self.colonna = self.colonna.saturating_add(quante);
    }

    /// Salta spazi, tabulazioni e a capo.
    fn respira(&mut self) {
        while matches!(self.guarda(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.avanti();
        }
    }

    /// Consuma una stringa JSON e restituisce quel tanto che serve a farne una
    /// chiave: le sequenze `\uXXXX` restano scritte come sono.
    ///
    /// Non è una svista. Una chiave del formato è un nome ASCII — un token, una
    /// parte, un effetto — e nessuno la scrive con `\u0063`. Scioglierle
    /// vorrebbe dire portare qui un lettore di surrogati per un caso che non
    /// esiste; le due che si sciolgono davvero (`\"` e `\\`) sono quelle che
    /// cambiano **dove finisce la stringa**, ed è tutto un altro problema.
    fn stringa(&mut self) -> Option<String> {
        if self.guarda() != Some(b'"') {
            return None;
        }
        self.avanti();
        let mut fuori = Vec::new();
        loop {
            let byte = self.guarda()?;
            self.avanti();
            match byte {
                b'"' => break,
                b'\\' => {
                    let dopo = self.guarda()?;
                    self.avanti();
                    match dopo {
                        b'"' => fuori.push(b'"'),
                        b'\\' => fuori.push(b'\\'),
                        b'/' => fuori.push(b'/'),
                        b'n' => fuori.push(b'\n'),
                        b't' => fuori.push(b'\t'),
                        b'r' => fuori.push(b'\r'),
                        b'b' => fuori.push(0x08),
                        b'f' => fuori.push(0x0c),
                        altro => {
                            fuori.push(b'\\');
                            fuori.push(altro);
                        }
                    }
                }
                altro => fuori.push(altro),
            }
        }
        Some(String::from_utf8_lossy(&fuori).into_owned())
    }

    /// Consuma un valore qualunque, segnandolo a `percorso`.
    ///
    /// `punto` è dove il valore **si nomina**: la sua chiave quando ce n'è una,
    /// il valore stesso quando è una voce di lista o la radice. È la differenza
    /// fra portare il cursore su `"color.accent": "#zzz"` e portarlo sul
    /// `"#zzz"`, e la prima è quel che si è venuti a cercare.
    fn valore(&mut self, percorso: &str, punto: Punto, profondita: usize) {
        if self.restanti == 0 {
            return;
        }
        self.restanti -= 1;
        self.dove.insert(percorso.to_owned(), punto);

        self.respira();
        match self.guarda() {
            Some(b'{') if profondita < PROFONDITA => self.oggetto(percorso, profondita),
            Some(b'[') if profondita < PROFONDITA => self.lista(percorso, profondita),
            _ => self.salta(),
        }
    }

    /// Le voci di un oggetto, ognuna col suo percorso.
    fn oggetto(&mut self, percorso: &str, profondita: usize) {
        self.avanti(); // la graffa
        loop {
            self.respira();
            match self.guarda() {
                None => return,
                Some(b'}') => {
                    self.avanti();
                    return;
                }
                Some(b',') => {
                    self.avanti();
                    continue;
                }
                Some(b'"') => {}
                // Qualunque altra cosa qui è testo a metà di una battuta: si
                // smette invece di provare a indovinare dove ricominci.
                _ => return,
            }
            let punto = self.qui();
            let Some(chiave) = self.stringa() else {
                return;
            };
            self.respira();
            if self.guarda() != Some(b':') {
                return;
            }
            self.avanti();
            let dentro = if percorso.is_empty() {
                chiave
            } else {
                format!("{percorso}.{chiave}")
            };
            self.valore(&dentro, punto, profondita + 1);
        }
    }

    /// Le voci di una lista: il percorso è la posizione, contando da zero.
    fn lista(&mut self, percorso: &str, profondita: usize) {
        self.avanti(); // la quadra
        let mut indice = 0_usize;
        loop {
            self.respira();
            match self.guarda() {
                None => return,
                Some(b']') => {
                    self.avanti();
                    return;
                }
                Some(b',') => {
                    self.avanti();
                    continue;
                }
                _ => {}
            }
            let punto = self.qui();
            let dentro = if percorso.is_empty() {
                indice.to_string()
            } else {
                format!("{percorso}.{indice}")
            };
            self.valore(&dentro, punto, profondita + 1);
            indice += 1;
        }
    }

    /// Consuma un valore semplice — stringa, numero, `true`, `false`, `null` —
    /// e insieme quel che non si è saputo leggere, fermandosi al primo segno che
    /// appartiene a chi ci sta intorno.
    fn salta(&mut self) {
        if self.guarda() == Some(b'"') {
            self.stringa();
            return;
        }
        while let Some(byte) = self.guarda() {
            if matches!(byte, b',' | b'}' | b']') {
                return;
            }
            self.avanti();
        }
    }
}

/// Dove sta ogni cosa di questo documento.
///
/// Non valida niente e non fallisce mai: su un testo rotto restituisce quel che
/// ha capito fino al punto in cui si è rotto — che è esattamente quel che serve,
/// perché i problemi di un documento a metà stanno prima di quel punto e non
/// dopo.
#[must_use]
pub fn posizioni(sorgente: &str) -> Posizioni {
    let mut lettore = Lettore::nuovo(sorgente);
    lettore.respira();
    let punto = lettore.qui();
    lettore.valore("", punto, 0);
    Posizioni { dove: lettore.dove }
}

#[cfg(test)]
mod prove {
    use super::*;

    fn punto(sorgente: &str, percorso: &str) -> Punto {
        posizioni(sorgente)
            .di(percorso)
            .unwrap_or_else(|| panic!("«{percorso}» non trovato"))
    }

    #[test]
    fn una_chiave_annidata_sta_sulla_sua_riga() {
        let sorgente = "{\n  \"meta\": {\n    \"name\": \"Prova\"\n  }\n}";
        assert_eq!(
            punto(sorgente, "meta"),
            Punto {
                riga: 2,
                colonna: 3
            }
        );
        assert_eq!(
            punto(sorgente, "meta.name"),
            Punto {
                riga: 3,
                colonna: 5
            }
        );
    }

    #[test]
    fn una_posizione_di_lista_e_un_passo_come_gli_altri() {
        let sorgente =
            "{\n  \"stops\": [\n    { \"color\": \"#000\" },\n    { \"color\": \"#fff\" }\n  ]\n}";
        assert_eq!(
            punto(sorgente, "stops.1"),
            Punto {
                riga: 4,
                colonna: 5
            },
            "la seconda voce, non la prima"
        );
        assert_eq!(
            punto(sorgente, "stops.1.color"),
            Punto {
                riga: 4,
                colonna: 7
            }
        );
    }

    /// Il caso per cui l'euristica della finestra sbagliava sempre: il percorso
    /// finisce con un nome che nel file compare dieci volte.
    #[test]
    fn un_nome_ripetuto_non_confonde() {
        let sorgente = "{\n  \"a\": { \"color\": 1 },\n  \"b\": { \"color\": 2 },\n  \"c\": { \"color\": 3 }\n}";
        assert_eq!(punto(sorgente, "c.color").riga, 4);
        assert_eq!(punto(sorgente, "b.color").riga, 3);
    }

    /// Un token si chiama `color.surface.0`, punti compresi, e il percorso che ne
    /// esce è lo stesso che costruisce `document.rs::giu`.
    #[test]
    fn una_chiave_con_i_punti_dentro_resta_una_chiave_sola() {
        let sorgente = "{\n  \"tokens\": {\n    \"color.surface.0\": \"#050508\"\n  }\n}";
        assert_eq!(punto(sorgente, "tokens.color.surface.0").riga, 3);
    }

    #[test]
    fn una_virgoletta_scappata_non_chiude_la_stringa() {
        let sorgente = "{\n  \"a\": \"uno \\\" due\",\n  \"b\": 2\n}";
        assert_eq!(punto(sorgente, "b").riga, 3, "«b» è ancora al suo posto");
    }

    #[test]
    fn una_barra_rovescia_finale_non_scappa_la_virgoletta() {
        let sorgente = "{\n  \"a\": \"c:\\\\\",\n  \"b\": 2\n}";
        assert_eq!(punto(sorgente, "b").riga, 3);
    }

    #[test]
    fn il_ritorno_a_capo_di_windows_conta_una_riga_sola() {
        let sorgente = "{\r\n  \"a\": 1,\r\n  \"b\": 2\r\n}";
        assert_eq!(punto(sorgente, "b").riga, 3);
    }

    #[test]
    fn la_colonna_conta_come_la_conta_javascript() {
        // Un emoji è una coppia surrogata: due unità UTF-16, un `char` solo.
        let sorgente = "{ \"a\": \"🎵\", \"b\": 2 }";
        let prima = sorgente
            .split_once("\"b\"")
            .map(|(p, _)| p)
            .expect("«b» c'è");
        let atteso = u32::try_from(prima.encode_utf16().count()).expect("ci sta") + 1;
        assert_eq!(punto(sorgente, "b").colonna, atteso);
    }

    #[test]
    fn un_percorso_che_non_ce_ripiega_sul_padre() {
        let sorgente = "{\n  \"meta\": {\n    \"name\": \"Prova\"\n  }\n}";
        assert_eq!(
            posizioni(sorgente).di("meta.version"),
            Some(Punto {
                riga: 2,
                colonna: 3
            }),
            "il campo obbligatorio che manca sta sulla riga dell'oggetto a cui manca"
        );
    }

    /// La radice c'è sempre, e il ripiego ci arriva: un percorso di cui non si
    /// sa niente cade sull'inizio del documento invece che nel vuoto. È la
    /// risposta giusta per l'editor — «da qualche parte qui» batte «da nessuna
    /// parte», che spegnerebbe il bottone che porta il cursore.
    #[test]
    fn un_documento_vuoto_non_esplode() {
        assert_eq!(
            posizioni("").di("qualunque.cosa"),
            Some(Punto {
                riga: 1,
                colonna: 1
            })
        );
        assert!(posizioni("{").di("x").is_some());
    }

    #[test]
    fn un_documento_a_meta_dice_quel_che_ha_letto() {
        let sorgente = "{\n  \"id\": \"prova\",\n  \"meta\": {\n    \"name\": ";
        assert_eq!(punto(sorgente, "id").riga, 2);
        assert_eq!(punto(sorgente, "meta").riga, 3);
    }

    #[test]
    fn un_annidamento_assurdo_si_ferma_invece_di_scendere() {
        let sorgente = "[".repeat(10_000);
        // Quel che conta è che torni.
        assert!(posizioni(&sorgente).quante() <= PROFONDITA + 1);
    }

    /// Il collaudo vero: ogni percorso segnato sulla skin di riferimento cade su
    /// una riga che nomina davvero l'ultimo pezzo di quel percorso.
    #[test]
    fn sulla_skin_di_riferimento_ogni_punto_cade_dove_dice() {
        let sorgente = crate::PLAIN_SOURCE;
        let righe: Vec<&str> = sorgente.split('\n').collect();
        let trovate = posizioni(sorgente);
        assert!(trovate.quante() > 200, "plain.json ha molte voci");

        for (percorso, punto) in &trovate.dove {
            if percorso.is_empty() {
                continue;
            }
            let ultimo = percorso.rsplit('.').next().unwrap_or_default();
            // Un segmento numerico è una posizione di lista: non è scritto.
            if ultimo.parse::<usize>().is_ok() {
                continue;
            }
            let riga = righe
                .get(usize::try_from(punto.riga).expect("ci sta") - 1)
                .copied()
                .unwrap_or_default();
            assert!(
                riga.contains(ultimo),
                "«{percorso}» dice riga {} ma lì c'è «{}»",
                punto.riga,
                riga.trim()
            );
        }
    }
}
