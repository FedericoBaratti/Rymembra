//! Quel che due Rymembra si dicono in una sessione, dopo la stretta.
//!
//! # Due strati, e chi li vede
//!
//! Questo modulo è lo strato **dell'applicazione**: inviti, lo stato della
//! sessione, le ancore della sincronia, i gesti dei membri. Lo vede il desktop,
//! che li manda con [`crate::Rete::manda`] e li riceve come
//! [`crate::Evento::Ricevuto`].
//!
//! Lo strato **del trasporto** — il saluto, il battito, l'orologio,
//! l'associazione, le letture dei brani — sta dentro il crate e non esce mai:
//! l'orologio va risposto nel filo che legge la connessione, con l'istante
//! preso alla ricezione e quello preso alla scrittura, e un giro attraverso il
//! filo della sessione ci metterebbe dentro i suoi millisecondi.
//!
//! # Versioni
//!
//! Ogni messaggio porta il suo `tipo`. Un tipo che questo PC non conosce arriva
//! come [`Messaggio::Sconosciuto`] e si ignora: è così che un PC più nuovo può
//! aggiungere un messaggio senza far cadere la connessione a uno più vecchio.
//! [`VERSIONE`] si alza solo quando un messaggio che esiste **cambia
//! significato**, e allora i due PC si rifiutano con `insieme.versione`.
//!
//! Lo stesso vale **dentro** i messaggi: un gesto nuovo ([`Azione`]), una
//! sorgente nuova ([`SorgenteSulFilo`]), un modo di ripetere nuovo
//! ([`Ripeti`]) o un posto nuovo dove aggiungere ([`Dove`]) arrivano come la
//! loro variante `Sconosciuta`/`Sconosciuto`. Fino alla revisione del 26
//! ottobre non c'era: una variante nuova faceva fallire la lettura di tutto il
//! messaggio, e la connessione si chiudeva con `insieme.protocollo` invece di
//! ignorare una parte. [`Messaggio::valida`] le lascia passare — chiudere per
//! loro è proprio quel che si voleva evitare — e tocca a chi le riceve non
//! farne niente: un comando con un gesto che non si capisce
//! ([`Azione::conosciuta`]) si risponde con un `Esito` col codice
//! `insieme.gestoNonDisponibile`, una voce con una sorgente che non si
//! capisce non si suona. Non servono a mandare: chi ne manda una manda un
//! messaggio che nessuno capirà.
//!
//! # Niente testo libero dall'altro PC
//!
//! I rifiuti viaggiano come codice del catalogo (`insieme.sessionePiena`), mai
//! come frase: chi li riceve li traduce nella sua lingua, e un PC estraneo non
//! può far comparire una frase sua sullo schermo di un altro. I soli testi sono
//! nomi e titoli, e ognuno ha il suo tetto in [`Messaggio::valida`].

use serde::{Deserialize, Serialize};

/// La versione del protocollo. Vedi «Versioni» qui sopra.
pub const VERSIONE: u32 = 1;

/// Quante persone al massimo in una sessione, chi ospita compreso.
pub const MASSIMO_PERSONE: usize = 9;

/// Quante voci al massimo nella coda di una sessione.
pub const MASSIMO_VOCI: usize = 1000;

/// Quante voci al massimo in un solo [`Azione::Aggiungi`].
pub const MASSIMO_AGGIUNTE: usize = 200;

/// Il tetto di un nome di PC, in caratteri.
pub const MASSIMO_NOME: usize = 64;

/// Il tetto di un titolo, di un interprete, di un album, in caratteri.
pub const MASSIMO_TESTO: usize = 512;

/// Il tetto di un riferimento di catalogo, in caratteri.
pub const MASSIMO_RIFERIMENTO: usize = 2048;

/// Il tetto della durata di una voce: un giorno. Oltre è un numero sbagliato,
/// non un brano lungo.
pub const MASSIMA_DURATA_MS: u64 = 24 * 60 * 60 * 1000;

/// Un permesso di lettura: sedici byte a caso, uno per file e per sessione.
///
/// Chi lo ha può leggere quel file, e nient'altro, finché chi lo ha dato non lo
/// revoca. Non dice niente del file: né il percorso né il nome.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Gettone(pub [u8; 16]);

impl Gettone {
    /// Un gettone nuovo, dal generatore del sistema.
    ///
    /// # Errori
    ///
    /// `internal.unexpected` se il generatore del sistema non risponde.
    pub fn nuovo() -> Result<Self, rymembra_domain::errors::AppError> {
        let mut byte = [0_u8; 16];
        getrandom::fill(&mut byte).map_err(|err| {
            rymembra_domain::errors::AppError::new(
                rymembra_domain::errors::ErrorCode::InternalUnexpected { detail: None },
            )
            .with_cause(format!("il generatore del sistema non risponde: {err}"))
        })?;
        Ok(Self(byte))
    }
}

impl Gettone {
    /// Un nome stabile del file dietro il gettone, che **non** è il gettone:
    /// i primi 8 byte di blake3, in esadecimale. Uguale per le voci dello
    /// stesso file — è quel che fa riconoscere contigue due voci della stessa
    /// immagine CD — e inservibile per leggerlo, così può finire dove un
    /// gettone non deve finire.
    #[must_use]
    pub fn impronta(&self) -> String {
        let hash = blake3::hash(&self.0);
        let mut byte = [0_u8; 8];
        for (posto, b) in byte.iter_mut().zip(hash.as_bytes()) {
            *posto = *b;
        }
        crate::esadecimale::scrivi(&byte)
    }
}

/// Mai per esteso nel diario: un gettone stampato è un gettone copiabile.
impl std::fmt::Debug for Gettone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Gettone(…)")
    }
}

impl Serialize for Gettone {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&crate::esadecimale::scrivi(&self.0))
    }
}

impl<'de> Deserialize<'de> for Gettone {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let testo = String::deserialize(d)?;
        crate::esadecimale::leggi::<16>(&testo)
            .map(Self)
            .ok_or_else(|| serde::de::Error::custom("gettone non valido"))
    }
}

