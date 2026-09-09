//! Il decodificatore degli eventi che arrivano a pezzi.
//!
//! # Il problema, in una riga
//!
//! I confini dei blocchi che [`aether_net::http::Rete::flusso`] consegna non
//! hanno niente a che vedere con i confini degli eventi. Un blocco è
//! sessantaquattro kilobyte di socket; un evento è `data: {…}` seguito da una
//! riga vuota. Il primo può finire in mezzo alla parola `content`, e il secondo
//! non esiste finché la riga vuota non è arrivata.
//!
//! Chi tratta un blocco come un messaggio ottiene un JSON troncato ogni tanto —
//! «ogni tanto» che dipende dalla lunghezza della risposta, cioè il difetto che
//! si vede solo con le risposte lunghe e mai in prova.
//!
//! # Cosa fa questo tipo, e cosa non fa
//!
//! Accumula i byte finché non ha una riga intera, e consegna il **carico** dei
//! campi `data:` — la stringa dopo i due punti, senza lo spazio di cortesia.
//! Non sa cosa ci sia scritto dentro: qui non si interpreta nessun JSON, e
//! `[DONE]` è l'unico valore su cui questo modulo abbia un'opinione, perché è
//! l'unico che non è un carico ma la fine.
//!
//! Non gestisce `event:`, `id:` e `retry:` — li scarta come le righe di
//! commento — e non è una semplificazione azzardata: nessuno dei servizi
//! OpenAI-compatibili li manda su questo canale, e uno che li mandasse
//! metterebbe comunque il contenuto in `data:`.
//!
//! # Sui terminatori di riga
//!
//! La specifica ne ammette tre: l'a capo, il ritorno a capo seguito dall'a capo,
//! e il ritorno a capo da solo. I primi due si vedono davvero; il terzo no, ma
//! costa una riga trattarlo e costerebbe un pomeriggio di indagini non trattarlo.

/// Quanto può crescere l'avanzo prima che sia un difetto e non un evento lungo.
///
/// Un megabyte. Un evento di questo protocollo è qualche centinaio di byte; uno
/// da un megabyte vuol dire che dall'altra parte non c'è un flusso SSE — è una
/// pagina HTML, o un JSON solo, arrivato con lo stato giusto. Senza questo
/// limite quel caso diventa un `Vec` che cresce finché c'è memoria.
const AVANZO_MASSIMO: usize = 1024 * 1024;

/// Il carico di un evento, o il segnale che il flusso è finito.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Evento {
    /// Quel che c'era dopo `data:`, da interpretare altrove.
    Dati(String),
    /// `data: [DONE]`: il servizio ha finito di parlare.
    Fine,
}

/// Gli eventi che si stanno formando.
#[derive(Debug, Default)]
pub struct Sse {
    /// I byte arrivati dopo l'ultima riga completa.
    avanzo: Vec<u8>,
    /// Se si è già visto `[DONE]`.
    finito: bool,
    /// Se l'avanzo ha superato [`AVANZO_MASSIMO`] e si è smesso di accumulare.
    sfondato: bool,
    /// Quante righe intere e non vuote sono passate di qui.
    ///
    /// Comprese quelle che non producono un evento: i commenti, `event:`,
    /// `id:`. Serve a distinguere due guasti che si somigliano e non si curano
    /// allo stesso modo — dall'altra parte non c'era un flusso, oppure c'era e
    /// non ha detto niente. Un servizio che manda `: keep-alive` per venti
    /// secondi e poi chiude ha parlato questo protocollo benissimo; senza
    /// questo conto lo si accuserebbe di non conoscerlo.
    righe: usize,
}

impl Sse {
    /// Un decodificatore che non ha ancora visto niente.
    #[must_use]
    pub fn nuovo() -> Self {
        Self::default()
    }

