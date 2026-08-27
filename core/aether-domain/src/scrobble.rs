//! L'ascolto da mandare fuori, e a chi.
//!
//! # Perché sta nel dominio e non nel crate che parla con i servizi
//!
//! Stessa ragione per cui `AccountSnapshot` sta qui e non in `aether-archivio`:
//! è il **perno** fra due parti che non devono conoscersi. Chi lo produce è la
//! coda in `aether-app`, che vede il database e non la rete; chi lo consuma è
//! `aether-scrobble`, che vede la rete e non il database. Se il tipo vivesse di
//! là, `aether-app` dipenderebbe da `aether-scrobble` e con lui da
//! `aether-net` — e la riga che apre il suo `Cargo.toml`, «questo crate non fa
//! richieste da sé», smetterebbe di essere vera per il gusto di risparmiare una
//! conversione di dieci righe.
//!
//! Qui invece i due crate si incontrano su un valore puro, e nessuno dei due
//! sale sull'altro.
//!
//! # Cosa questo modulo non decide
//!
//! Se un ascolto conta. Quella regola è una sola in tutta l'applicazione e sta
//! in [`crate::listen::counts_as_play`] — metà brano o quattro minuti, mai sotto
//! i trenta secondi — ed è la stessa che alimenta `play_count` e la cronologia.
//! Ce ne fosse una seconda qui, tornerebbe il difetto che `listen.rs` è nato per
//! correggere: due misure dello stesso ascolto che non concordano.

/// A chi si mandano gli ascolti.
///
/// Un `enum` e non un tratto: i due servizi non hanno la stessa forma — uno
/// firma ogni richiesta e l'altro no, uno accetta mille ascolti per volta e
/// l'altro cinquanta, uno vuole un consenso nel browser e l'altro un token
/// incollato — e un tratto che li coprisse tutti e due avrebbe un metodo per
/// servizio. Questo tipo serve a **nominarli**: sta nella colonna `service`
/// della coda, negli errori e nelle impostazioni, ed è l'unico posto in cui i
/// due nomi sono scritti.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Servizio {
    /// ListenBrainz, di MetaBrainz.
    ListenBrainz,
    /// Last.fm.
    LastFm,
}

impl Servizio {
    /// Il nome stabile: attraversa il database, l'IPC e gli errori.
    #[must_use]
    pub const fn chiave(self) -> &'static str {
        match self {
            Self::ListenBrainz => "listenbrainz",
            Self::LastFm => "lastfm",
        }
    }

    /// Il nome come lo scrive chi lo gestisce, per le schermate.
    #[must_use]
    pub const fn nome(self) -> &'static str {
        match self {
            Self::ListenBrainz => "ListenBrainz",
            Self::LastFm => "Last.fm",
        }
    }

    /// Dal nome stabile.
    #[must_use]
    pub fn da_chiave(chiave: &str) -> Option<Self> {
        Self::tutti().into_iter().find(|s| s.chiave() == chiave)
    }

    /// Tutti, in ordine di dichiarazione.
    #[must_use]
    pub const fn tutti() -> [Self; 2] {
        [Self::ListenBrainz, Self::LastFm]
    }
}

/// Un ascolto da mandare.
///
/// # Perché porta i tag con sé invece di un `track_id`
///
/// Perché fra l'ascolto e l'invio può passare del tempo — l'utente era offline,
/// il servizio era giù, l'applicazione era chiusa — e in quel tempo il brano può
/// essere stato cancellato, spostato da un riordino o ritaggato da un
/// arricchimento. Un ascolto che nomina una riga della libreria è un ascolto che
/// si perde quando quella riga sparisce, e che manda i tag di **oggi** per un
/// ascolto di ieri — cioè mente proprio nel caso in cui i tag sono stati
/// corretti nel frattempo.
///
/// Quel che va mandato è cosa si è ascoltato allora.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ascolto {
    /// L'artista. Obbligatorio per tutti e due i servizi.
    pub artista: String,
    /// Il titolo. Obbligatorio per tutti e due i servizi.
    pub titolo: String,
    /// L'album, quando c'è.
    pub album: Option<String>,
    /// L'artista dell'album, quando è dichiarato.
    pub artista_album: Option<String>,
    /// La durata, in millisecondi.
    pub durata_ms: Option<u64>,
    /// Il numero di traccia.
    pub numero_traccia: Option<u32>,
    /// L'identificativo MusicBrainz della registrazione.
    ///
    /// Quando c'è, i servizi smettono di indovinare: è la differenza fra un
    /// ascolto attribuito alla canzone giusta e uno attribuito a una cover
    /// omonima. Aether ce l'ha in `tracks.mb_recording_id` quando
    /// l'arricchimento è passato.
    pub mbid_registrazione: Option<String>,
    /// Quando l'ascolto è **cominciato**, in secondi dall'epoca.
    ///
    /// L'inizio, e tutti e due i protocolli lo vogliono così — è anche ciò che
    /// [`crate::listen::Listen::started_at`] contiene, per la ragione già
    /// scritta là: registrare la fine sposterebbe ogni ascolto avanti della
    /// durata del brano.
    ///
    /// Secondi e non millisecondi: è l'unità del protocollo di tutti e due, e
    /// convertire una volta sola nel punto in cui si legge dal database è meglio
    /// che ricordarsene in due posti che mandano.
    pub quando_s: i64,
}

impl Ascolto {
    /// Un ascolto con il minimo indispensabile.
    #[must_use]
    pub fn nuovo(artista: impl Into<String>, titolo: impl Into<String>, quando_s: i64) -> Self {
        Self {
            artista: artista.into(),
            titolo: titolo.into(),
            album: None,
            artista_album: None,
            durata_ms: None,
            numero_traccia: None,
            mbid_registrazione: None,
            quando_s,
        }
    }

    /// Questo ascolto è mandabile?
    ///
    /// Artista e titolo non vuoti: sono gli unici due campi obbligatori di tutti
    /// e due i protocolli. Uno vuoto non dà un guasto di rete — dà un `400` da
    /// ListenBrainz e un «ignorato, codice 1» da Last.fm, cioè una riga che
    /// resta in coda a ritentare per sempre oppure sparisce senza dire niente.
    /// Meglio non farla entrare: un file senza tag esiste, in ogni libreria.
    #[must_use]
    pub fn valido(&self) -> bool {
        !self.artista.trim().is_empty() && !self.titolo.trim().is_empty()
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn i_nomi_dei_servizi_vanno_e_tornano() {
        for servizio in Servizio::tutti() {
            assert_eq!(
                Servizio::da_chiave(servizio.chiave()),
                Some(servizio),
                "{} non torna dal suo nome",
                servizio.nome()
            );
        }
        assert_eq!(Servizio::da_chiave("libre.fm"), None);
    }

    #[test]
    fn un_ascolto_senza_artista_non_e_mandabile() {
        let mut a = Ascolto::nuovo("Colle der Fomento", "Anima e ghiaccio", 1_700_000_000);
        assert!(a.valido());

        a.artista = "   ".to_owned();
        assert!(
            !a.valido(),
            "uno spazio non è un artista: ListenBrainz risponde 400 e Last.fm lo ignora in silenzio"
        );

        a.artista = "Colle der Fomento".to_owned();
        a.titolo = String::new();
        assert!(!a.valido());
    }
}