/// Un messaggio dell'applicazione. Vedi il `//!` del modulo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tipo", rename_all = "camelCase")]
pub enum Messaggio {
    // ── l'ingresso ──────────────────────────────────────────────────────────
    /// Chi ospita invita un PC associato che è collegato. Host → membro.
    Invito {
        /// Il numero dell'invito, per la risposta.
        invito: u64,
        /// Fra quanti millisecondi l'invito scade, contati da chi lo riceve.
        ///
        /// Una durata e non un istante: i due orologi non sono d'accordo finché
        /// la sessione non comincia.
        scade_fra_ms: u64,
    },
    /// La risposta a un [`Messaggio::Invito`]. Membro → host.
    RispostaInvito {
        /// Il numero dell'invito.
        invito: u64,
        /// Sì o no.
        accetta: bool,
    },
    /// Un PC associato chiede di entrare nella sessione. Membro → host.
    Chiedi,
    /// Il ritiro di un [`Messaggio::Chiedi`] ancora senza risposta. Membro → host.
    NonChiedoPiu,
    /// Si entra: da qui arrivano lo stato e le ancore. Host → membro.
    Ammesso {
        /// Il numero di questo membro nella sessione: è quello di
        /// [`VoceSulFilo::di`] per le sue voci. L'host è sempre `0`.
        membro: u32,
        /// Che cosa potrà fare, già qui e non solo nel primo stato: fra i due
        /// passa un attimo in cui un tasto premuto partirebbe lo stesso.
        #[serde(default = "Diritti::come_prima")]
        diritti: Diritti,
    },
    /// Un no, col suo codice del catalogo (`insieme.sessionePiena`,
    /// `insieme.rifiutato`, …). Host → membro.
    Rifiutato {
        /// Il codice stabile, `ErrorCodeKind::code()`.
        codice: String,
    },
    /// Un membro esce dalla sessione. Membro → host.
    Esco,

    // ── la sessione ─────────────────────────────────────────────────────────
    /// Tutto quel che serve per specchiare la sessione. Host → membri.
    ///
    /// Intero ogni volta e non a differenze: mille voci sono un centinaio di
    /// kilobyte, cambiano a gesti e non a colpi d'orologio, e uno stato
    /// intero non si può applicare male perché se ne è perso uno prima.
    Stato(StatoSessione),
    /// «Questo punto si sente a quest'istante», sull'orologio dell'host.
    /// Host → membri, qualche volta al secondo e subito a ogni cambio.
    Ancora(AncoraSulFilo),
    /// La sessione è finita, o questo membro ne è stato tolto. Host → membro.
    Fine {
        /// `None`: chi ospita l'ha terminata. Altrimenti il perché.
        codice: Option<String>,
    },

    // ── i gesti dei membri (fase 2) ─────────────────────────────────────────
    /// Un gesto di un membro, da eseguire sull'host. Membro → host.
    Comando {
        /// Il numero del comando, per l'[`Messaggio::Esito`].
        id: u32,
        /// Che cosa.
        azione: Azione,
    },
    /// Com'è andato un [`Messaggio::Comando`]. Host → membro.
    Esito {
        /// Il numero del comando.
        id: u32,
        /// `None`: fatto. Altrimenti il codice del catalogo del rifiuto.
        codice: Option<String>,
    },

    /// Un tipo che questo PC non conosce: si ignora. Vedi «Versioni».
    #[serde(other)]
    Sconosciuto,
}

/// Lo stato della sessione. Vedi [`Messaggio::Stato`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatoSessione {
    /// Cresce a ogni stato mandato: un membro scarta quelli più vecchi
    /// dell'ultimo applicato.
    pub versione: u64,
    /// Chi c'è, l'host per primo (`membro == 0`).
    pub persone: Vec<Persona>,
    /// La coda della sessione, nell'ordine in cui suonerà.
    pub coda: Vec<VoceSulFilo>,
    /// La voce che suona adesso, per `uid`.
    pub corrente: Option<u64>,
    /// L'host è in pausa.
    pub in_pausa: bool,
    /// Come si ripete. Lo shuffle in sessione non c'è: l'ordine è quello della
    /// coda, uguale per tutti.
    pub ripeti: Ripeti,
}

/// Una persona nella sessione.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Persona {
    /// Il suo numero nella sessione; `0` è l'host.
    pub membro: u32,
    /// Il nome del suo PC, come l'ha scelto lui.
    pub nome: String,
    /// Che cosa può fare. L'host è sempre [`Diritti::Tutto`].
    #[serde(default = "Diritti::come_prima")]
    pub diritti: Diritti,
}

/// Che cosa può fare un membro: lo sceglie chi ospita quando lo fa entrare, e
/// lo può cambiare finché resta.
///
/// La porta è l'host: un gesto che non è concesso si risponde con un `Esito`
/// col codice `insieme.nonConcesso`, qualunque versione l'abbia mandato. Il
/// membro lo sa per non mandarlo — i tasti multimediali non passano dalla
/// finestra — e la sua finestra per spegnere i tasti prima del clic.
///
/// Volume, equalizzatore, uscita e latenza non c'entrano: sono di ciascuno, e
/// restano suoi a ogni livello.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Diritti {
    /// I comandi e la coda: scegliere una voce, toglierla, spostarla, e
    /// aggiungere i propri brani.
    Tutto,
    /// I comandi: pausa e ripresa, avanti e indietro, il salto nel brano, il
    /// modo di ripetere.
    Comanda,
    /// Ascolta e basta: nessun gesto sulla musica degli altri.
    ///
    /// È anche quel che vale un livello che questo PC non conosce: il più
    /// stretto, perché sbagliare per eccesso darebbe a qualcuno un comando
    /// che chi ospita non gli ha dato. Vedi «Versioni». Per questo è
    /// l'ultimo: `serde(other)` lo vuole lì.
    #[serde(other)]
    Ascolta,
}

impl Diritti {
    /// Quel che vale un messaggio senza il campo: prima che ci fosse, chi
    /// entrava faceva tutto.
    ///
    /// Una funzione e non un `Default`: un livello dimenticato nel codice di
    /// chi ospita non deve diventare «tutto» senza che nessuno l'abbia scritto.
    #[must_use]
    pub const fn come_prima() -> Self {
        Self::Tutto
    }