    /// Gli eventi interi contenuti in questo pezzo.
    ///
    /// Quel che resta a metà rimane dentro ed esce al pezzo successivo. Un
    /// pezzo che non completa nessuna riga restituisce un elenco vuoto, che è
    /// la risposta giusta e non un guasto.
    pub fn mangia(&mut self, pezzo: &[u8]) -> Vec<Evento> {
        if self.sfondato {
            return Vec::new();
        }
        if self.avanzo.len().saturating_add(pezzo.len()) > AVANZO_MASSIMO {
            self.sfondato = true;
            self.avanzo = Vec::new();
            return Vec::new();
        }
        self.avanzo.extend_from_slice(pezzo);

        let mut fuori = Vec::new();
        let mut da = 0_usize;
        let mut i = 0_usize;
        while i < self.avanzo.len() {
            let byte = self.avanzo.get(i).copied().unwrap_or(0);
            if byte != b'\n' && byte != b'\r' {
                i = i.saturating_add(1);
                continue;
            }
            if let Some(riga) = self.avanzo.get(da..i) {
                if !riga.is_empty() {
                    self.righe = self.righe.saturating_add(1);
                }
                if let Some(evento) = leggi_riga(riga) {
                    if evento == Evento::Fine {
                        self.finito = true;
                    }
                    fuori.push(evento);
                }
            }
            // Il ritorno a capo seguito dall'a capo è **un** terminatore, non
            // due: senza questo salto la riga successiva comincerebbe con l'a
            // capo rimasto e sembrerebbe vuota.
            let doppio = byte == b'\r' && self.avanzo.get(i.saturating_add(1)) == Some(&b'\n');
            i = i.saturating_add(if doppio { 2 } else { 1 });
            da = i;
        }
        // Solo quel che non è ancora finito con un a capo resta per la prossima
        // volta. `drain` e non un `Vec` nuovo: l'avanzo è quasi sempre corto, e
        // riallocarlo a ogni blocco sarebbe una copia per niente.
        self.avanzo.drain(..da);
        fuori
    }

    /// Se il servizio ha detto `[DONE]`.
    ///
    /// Serve a distinguere una risposta **conclusa** da una troncata: un flusso
    /// che finisce senza questo è una connessione caduta a metà frase, e chi
    /// legge ha il diritto di saperlo.
    #[must_use]
    pub const fn finito(&self) -> bool {
        self.finito
    }

    /// Se si è smesso di accumulare perché quel che arrivava non era un flusso.
    #[must_use]
    pub const fn sfondato(&self) -> bool {
        self.sfondato
    }

    /// Quante righe intere e non vuote sono passate. Vedi il campo omonimo.
    #[must_use]
    pub const fn righe(&self) -> usize {
        self.righe
    }

    /// Quel che è rimasto a metà, come testo.
    ///
    /// Per il messaggio di un errore, non per interpretarlo: quando un servizio
    /// risponde con lo stato giusto ma con una pagina di errore invece di un
    /// flusso, questo è l'unico posto in cui quella pagina si può leggere.
    #[must_use]
    pub fn avanzo(&self) -> String {
        String::from_utf8_lossy(&self.avanzo).into_owned()
    }
}

/// Cosa significa una riga completa, se significa qualcosa.
///
/// `None` per le righe vuote — che separano gli eventi — per i commenti, che
/// cominciano con i due punti e che i servizi mandano come battito, e per i
/// campi che non sono `data`.
fn leggi_riga(riga: &[u8]) -> Option<Evento> {
    let riga = String::from_utf8_lossy(riga);
    let carico = riga.trim_end_matches(['\r', '\n']).strip_prefix("data:")?;
    // Un solo spazio, e opzionale: è la cortesia della specifica. Toglierne di
    // più corromperebbe un carico che comincia davvero con uno spazio.
    let carico = carico.strip_prefix(' ').unwrap_or(carico);
    if carico == "[DONE]" {
        return Some(Evento::Fine);
    }
    if carico.is_empty() {
        return None;
    }
    Some(Evento::Dati(carico.to_owned()))
}

#[cfg(test)]
mod prove {
    use super::*;

    fn dati(testi: &[&str]) -> Vec<Evento> {
        testi
            .iter()
            .map(|t| Evento::Dati((*t).to_owned()))
            .collect()
    }

    #[test]
    fn due_eventi_in_un_blocco_solo() {
        let mut sse = Sse::nuovo();
        assert_eq!(
            sse.mangia(b"data: {\"a\":1}\n\ndata: {\"b\":2}\n\n"),
            dati(&["{\"a\":1}", "{\"b\":2}"])
        );
    }

    /// Il difetto per cui questo tipo esiste: il blocco finisce in mezzo alla
    /// parola, e senza accumulo uscirebbe un JSON troncato.
    #[test]
    fn un_evento_spezzato_fra_due_blocchi_esce_intero() {
        let mut sse = Sse::nuovo();
        assert_eq!(sse.mangia(b"data: {\"cont"), Vec::new());
        assert_eq!(
            sse.mangia(b"ent\":\"ciao\"}\n\n"),
            dati(&["{\"content\":\"ciao\"}"])
        );
    }

