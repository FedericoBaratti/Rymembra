//! Lo strato del trasporto: quel che due Rymembra si dicono sotto i
//! [`Messaggio`], e che il desktop non vede mai.
//!
//! Vedi «Due strati, e chi li vede» in [`crate::messaggi`]. Stessa regola delle
//! versioni: un `tipo` che questo PC non conosce arriva come
//! [`Trasporto::Sconosciuto`] e si ignora.

use serde::{Deserialize, Serialize};

use crate::messaggi::{Gettone, Messaggio};

/// A che cosa serve una connessione. Lo dichiara chi la apre, nel saluto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Uso {
    /// Presenza e sessione: saluto, battito, orologio, associazione, e i
    /// [`Messaggio`]. Una per PC collegato.
    #[default]
    Controllo,
    /// I byte dei brani e delle copertine, dal membro verso l'host.
    Lettura,
    /// Fase 2: il membro la apre verso l'host, e l'host ci chiede i byte dei
    /// gettoni che quel membro serve.
    Servizio,
}

/// Trentadue byte, in esadecimale sul filo: nonce e impegni.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Byte32(pub(crate) [u8; 32]);

/// Un nonce stampato sarebbe un nonce copiabile: vale per i gettoni, vale qui.
impl std::fmt::Debug for Byte32 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Byte32(…)")
    }
}

impl Serialize for Byte32 {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&crate::esadecimale::scrivi(&self.0))
    }
}

impl<'de> Deserialize<'de> for Byte32 {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let testo = String::deserialize(d)?;
        crate::esadecimale::leggi::<32>(&testo)
            .map(Self)
            .ok_or_else(|| serde::de::Error::custom("trentadue byte non validi"))
    }
}

/// Un messaggio del trasporto.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tipo", rename_all = "camelCase")]
pub(crate) enum Trasporto {
    /// Il saluto, subito dopo la stretta, nei due sensi. Sempre il primo
    /// messaggio, e mai più dopo.
    ///
    /// La sua forma non cambia **mai**, nemmeno alzando [`crate::messaggi::VERSIONE`]:
    /// è il messaggio che dice la versione, e se cambiasse anche lui due PC di
    /// versioni diverse non riuscirebbero a dirsi che sono di versioni
    /// diverse.
    Ciao {
        /// La versione del protocollo di chi saluta.
        versione: u32,
        /// Il nome del suo PC.
        nome: String,
        /// Sta ospitando.
        ospita: bool,
        /// A che cosa serve la connessione; di serie, controllo.
        #[serde(default)]
        uso: Uso,
    },
    /// «Ci sono»: si manda dopo [`crate::Tempi::battito`] senza aver mandato
    /// altro.
    Battito,
    /// «Chiudo, e questo è il perché»: un codice del catalogo, mai una frase.
    Chiudo {
        /// `ErrorCodeKind::code()`, o niente.
        codice: Option<String>,
    },
    /// Un ping d'orologio. `t1` è l'istante di partenza sull'orologio di chi
    /// lo manda.
    Ping {
        /// Il numero del ping.
        seq: u32,
        /// La partenza.
        t1: u64,
    },
    /// La risposta a un [`Trasporto::Ping`]: `t2` preso appena il ping è
    /// arrivato, `t3` subito prima di cifrare questa risposta.
    Pong {
        /// Il numero del ping.
        seq: u32,
        /// La partenza del ping, ricopiata.
        t1: u64,
        /// L'arrivo del ping.
        t2: u64,
        /// La partenza di questo pong.
        t3: u64,
    },
    /// Associazione, dal membro: l'impegno sul suo nonce.
    Impegno {
        /// `H(nonce_membro ‖ chiavi)`.
        impegno: Byte32,
    },
    /// Associazione, dall'host: il suo nonce.
    Nonce {
        /// Il nonce dell'host.
        nonce: Byte32,
    },
    /// Associazione, dall'host al posto del [`Trasporto::Nonce`]: ne ha già
    /// una in corso con un altro PC, e ne tiene una sola alla volta (vedi
    /// `codice::Tentativi`). L'impegno non è stato preso, e la connessione
    /// resta: chi l'ha chiesta libera la sua prenotazione senza contarla, e
    /// riprova più tardi.
    ///
    /// Fino alla revisione del 26 ottobre al suo posto c'era un `Chiudo` con
    /// `insieme.troppiTentativi`: la connessione cadeva, e chi chiedeva
    /// contava come fallito un tentativo che non c'era stato — tre di fila, e
    /// restava fermo dieci minuti per colpa di un altro PC.
    Occupato,
    /// Associazione, dal membro: il nonce su cui si era impegnato.
    Rivela {
        /// Il nonce del membro.
        nonce: Byte32,
    },
    /// Associazione, dai due capi: la persona ha confermato il codice.
    Conferma {
        /// Coincide o no.
        uguale: bool,
    },
    /// Una lettura di byte di un brano.
    Leggi {
        /// Il numero della lettura, per la risposta.
        lettura: u32,
        /// Il permesso.
        gettone: Gettone,
        /// Da dove.
        da: u64,
        /// Quanti, al più `servente::LETTURA_MASSIMA`.
        quanti: u64,
    },
    /// La lettura di una copertina.
    Copertina {
        /// Il numero della lettura.
        lettura: u32,
        /// Il permesso.
        gettone: Gettone,
    },
    /// La risposta a un [`Trasporto::Leggi`]: seguono `byte` byte in uno o
    /// più messaggi [`crate::cornice::Tipo::Byte`], subito, senza niente in
    /// mezzo.
    Pezzo {
        /// Il numero della lettura.
        lettura: u32,
        /// Quanto è lungo tutto il file.
        totale: u64,
        /// Quanti byte seguono.
        byte: u64,
    },
    /// La risposta a una [`Trasporto::Copertina`]: seguono `byte` byte.
    Immagine {
        /// Il numero della lettura.
        lettura: u32,
        /// Quanti byte seguono.
        byte: u64,
    },
    /// Una lettura rifiutata, col suo codice del catalogo.
    LetturaRifiutata {
        /// Il numero della lettura.
        lettura: u32,
        /// `ErrorCodeKind::code()`.
        codice: String,
    },
    /// Un messaggio dell'applicazione.
    App {
        /// Il messaggio.
        messaggio: Messaggio,
    },
    /// Un tipo che questo PC non conosce: si ignora.
    #[serde(other)]
    Sconosciuto,
}