    /// I gesti di questo ambito sono concessi.
    #[must_use]
    pub const fn concede(self, ambito: Ambito) -> bool {
        match (self, ambito) {
            (Self::Tutto, _) | (Self::Comanda, Ambito::Comandi) => true,
            (Self::Ascolta, _) | (Self::Comanda, Ambito::Coda) => false,
        }
    }
}

/// Di che cosa è fatto un gesto, per i [`Diritti`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ambito {
    /// Il trasporto: pausa, ripresa, avanti, indietro, salto, ripeti.
    Comandi,
    /// La coda: scegliere, togliere, spostare, aggiungere.
    Coda,
}

/// Come si ripete la coda della sessione.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Ripeti {
    /// Alla fine della coda ci si ferma.
    No,
    /// La voce corrente, da capo.
    Una,
    /// Tutta la coda, da capo.
    Tutta,
    /// Un modo che questo PC non conosce: vedi «Versioni».
    #[serde(other)]
    Sconosciuto,
}

/// Una voce della coda di sessione.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoceSulFilo {
    /// L'identità della voce nella sessione, assegnata dall'host e uguale su
    /// tutti i PC. È anche l'`etichetta` delle ancore del motore.
    pub uid: u64,
    /// Di chi è il brano: il [`Persona::membro`] del proprietario.
    pub di: u32,
    /// Il titolo.
    pub titolo: String,
    /// L'interprete.
    pub artista: String,
    /// L'album.
    pub album: String,
    /// Quanto dura, in millisecondi.
    pub durata_ms: u64,
    /// Il numero di traccia, se c'è.
    pub numero: Option<u32>,
    /// L'anno, se c'è.
    pub anno: Option<i32>,
    /// Il permesso di leggere la copertina, se ce n'è una.
    pub copertina: Option<Gettone>,
    /// Da dove si prendono i byte.
    pub sorgente: SorgenteSulFilo,
    /// La correzione ReplayGain del brano, in dB, se il file la dichiara.
    ///
    /// Viaggia perché la normalizzazione di chi ascolta la applichi allo
    /// stesso brano con lo stesso numero: senza, due PC normalizzerebbero lo
    /// stesso brano in due modi.
    pub replaygain_db: Option<f32>,
    /// Il picco dichiarato, per lo stesso motivo.
    pub picco: Option<f32>,
    /// Com'è fatto il file, perché chi ne ha una copia identica la usi al
    /// posto della rete (fase 3). `None` per i brani di catalogo.
    pub copia: Option<DescrittoreCopia>,
}

/// Da dove si prendono i byte di una voce.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tipo", rename_all = "camelCase")]
pub enum SorgenteSulFilo {
    /// Un file sul disco del proprietario, letto in rete col suo gettone.
    File {
        /// Il permesso di lettura.
        gettone: Gettone,
        /// L'estensione, per il riconoscimento del formato (`flac`, `mp3`).
        estensione: Option<String>,
        /// Quanti byte.
        dimensione: u64,
        /// Il tratto, se la voce è un brano di un'immagine CD: un file, più
        /// voci, e le voci di un'immagine restano senza buchi fra loro.
        segmento: Option<SegmentoSulFilo>,
    },
    /// Un brano di catalogo: ogni PC lo apre per conto suo, passando dalla
    /// propria lista di cataloghi ammessi. I byte di un catalogo non si
    /// rilanciano da un PC all'altro.
    Catalogo {
        /// Il riferimento stabile, `riferimento_da_testo` lo sa leggere.
        riferimento: String,
    },
    /// Una sorgente che questo PC non conosce: la voce si mostra ma non si
    /// suona. Vedi «Versioni».
    #[serde(other)]
    Sconosciuta,
}

/// Il tratto di un'immagine CD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentoSulFilo {
    /// Da dove, in millisecondi dall'inizio del file.
    pub inizio_ms: u64,
    /// Fino a dove; `None` fino alla fine del file.
    pub fine_ms: Option<u64>,
}

/// Com'è fatto il file del proprietario (fase 3).
///
/// Quel che basta a chi ascolta per riconoscere una copia **identica** sul
/// proprio disco, senza mandare in rete né il file né il suo percorso — tranne
/// quando il percorso è una condivisione di rete, che è lo stesso file per
/// tutti e due.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescrittoreCopia {
    /// Il percorso, **solo** se è una condivisione `\\server\…`.
    pub unc: Option<String>,
    /// Quanti byte.
    pub dimensione: u64,
    /// L'ultima modifica, in millisecondi dall'epoca Unix.
    pub modificato_ms: i64,
    /// blake3 di dimensione ‖ primi 64 KiB ‖ ultimi 64 KiB, in esadecimale.
    pub impronta_rapida: String,
    /// La chiave di contenuto della libreria, se il brano ne ha una.
    pub chiave_contenuto: Option<String>,
}

/// «Il punto `posizione_us` della voce `etichetta` si sente all'istante
/// `udito_ns`», sull'orologio dell'host.
///
/// Con `in_corso` falso l'host è fermo, e `posizione_us` è dove ripartirà.
/// Con `udito_ns` nel futuro è una partenza programmata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AncoraSulFilo {
    /// L'`uid` della voce.
    pub etichetta: u64,
    /// Il punto, in microsecondi dall'inizio della voce.
    pub posizione_us: u64,
    /// L'istante, in nanosecondi sulla base dei tempi dell'host.
    pub udito_ns: u64,
    /// L'host sta suonando.
    pub in_corso: bool,
    /// L'ancora è un avvio programmato dall'host (ripresa, salto o cambio di
    /// voce annunciati in anticipo), non la posizione di una voce che suona già:
    /// chi segue non aspetta il passaggio gapless e riapre subito.
    #[serde(default)]
    pub programmata: bool,
}