    #[test]
    fn un_evento_spezzato_dentro_il_terminatore_non_diventa_due() {
        let mut sse = Sse::nuovo();
        assert_eq!(sse.mangia(b"data: uno\r"), dati(&["uno"]));
        // L'a capo che completa il terminatore non deve aprire una riga vuota.
        assert_eq!(sse.mangia(b"\ndata: due\r\n"), dati(&["due"]));
    }

    /// La distinzione fra i due modi di non ricevere niente: un servizio che
    /// manda solo battiti e poi chiude ha parlato questo protocollo benissimo,
    /// e va detto che ha taciuto, non che non lo conosce.
    #[test]
    fn i_battiti_contano_come_righe_anche_se_non_sono_eventi() {
        let mut sse = Sse::nuovo();
        assert_eq!(
            sse.mangia(
                b": OPENROUTER PROCESSING

"
            ),
            Vec::new()
        );
        assert_eq!(
            sse.mangia(
                b": OPENROUTER PROCESSING

"
            ),
            Vec::new()
        );
        assert_eq!(sse.righe(), 2, "due battiti, due righe, zero eventi");

        let mut muto = Sse::nuovo();
        muto.mangia(b"<!doctype html><html>");
        assert_eq!(muto.righe(), 0, "senza un a capo non c’è nessuna riga");
    }

    #[test]
    fn i_commenti_e_i_campi_che_non_servono_si_scartano() {
        let mut sse = Sse::nuovo();
        assert_eq!(
            sse.mangia(b": ping\nevent: message\nid: 7\nretry: 1000\ndata: eccolo\n\n"),
            dati(&["eccolo"])
        );
    }

    /// `[DONE]` esce come evento **e** alza la bandiera. L'evento serve a chi
    /// consuma il flusso in ordine — la fine ha una posizione, non solo un
    /// valore — e la bandiera a chi guarda com'è andata quando è tutto finito.
    #[test]
    fn done_e_un_evento_a_parte_e_non_un_carico() {
        let mut sse = Sse::nuovo();
        assert!(!sse.finito());
        let mut atteso = dati(&["{\"a\":1}"]);
        atteso.push(Evento::Fine);
        assert_eq!(sse.mangia(b"data: {\"a\":1}\n\ndata: [DONE]\n\n"), atteso);
        assert!(sse.finito());
    }

    /// Un byte alla volta è il caso peggiore, e deve dare lo stesso risultato
    /// di un blocco solo: se non lo dà, il difetto dipende dalla velocità della
    /// rete e non si riproduce mai due volte uguale.
    #[test]
    fn un_byte_per_volta_da_lo_stesso_risultato() {
        let flusso = b": ping\r\ndata: {\"a\":1}\r\n\r\ndata:senza spazio\n\ndata: [DONE]\n\n";
        let mut tutto = Sse::nuovo();
        let atteso = tutto.mangia(flusso);
        let mut voluto = dati(&["{\"a\":1}", "senza spazio"]);
        voluto.push(Evento::Fine);
        assert_eq!(atteso, voluto);

        let mut pezzo_a_pezzo = Sse::nuovo();
        let mut avuti = Vec::new();
        for byte in flusso {
            avuti.extend(pezzo_a_pezzo.mangia(&[*byte]));
        }
        assert_eq!(avuti, atteso);
        assert!(pezzo_a_pezzo.finito());
    }

    #[test]
    fn un_carico_che_comincia_con_uno_spazio_lo_tiene() {
        let mut sse = Sse::nuovo();
        // Due spazi: il primo è la cortesia della specifica, il secondo è testo.
        assert_eq!(sse.mangia(b"data:  eccolo\n\n"), dati(&[" eccolo"]));
    }

    /// Una pagina di errore arrivata con lo stato giusto invece di un flusso:
    /// senza il limite, l'avanzo cresce finché c'è memoria.
    #[test]
    fn quel_che_non_e_un_flusso_non_diventa_memoria_infinita() {
        let mut sse = Sse::nuovo();
        assert_eq!(sse.mangia(&vec![b'x'; AVANZO_MASSIMO + 1]), Vec::new());
        assert!(sse.sfondato());
        assert_eq!(sse.mangia(b"data: eccolo\n\n"), Vec::new());
    }

    #[test]
    fn quel_che_resta_a_meta_si_puo_leggere() {
        let mut sse = Sse::nuovo();
        assert_eq!(sse.mangia(b"<html>non era un flusso"), Vec::new());
        assert_eq!(sse.avanzo(), "<html>non era un flusso");
    }
}