/// Un [`Messaggio`] da mandare, impacchettato senza copiarlo.
///
/// La stessa forma di [`Trasporto::App`] sul filo: `Rete::manda` riceve un
/// riferimento, e uno stato di sessione di mille voci clonato solo per
/// serializzarlo sarebbe un centinaio di kilobyte copiati quattro volte al
/// secondo.
#[derive(Serialize)]
#[serde(tag = "tipo", rename = "app")]
pub(crate) struct AppDaMandare<'a> {
    /// Il messaggio.
    pub(crate) messaggio: &'a Messaggio,
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn il_messaggio_applicativo_impacchettato_e_quello_che_si_legge() {
        let messaggio = Messaggio::Invito {
            invito: 7,
            scade_fra_ms: 60_000,
        };
        let json = serde_json::to_vec(&AppDaMandare {
            messaggio: &messaggio,
        })
        .expect("si serializza");
        let letto: Trasporto = serde_json::from_slice(&json).expect("si legge");
        assert_eq!(letto, Trasporto::App { messaggio });
    }

    #[test]
    fn un_tipo_nuovo_si_ignora_e_uno_nuovo_dentro_app_pure() {
        let ignoto: Trasporto =
            serde_json::from_str(r#"{"tipo":"dalFuturo","x":1}"#).expect("si legge");
        assert_eq!(ignoto, Trasporto::Sconosciuto);
        let dentro: Trasporto =
            serde_json::from_str(r#"{"tipo":"app","messaggio":{"tipo":"dalFuturo"}}"#)
                .expect("si legge");
        assert_eq!(
            dentro,
            Trasporto::App {
                messaggio: Messaggio::Sconosciuto
            }
        );
    }

    #[test]
    fn il_saluto_senza_uso_e_di_controllo() {
        let ciao: Trasporto =
            serde_json::from_str(r#"{"tipo":"ciao","versione":1,"nome":"PC","ospita":false}"#)
                .expect("si legge");
        assert_eq!(
            ciao,
            Trasporto::Ciao {
                versione: 1,
                nome: "PC".to_owned(),
                ospita: false,
                uso: Uso::Controllo
            }
        );
    }

    #[test]
    fn i_trentadue_byte_viaggiano_in_esadecimale() {
        let nonce = Trasporto::Nonce {
            nonce: Byte32([0xab; 32]),
        };
        let json = serde_json::to_string(&nonce).expect("si serializza");
        assert!(json.contains(&"ab".repeat(32)), "{json}");
        assert_eq!(
            serde_json::from_str::<Trasporto>(&json).expect("si legge"),
            nonce
        );
        assert!(
            serde_json::from_str::<Trasporto>(r#"{"tipo":"nonce","nonce":"abcd"}"#).is_err(),
            "corto"
        );
        assert!(
            !format!("{nonce:?}").contains("ab"),
            "nel diario non si stampa"
        );
    }
}