/// Un gesto di un membro (fase 2). Ogni voce si nomina per `uid`, mai per
/// posizione: fra il gesto e il suo arrivo la coda può essere cambiata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tipo", rename_all = "camelCase")]
pub enum Azione {
    /// Pausa.
    Pausa,
    /// Ripresa.
    Riprendi,
    /// La voce dopo. `da` è la voce che il membro vedeva suonare: se nel
    /// frattempo è già cambiata, il gesto è già stato fatto da qualcun altro
    /// e non si ripete.
    Prossimo {
        /// La voce che il membro vedeva.
        da: Option<u64>,
    },
    /// La voce prima, o l'inizio di questa: come il tasto del lettore.
    Precedente {
        /// La voce che il membro vedeva.
        da: Option<u64>,
    },
    /// Un salto dentro la voce `etichetta`.
    VaiA {
        /// La voce in cui saltare.
        etichetta: u64,
        /// Dove, in millisecondi.
        ms: u64,
    },
    /// Fa suonare quella voce della coda.
    Vai {
        /// La voce.
        uid: u64,
    },
    /// Toglie quella voce dalla coda.
    Togli {
        /// La voce.
        uid: u64,
    },
    /// Sposta una voce dopo un'altra; `dopo: None` la mette in testa.
    Sposta {
        /// La voce da spostare.
        uid: u64,
        /// Dopo quale voce.
        dopo: Option<u64>,
    },
    /// Aggiunge voci del membro alla coda della sessione.
    Aggiungi {
        /// Le voci, con `uid` scelti dal membro: l'host le rinumera.
        voci: Vec<VoceSulFilo>,
        /// In fondo o subito dopo la corrente.
        dove: Dove,
        /// La prima delle voci aggiunte parte subito: è il doppio clic di chi
        /// segue su un suo brano. L'`uid` lo sceglie l'host, quindi chi segue
        /// non potrebbe chiederlo con un [`Azione::Vai`] separato.
        ///
        /// `default`: un messaggio senza il campo vale «non partire».
        #[serde(default)]
        suona: bool,
    },
    /// Come si ripete.
    Ripeti {
        /// Il modo nuovo.
        modo: Ripeti,
    },
    /// Un gesto che questo PC non conosce: chi ospita risponde
    /// `insieme.gestoNonDisponibile`. Vedi «Versioni».
    #[serde(other)]
    Sconosciuta,
}

/// Dove vanno le voci aggiunte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Dove {
    /// In fondo alla coda.
    InFondo,
    /// Subito dopo la voce corrente.
    Dopo,
    /// Un posto che questo PC non conosce: vedi «Versioni».
    #[serde(other)]
    Sconosciuto,
}

// ── le stampe di prova ──────────────────────────────────────────────────────
//
// Scritte a mano per i tipi che portano nomi di PC, titoli, interpreti,
// album, riferimenti di catalogo e percorsi: una stampa di prova finisce nel
// diario, e il diario non li vuole (vedi il diario nel piano). Restano i
// numeri, che servono a capire un guasto senza dire che cosa si ascoltava.
// `Messaggio` e `StatoSessione` restano derivati: stampano i loro campi con
// queste.

impl std::fmt::Debug for Persona {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Persona")
            .field("membro", &self.membro)
            .field("diritti", &self.diritti)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for VoceSulFilo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VoceSulFilo")
            .field("uid", &self.uid)
            .field("di", &self.di)
            .field("durata_ms", &self.durata_ms)
            .field("sorgente", &self.sorgente)
            .field("copia", &self.copia.is_some())
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for SorgenteSulFilo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File {
                dimensione,
                segmento,
                ..
            } => f
                .debug_struct("File")
                .field("dimensione", dimensione)
                .field("segmento", segmento)
                .finish_non_exhaustive(),
            Self::Catalogo { .. } => f.debug_struct("Catalogo").finish_non_exhaustive(),
            Self::Sconosciuta => f.write_str("Sconosciuta"),
        }
    }
}

impl std::fmt::Debug for DescrittoreCopia {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DescrittoreCopia")
            .field("dimensione", &self.dimensione)
            .field("unc", &self.unc.is_some())
            .finish_non_exhaustive()
    }
}

/// Perché un messaggio è stato rifiutato da [`Messaggio::valida`]: il campo, e
/// nient'altro. Finisce nel diario di chi lo riceve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fuori(pub &'static str);

impl Messaggio {
    /// Ogni tetto di questo modulo, controllato prima di usare il messaggio.
    ///
    /// La cornice ha già limitato la dimensione in byte; qui si limitano le
    /// cose che un JSON piccolo può comunque gonfiare: mille titoli lunghi, un
    /// nome di PC di diecimila caratteri, una durata di un secolo.
    ///
    /// # Errori
    ///
    /// Il primo campo fuori dai tetti.
    pub fn valida(&self) -> Result<(), Fuori> {
        match self {
            Self::Stato(stato) => stato.valida(),
            Self::Rifiutato { codice } => testo(codice, 64, "codice"),
            Self::Fine { codice } | Self::Esito { codice, .. } => match codice {
                Some(codice) => testo(codice, 64, "codice"),
                None => Ok(()),
            },
            Self::Comando { azione, .. } => azione.valida(),
            Self::Invito { .. }
            | Self::RispostaInvito { .. }
            | Self::Chiedi
            | Self::NonChiedoPiu
            | Self::Ammesso { .. }
            | Self::Esco
            | Self::Ancora(_)
            | Self::Sconosciuto => Ok(()),
        }
    }
}

impl StatoSessione {
    fn valida(&self) -> Result<(), Fuori> {
        if self.persone.len() > MASSIMO_PERSONE {
            return Err(Fuori("persone"));
        }
        for persona in &self.persone {
            testo(&persona.nome, MASSIMO_NOME, "nome")?;
        }
        if self.coda.len() > MASSIMO_VOCI {
            return Err(Fuori("coda"));
        }
        for voce in &self.coda {
            voce.valida()?;
        }
        Ok(())
    }
}

impl VoceSulFilo {
    fn valida(&self) -> Result<(), Fuori> {
        testo(&self.titolo, MASSIMO_TESTO, "titolo")?;
        testo(&self.artista, MASSIMO_TESTO, "artista")?;
        testo(&self.album, MASSIMO_TESTO, "album")?;
        if self.durata_ms > MASSIMA_DURATA_MS {
            return Err(Fuori("durata"));
        }
        if self
            .replaygain_db
            .is_some_and(|db| !db.is_finite() || db.abs() > 60.0)
            || self
                .picco
                .is_some_and(|p| !p.is_finite() || !(0.0..=16.0).contains(&p))
        {
            return Err(Fuori("replaygain"));
        }
        match &self.sorgente {
            SorgenteSulFilo::File {
                estensione,
                segmento,
                ..
            } => {
                if let Some(estensione) = estensione {
                    testo(estensione, 16, "estensione")?;
                }
                if let Some(SegmentoSulFilo { inizio_ms, fine_ms }) = segmento
                    && (*inizio_ms > MASSIMA_DURATA_MS
                        || fine_ms.is_some_and(|fine| fine <= *inizio_ms))
                {
                    return Err(Fuori("segmento"));
                }
            }
            SorgenteSulFilo::Catalogo { riferimento } => {
                testo(riferimento, MASSIMO_RIFERIMENTO, "riferimento")?;
            }
            // Niente da controllare, e niente che si userà: vedi «Versioni».
            SorgenteSulFilo::Sconosciuta => {}
        }
        if let Some(copia) = &self.copia {
            if let Some(unc) = &copia.unc {
                testo(unc, MASSIMO_RIFERIMENTO, "unc")?;
                if !unc_valido(unc) {
                    return Err(Fuori("unc"));
                }
            }
            testo(&copia.impronta_rapida, 64, "impronta")?;
            if let Some(chiave) = &copia.chiave_contenuto {
                testo(chiave, 128, "chiave")?;
            }
        }
        Ok(())
    }
}

impl Azione {
    fn valida(&self) -> Result<(), Fuori> {
        match self {
            Self::Aggiungi { voci, .. } => {
                if voci.len() > MASSIMO_AGGIUNTE {
                    return Err(Fuori("aggiunte"));
                }
                voci.iter().try_for_each(VoceSulFilo::valida)
            }
            Self::Pausa
            | Self::Riprendi
            | Self::Prossimo { .. }
            | Self::Precedente { .. }
            | Self::VaiA { .. }
            | Self::Vai { .. }
            | Self::Togli { .. }
            | Self::Sposta { .. }
            | Self::Ripeti { .. }
            | Self::Sconosciuta => Ok(()),
        }
    }

    /// Questo PC capisce tutto il gesto: né il gesto né una sua parte (il
    /// modo di [`Azione::Ripeti`], il posto e le sorgenti delle voci di
    /// [`Azione::Aggiungi`]) arrivano da una versione più nuova.
    ///
    /// Chi ospita risponde a un gesto che non capisce con un `Esito` col
    /// codice `insieme.gestoNonDisponibile`, invece di farne metà o di
    /// tacere: chi l'ha chiesto aspetta una risposta. Vedi «Versioni».
    #[must_use]
    pub fn conosciuta(&self) -> bool {
        match self {
            Self::Sconosciuta
            | Self::Ripeti {
                modo: Ripeti::Sconosciuto,
            } => false,
            Self::Aggiungi { voci, dove, .. } => {
                *dove != Dove::Sconosciuto
                    && voci
                        .iter()
                        .all(|v| !matches!(v.sorgente, SorgenteSulFilo::Sconosciuta))
            }
            Self::Pausa
            | Self::Riprendi
            | Self::Prossimo { .. }
            | Self::Precedente { .. }
            | Self::VaiA { .. }
            | Self::Vai { .. }
            | Self::Togli { .. }
            | Self::Sposta { .. }
            | Self::Ripeti { .. } => true,
        }
    }

    /// Di che cosa è fatto il gesto, per i [`Diritti`] di chi lo manda.
    /// `None` per un gesto che questo PC non conosce, che si rifiuta prima:
    /// vedi [`Azione::conosciuta`].
    #[must_use]
    pub const fn ambito(&self) -> Option<Ambito> {
        match self {
            Self::Pausa
            | Self::Riprendi
            | Self::Prossimo { .. }
            | Self::Precedente { .. }
            | Self::VaiA { .. }
            | Self::Ripeti { .. } => Some(Ambito::Comandi),
            Self::Vai { .. } | Self::Togli { .. } | Self::Sposta { .. } | Self::Aggiungi { .. } => {
                Some(Ambito::Coda)
            }
            Self::Sconosciuta => None,
        }
    }
}

/// Una condivisione di rete nella forma `\\server\share\…`: server e
/// condivisione non vuoti, e niente prefissi di dispositivo.
///
/// `\\?\` e `\\.\` cominciano anche loro con due barre, ma non sono una
/// condivisione: sono i prefissi con cui Windows apre un percorso locale
/// senza normalizzarlo (`\\?\C:\…`) o un dispositivo (`\\.\pipe\…`,
/// `\\.\PhysicalDrive0`). Il descrittore dice dove cercare una copia sul
/// disco di chi ascolta; un host che potesse indicare uno di quei percorsi
/// gli farebbe aprire una cosa sua.
fn unc_valido(unc: &str) -> bool {
    let Some(resto) = unc.strip_prefix(r"\\") else {
        return false;
    };
    let mut pezzi = resto.split('\\');
    let server = pezzi.next().unwrap_or_default();
    let condivisione = pezzi.next().unwrap_or_default();
    !server.is_empty()
        && server != "?"
        && server != "."
        && !condivisione.is_empty()
        && !server.contains('/')
}

/// Un testo entro il suo tetto, in caratteri, e senza caratteri di controllo:
/// un a capo in un titolo non è un titolo, è un tentativo di impaginare lo
/// schermo di un altro.
fn testo(valore: &str, tetto: usize, campo: &'static str) -> Result<(), Fuori> {
    if valore.chars().count() > tetto || valore.chars().any(char::is_control) {
        return Err(Fuori(campo));
    }
    Ok(())
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Una voce dentro tutti i tetti, da guastare un campo alla volta.
    fn voce() -> VoceSulFilo {
        VoceSulFilo {
            uid: 1,
            di: 0,
            titolo: "Titolo".to_owned(),
            artista: "Interprete".to_owned(),
            album: "Album".to_owned(),
            durata_ms: 200_000,
            numero: Some(1),
            anno: Some(1999),
            copertina: None,
            sorgente: SorgenteSulFilo::File {
                gettone: Gettone([1; 16]),
                estensione: Some("flac".to_owned()),
                dimensione: 1_000,
                segmento: Some(SegmentoSulFilo {
                    inizio_ms: 0,
                    fine_ms: Some(1_000),
                }),
            },
            replaygain_db: Some(-6.5),
            picco: Some(0.98),
            copia: Some(DescrittoreCopia {
                unc: Some(r"\\nas\musica\a.flac".to_owned()),
                dimensione: 1_000,
                modificato_ms: 1,
                impronta_rapida: "ab".repeat(32),
                chiave_contenuto: Some("chiave".to_owned()),
            }),
        }
    }

    fn stato(coda: Vec<VoceSulFilo>, persone: usize) -> Messaggio {
        Messaggio::Stato(StatoSessione {
            versione: 1,
            persone: (0..persone)
                .map(|i| Persona {
                    membro: u32::try_from(i).unwrap_or(0),
                    nome: format!("PC {i}"),
                    diritti: Diritti::Comanda,
                })
                .collect(),
            coda,
            corrente: Some(1),
            in_pausa: false,
            ripeti: Ripeti::No,
        })
    }

    /// Il campo fuori, per una voce guastata da `guasta`.
    fn fuori(guasta: impl FnOnce(&mut VoceSulFilo)) -> Result<(), Fuori> {
        let mut v = voce();
        guasta(&mut v);
        stato(vec![v], 1).valida()
    }

    #[test]
    fn una_sessione_piena_e_dentro_i_tetti_passa() {
        let coda = vec![voce(); MASSIMO_VOCI];
        assert_eq!(stato(coda, MASSIMO_PERSONE).valida(), Ok(()));
    }

    #[test]
    fn persone_e_voci_oltre_il_tetto_non_passano() {
        assert_eq!(
            stato(vec![voce()], MASSIMO_PERSONE + 1).valida(),
            Err(Fuori("persone"))
        );
        assert_eq!(
            stato(vec![voce(); MASSIMO_VOCI + 1], 1).valida(),
            Err(Fuori("coda"))
        );
        let mut lungo = stato(vec![], 1);
        if let Messaggio::Stato(s) = &mut lungo
            && let Some(p) = s.persone.first_mut()
        {
            p.nome = "x".repeat(MASSIMO_NOME + 1);
        }
        assert_eq!(lungo.valida(), Err(Fuori("nome")));
    }

    #[test]
    fn ogni_testo_della_voce_ha_il_suo_tetto_e_niente_caratteri_di_controllo() {
        let troppo = "é".repeat(MASSIMO_TESTO + 1);
        assert_eq!(fuori(|v| v.titolo = troppo.clone()), Err(Fuori("titolo")));
        assert_eq!(fuori(|v| v.artista = troppo.clone()), Err(Fuori("artista")));
        assert_eq!(fuori(|v| v.album = troppo.clone()), Err(Fuori("album")));
        assert_eq!(
            fuori(|v| v.titolo = "é".repeat(MASSIMO_TESTO)),
            Ok(()),
            "il tetto è in caratteri, non in byte"
        );
        assert_eq!(
            fuori(|v| v.titolo = "a\nb".to_owned()),
            Err(Fuori("titolo"))
        );
        assert_eq!(
            fuori(|v| v.artista = "a\u{1b}[31m".to_owned()),
            Err(Fuori("artista")),
            "niente sequenze di terminale"
        );
    }

    #[test]
    fn durata_replaygain_e_picco_hanno_i_loro_tetti() {
        assert_eq!(fuori(|v| v.durata_ms = MASSIMA_DURATA_MS), Ok(()));
        assert_eq!(
            fuori(|v| v.durata_ms = MASSIMA_DURATA_MS + 1),
            Err(Fuori("durata"))
        );
        for db in [f32::NAN, f32::INFINITY, 60.5, -61.0] {
            assert_eq!(
                fuori(|v| v.replaygain_db = Some(db)),
                Err(Fuori("replaygain")),
                "{db}"
            );
        }
        for picco in [f32::NAN, -0.1, 16.1] {
            assert_eq!(
                fuori(|v| v.picco = Some(picco)),
                Err(Fuori("replaygain")),
                "{picco}"
            );
        }
    }

    #[test]
    fn la_sorgente_ha_i_suoi_tetti() {
        assert_eq!(
            fuori(|v| {
                if let SorgenteSulFilo::File { estensione, .. } = &mut v.sorgente {
                    *estensione = Some("x".repeat(17));
                }
            }),
            Err(Fuori("estensione"))
        );
        assert_eq!(
            fuori(|v| {
                if let SorgenteSulFilo::File { segmento, .. } = &mut v.sorgente {
                    *segmento = Some(SegmentoSulFilo {
                        inizio_ms: 500,
                        fine_ms: Some(500),
                    });
                }
            }),
            Err(Fuori("segmento")),
            "un tratto vuoto"
        );
        assert_eq!(
            fuori(|v| {
                if let SorgenteSulFilo::File { segmento, .. } = &mut v.sorgente {
                    *segmento = Some(SegmentoSulFilo {
                        inizio_ms: MASSIMA_DURATA_MS + 1,
                        fine_ms: None,
                    });
                }
            }),
            Err(Fuori("segmento"))
        );
        assert_eq!(
            fuori(|v| {
                v.sorgente = SorgenteSulFilo::Catalogo {
                    riferimento: "x".repeat(MASSIMO_RIFERIMENTO + 1),
                };
            }),
            Err(Fuori("riferimento"))
        );
        assert_eq!(
            fuori(|v| {
                v.sorgente = SorgenteSulFilo::Catalogo {
                    riferimento: "catalogo:123".to_owned(),
                };
            }),
            Ok(())
        );
    }

    /// Una voce col descrittore che punta lì.
    fn copia_con(unc: &str) -> Result<(), Fuori> {
        let unc = unc.to_owned();
        fuori(move |v| {
            if let Some(c) = &mut v.copia {
                c.unc = Some(unc);
            }
        })
    }

    #[test]
    fn il_descrittore_della_copia_ha_i_suoi_tetti() {
        let copia = |guasta: fn(&mut DescrittoreCopia)| {
            fuori(|v| {
                if let Some(c) = &mut v.copia {
                    guasta(c);
                }
            })
        };
        assert_eq!(
            copia(|c| c.unc = Some(r"C:\musica\a.flac".to_owned())),
            Err(Fuori("unc")),
            "solo condivisioni di rete"
        );
        assert_eq!(
            copia(|c| c.unc = Some(format!(r"\\{}", "x".repeat(MASSIMO_RIFERIMENTO)))),
            Err(Fuori("unc"))
        );
        // Prefissi di dispositivo, e condivisioni senza condivisione: la
        // revisione del 26 ottobre li aveva visti passare.
        for storto in [
            r"\\?\C:\musica\a.flac",
            r"\\.\pipe\x",
            r"\\.\PhysicalDrive0",
            r"\\server",
            r"\\server\",
            r"\\\condivisione\a.flac",
            r"\\server/condivisione\a.flac",
        ] {
            assert_eq!(copia_con(storto), Err(Fuori("unc")), "{storto}");
        }
        assert_eq!(copia_con(r"\\nas\musica\a.flac"), Ok(()));
        assert_eq!(copia_con(r"\\192.168.1.5\musica"), Ok(()));
        assert_eq!(
            copia(|c| c.impronta_rapida = "a".repeat(65)),
            Err(Fuori("impronta"))
        );
        assert_eq!(
            copia(|c| c.chiave_contenuto = Some("x".repeat(129))),
            Err(Fuori("chiave"))
        );
    }

    #[test]
    fn i_codici_e_i_gesti_hanno_i_loro_tetti() {
        let lungo = "x".repeat(65);
        assert_eq!(
            Messaggio::Rifiutato {
                codice: lungo.clone()
            }
            .valida(),
            Err(Fuori("codice"))
        );
        assert_eq!(
            Messaggio::Fine {
                codice: Some(lungo.clone())
            }
            .valida(),
            Err(Fuori("codice"))
        );
        assert_eq!(
            Messaggio::Esito {
                id: 1,
                codice: Some(lungo)
            }
            .valida(),
            Err(Fuori("codice"))
        );
        assert_eq!(Messaggio::Fine { codice: None }.valida(), Ok(()));
        let aggiungi = |quante: usize| Messaggio::Comando {
            id: 1,
            azione: Azione::Aggiungi {
                voci: vec![voce(); quante],
                dove: Dove::InFondo,
                suona: false,
            },
        };
        assert_eq!(aggiungi(MASSIMO_AGGIUNTE).valida(), Ok(()));
        assert_eq!(
            aggiungi(MASSIMO_AGGIUNTE + 1).valida(),
            Err(Fuori("aggiunte"))
        );
        let mut storta = voce();
        storta.titolo = "\u{7}".to_owned();
        assert_eq!(
            Messaggio::Comando {
                id: 1,
                azione: Azione::Aggiungi {
                    voci: vec![storta],
                    dove: Dove::Dopo,
                    suona: false,
                },
            }
            .valida(),
            Err(Fuori("titolo")),
            "anche le voci aggiunte da un membro"
        );
    }

    #[test]
    fn un_messaggio_va_e_torna_uguale_e_uno_sconosciuto_si_riconosce() {
        let messaggio = stato(vec![voce()], 2);
        let json = serde_json::to_string(&messaggio).expect("si serializza");
        assert_eq!(
            serde_json::from_str::<Messaggio>(&json).expect("si legge"),
            messaggio
        );
        assert_eq!(
            serde_json::from_str::<Messaggio>(r#"{"tipo":"dalFuturo","x":[1,2]}"#)
                .expect("si legge"),
            Messaggio::Sconosciuto
        );
        assert!(
            !json.contains("01010101010101010101010101010101") || json.contains("gettone"),
            "il gettone viaggia solo nel suo campo"
        );
        assert_eq!(
            format!("{:?}", Gettone([7; 16])),
            "Gettone(…)",
            "e nel diario non si stampa"
        );
    }

    /// Il messaggio letto da quel JSON, e già passato da `valida`.
    fn letto(json: &str) -> Messaggio {
        let messaggio: Messaggio = serde_json::from_str(json).expect("si legge");
        assert_eq!(messaggio.valida(), Ok(()), "{json}");
        messaggio
    }

    #[test]
    fn le_parti_di_una_versione_piu_nuova_si_leggono_invece_di_far_cadere_il_messaggio() {
        // Prima della revisione del 26 ottobre ognuno di questi faceva
        // fallire la lettura di tutto il messaggio, e la connessione si
        // chiudeva con `insieme.protocollo`.
        let gesto = letto(r#"{"tipo":"comando","id":7,"azione":{"tipo":"dalFuturo","x":[1]}}"#);
        assert_eq!(
            gesto,
            Messaggio::Comando {
                id: 7,
                azione: Azione::Sconosciuta
            }
        );
        let modo = letto(r#"{"tipo":"comando","id":8,"azione":{"tipo":"ripeti","modo":"aCaso"}}"#);
        assert_eq!(
            modo,
            Messaggio::Comando {
                id: 8,
                azione: Azione::Ripeti {
                    modo: Ripeti::Sconosciuto
                }
            }
        );
        let posto = letto(
            r#"{"tipo":"comando","id":9,"azione":{"tipo":"aggiungi","voci":[],"dove":"inMezzo"}}"#,
        );
        assert!(matches!(
            posto,
            Messaggio::Comando {
                azione: Azione::Aggiungi {
                    dove: Dove::Sconosciuto,
                    ..
                },
                ..
            }
        ));
        // Uno stato con una voce di una sorgente nuova e un modo di ripetere
        // nuovo: lo stato passa, e la voce si riconosce.
        let mut v = voce();
        v.sorgente = SorgenteSulFilo::Catalogo {
            riferimento: "segnaposto".to_owned(),
        };
        let mut json = serde_json::to_string(&stato(vec![v], 1)).expect("si serializza");
        json = json
            .replace(
                r#"{"tipo":"catalogo","riferimento":"segnaposto"}"#,
                r#"{"tipo":"radio","indirizzo":"http://x"}"#,
            )
            .replace(r#""ripeti":"no""#, r#""ripeti":"aCaso""#);
        let Messaggio::Stato(stato_letto) = letto(&json) else {
            panic!("uno stato");
        };
        assert_eq!(stato_letto.ripeti, Ripeti::Sconosciuto);
        assert!(matches!(
            stato_letto.coda.first().map(|v| &v.sorgente),
            Some(SorgenteSulFilo::Sconosciuta)
        ));
    }

    #[test]
    fn un_ancora_senza_programmata_non_e_programmata() {
        let ancora = letto(
            r#"{"tipo":"ancora","etichetta":3,"posizioneUs":10,"uditoNs":20,"inCorso":true}"#,
        );
        assert_eq!(
            ancora,
            Messaggio::Ancora(AncoraSulFilo {
                etichetta: 3,
                posizione_us: 10,
                udito_ns: 20,
                in_corso: true,
                programmata: false,
            })
        );
        let programmata = Messaggio::Ancora(AncoraSulFilo {
            etichetta: 3,
            posizione_us: 0,
            udito_ns: 20,
            in_corso: true,
            programmata: true,
        });
        let json = serde_json::to_string(&programmata).expect("si serializza");
        assert!(json.contains(r#""programmata":true"#), "{json}");
        assert_eq!(letto(&json), programmata);
    }

    #[test]
    fn un_gesto_con_una_parte_sconosciuta_non_e_conosciuto() {
        let aggiungi = |dove: Dove, sorgente: SorgenteSulFilo| {
            let mut v = voce();
            v.sorgente = sorgente;
            Azione::Aggiungi {
                voci: vec![voce(), v],
                dove,
                suona: false,
            }
        };
        let catalogo = || SorgenteSulFilo::Catalogo {
            riferimento: "catalogo:1".to_owned(),
        };
        assert!(Azione::Pausa.conosciuta());
        assert!(Azione::Ripeti { modo: Ripeti::Una }.conosciuta());
        assert!(aggiungi(Dove::Dopo, catalogo()).conosciuta());
        assert!(!Azione::Sconosciuta.conosciuta());
        assert!(
            !Azione::Ripeti {
                modo: Ripeti::Sconosciuto
            }
            .conosciuta()
        );
        assert!(!aggiungi(Dove::Sconosciuto, catalogo()).conosciuta());
        assert!(!aggiungi(Dove::InFondo, SorgenteSulFilo::Sconosciuta).conosciuta());
        // E vanno e tornano: una variante sconosciuta rimandata resta tale.
        for azione in [
            Azione::Sconosciuta,
            Azione::Ripeti {
                modo: Ripeti::Sconosciuto,
            },
        ] {
            let json = serde_json::to_string(&azione).expect("si serializza");
            assert_eq!(
                serde_json::from_str::<Azione>(&json).expect("si legge"),
                azione
            );
        }
    }

    /// Un host di prima non manda i diritti: chi entra fa tutto, come allora.
    #[test]
    fn senza_i_diritti_si_fa_tutto_come_prima() {
        assert_eq!(
            letto(r#"{"tipo":"ammesso","membro":3}"#),
            Messaggio::Ammesso {
                membro: 3,
                diritti: Diritti::Tutto,
            }
        );
        let Messaggio::Stato(stato_letto) = letto(
            r#"{"tipo":"stato","versione":1,"persone":[{"membro":0,"nome":"A"}],"coda":[],"corrente":null,"inPausa":false,"ripeti":"no"}"#,
        ) else {
            panic!("uno stato");
        };
        assert_eq!(
            stato_letto.persone.first().map(|p| p.diritti),
            Some(Diritti::Tutto)
        );
    }

    /// Un livello che questo PC non conosce vale il più stretto: sbagliare per
    /// eccesso darebbe un comando che nessuno ha dato.
    #[test]
    fn un_livello_sconosciuto_vale_ascolta() {
        assert_eq!(
            letto(r#"{"tipo":"ammesso","membro":3,"diritti":"soloLaCoda"}"#),
            Messaggio::Ammesso {
                membro: 3,
                diritti: Diritti::Ascolta,
            }
        );
        for diritti in [Diritti::Ascolta, Diritti::Comanda, Diritti::Tutto] {
            let ammesso = Messaggio::Ammesso { membro: 1, diritti };
            let json = serde_json::to_string(&ammesso).expect("si serializza");
            assert_eq!(letto(&json), ammesso, "{json}");
        }
        assert_eq!(
            serde_json::to_string(&Diritti::Comanda).expect("si serializza"),
            r#""comanda""#
        );
    }

    #[test]
    fn ogni_livello_concede_il_suo_e_quelli_sotto() {
        use Ambito::{Coda, Comandi};
        assert!(!Diritti::Ascolta.concede(Comandi));
        assert!(!Diritti::Ascolta.concede(Coda));
        assert!(Diritti::Comanda.concede(Comandi));
        assert!(!Diritti::Comanda.concede(Coda));
        assert!(Diritti::Tutto.concede(Comandi));
        assert!(Diritti::Tutto.concede(Coda));
    }

    #[test]
    fn il_trasporto_e_dei_comandi_e_le_voci_della_coda() {
        let comandi = [
            Azione::Pausa,
            Azione::Riprendi,
            Azione::Prossimo { da: Some(1) },
            Azione::Precedente { da: None },
            Azione::VaiA {
                etichetta: 1,
                ms: 0,
            },
            Azione::Ripeti { modo: Ripeti::Una },
        ];
        for azione in &comandi {
            assert_eq!(azione.ambito(), Some(Ambito::Comandi), "{azione:?}");
        }
        let coda = [
            Azione::Vai { uid: 1 },
            Azione::Togli { uid: 1 },
            Azione::Sposta { uid: 1, dopo: None },
            Azione::Aggiungi {
                voci: vec![voce()],
                dove: Dove::InFondo,
                suona: false,
            },
        ];
        for azione in &coda {
            assert_eq!(azione.ambito(), Some(Ambito::Coda), "{azione:?}");
        }
        assert_eq!(Azione::Sconosciuta.ambito(), None);
    }
}
